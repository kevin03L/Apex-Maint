/* ============================================================================
   Apex Maint — Tests unitaires de offline.js (Node.js test runner)
   Vérifications exigées par phase-9.md :
   1. Ordre d'envoi.
   2. Dépendances sur un identifiant temporaire.
   3. Passage en « À résoudre ».
   4. Effacement à la déconnexion.
   5. Pas d'envoi sous une autre identité.
   ============================================================================ */
'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const offline = require('../static/offline.js');

test('U6. Les lectures par statut gardent le résultat et l’ordre de la lecture complète', async () => {
  const reads = [];
  const db = createMockIndexedDB(kind => reads.push(kind));
  offline.setOfflineDBInstance(null);
  for (const [id, status, created_at] of [
    ['p2', 'pending', '2026-10-19T12:00:00Z'],
    ['c1', 'conflict', '2026-10-19T10:00:00Z'],
    ['p1', 'pending', '2026-10-19T10:00:00Z'],
    ['terminee', 'done', '2026-10-19T09:00:00Z'],
    ['c2', 'conflict', '2026-10-19T13:00:00Z']
  ]) {
    const operation = await offline.enqueueOperation({ id, created_at, type: 'test', url: '/', method: 'POST' }, db);
    const opened = await offline.openOfflineDB(db);
    opened.transaction('outbox', 'readwrite').objectStore('outbox').put({ ...operation, status });
  }
  const all = await offline.getAllOperations(db);
  reads.length = 0;
  assert.deepEqual(await offline.getPendingOperations(db), all.filter(op => op.status === 'pending'));
  assert.deepEqual(await offline.getConflictOperations(db), all.filter(op => op.status === 'conflict'));
  assert.deepEqual(await offline.getUnsyncedOperations(db), all.filter(op => ['pending', 'conflict'].includes(op.status)));
  assert.deepEqual(reads, ['by_status', 'by_status', 'all']);
});

test('M3. La déclaration a posteriori conserve tous les champs saisis', async () => {
  const db = createMockIndexedDB();
  offline.setOfflineDBInstance(null);
  const fields = {
    title: 'Réparation', type: 'a_posteriori', solution_report: 'Joint remplacé',
    duration_minutes: 45, technician_ids: ['tech'],
    started_at: '2026-10-18T10:00:00.000Z', completed_at: '2026-10-18T10:45:00.000Z'
  };
  const item = await offline.localCreateIntervention(fields, 'tech', 'site', db);
  const [operation] = await offline.getAllOperations(db);
  for (const key of ['solution_report', 'duration_minutes', 'technician_ids', 'started_at', 'completed_at']) {
    assert.deepEqual(item[key], fields[key]);
    assert.deepEqual(operation.body[key], fields[key]);
  }
  assert.equal(item.status, 'completed');
});

test('M2. La progression locale conserve la prise en charge, le temps et la clôture', async () => {
  const db = createMockIndexedDB();
  offline.setOfflineDBInstance(null);
  const item = await offline.localCreateIntervention({ title: 'Panne' }, 'tech', 'site', db);
  await offline.localTakeIntervention(item.id, 'tech', 'site', db);
  await offline.localRecordTime(item.id, 20, 'tech', 'site', db);
  await offline.localRecordTime(item.id, 45, 'tech', 'site', db);
  await offline.localCompleteIntervention(item.id, { solution_report: 'Réparé' }, 'tech', 'site', db);
  const saved = await offline.getSnapshotItem('interventions', item.id, db);
  assert.equal(saved.duration_minutes, 45);
  assert.equal(saved.status, 'completed');
  assert.equal(saved.solution_report, 'Réparé');
  assert.equal((await offline.getAllOperations(db)).length, 5);
});

test('U22. Les intervenants de l’instantané sont rattachés et le temps est remplacé', async () => {
  const db = createMockIndexedDB();
  offline.setOfflineDBInstance(null);
  await offline.saveSnapshot({
    full: true,
    interventions: [{ id: 'I1', site_id: 'site', duration_minutes: 20, updated_at: '2026-10-18T10:00:00.000Z' }],
    technicians: [{ intervention_id: 'I1', user_id: 'tech', is_lead: true, duration_minutes: 20 }],
    users: [{ id: 'tech', display_name: 'Technicien' }]
  }, 'tech', 'site', db);
  const local = await offline.getLocalIntervention('I1', db);
  assert.equal(local.technicians[0].display_name, 'Technicien');
  await offline.localRecordTime('I1', 45, 'tech', 'site', db);
  const saved = await offline.getSnapshotItem('interventions', 'I1', db);
  assert.equal(saved.duration_minutes, 45);
});

test('U23. La création hors ligne garde le site choisi et le parent', async () => {
  const db = createMockIndexedDB();
  offline.setOfflineDBInstance(null);
  const item = await offline.localCreateIntervention(
    { title: 'Sous-tâche', site_id: 'site-b', parent_id: 'temp_parent' }, 'admin', null, db
  );
  const [operation] = await offline.getAllOperations(db);
  assert.equal(item.site_id, 'site-b');
  assert.equal(operation.body.site_id, 'site-b');
  assert.equal(operation.url, '/api/interventions/temp_parent/sub-interventions');
  assert.equal(operation.depends_on, 'temp_parent');
});

/** Mock IndexedDB en mémoire pour node:test */
function createMockIndexedDB(onRead = () => {}) {
  const stores = new Map();

  function getStore(name) {
    if (!stores.has(name)) {
      stores.set(name, new Map());
    }
    return stores.get(name);
  }

  const db = {
    objectStoreNames: {
      contains(name) {
        return stores.has(name);
      }
    },
    createObjectStore(name, options = {}) {
      const data = getStore(name);
      const keyPath = options.keyPath || 'id';
      const indexes = new Map();

      return {
        name,
        keyPath,
        createIndex(idxName, idxKeyPath) {
          indexes.set(idxName, idxKeyPath);
        }
      };
    },
    transaction(storeNames) {
      const txStores = Array.isArray(storeNames) ? storeNames : [storeNames];
      const tx = {
        oncomplete: null,
        onerror: null,
        objectStore(name) {
          const map = getStore(name);
          return {
            put(item) {
              const req = { onsuccess: null, onerror: null };
              const key = item.key || item.store_key || item.id;
              map.set(key, JSON.parse(JSON.stringify(item)));
              queueMicrotask(() => {
                if (req.onsuccess) req.onsuccess({ target: { result: key } });
                if (tx.oncomplete) tx.oncomplete();
              });
              return req;
            },
            get(key) {
              const req = { onsuccess: null, onerror: null, result: undefined };
              const item = map.get(key);
              req.result = item ? JSON.parse(JSON.stringify(item)) : undefined;
              queueMicrotask(() => {
                if (req.onsuccess) req.onsuccess({ target: { result: req.result } });
              });
              return req;
            },
            getAll() {
              onRead('all');
              const req = { onsuccess: null, onerror: null, result: [] };
              req.result = Array.from(map.values()).map(v => JSON.parse(JSON.stringify(v)));
              queueMicrotask(() => {
                if (req.onsuccess) req.onsuccess({ target: { result: req.result } });
              });
              return req;
            },
            delete(key) {
              const req = { onsuccess: null, onerror: null };
              map.delete(key);
              queueMicrotask(() => {
                if (req.onsuccess) req.onsuccess({ target: {} });
                if (tx.oncomplete) tx.oncomplete();
              });
              return req;
            },
            clear() {
              const req = { onsuccess: null, onerror: null };
              map.clear();
              queueMicrotask(() => {
                if (req.onsuccess) req.onsuccess({ target: {} });
                if (tx.oncomplete) tx.oncomplete();
              });
              return req;
            },
            index(idxName) {
              return {
                getAll(query) {
                  onRead(idxName);
                  const req = { onsuccess: null, onerror: null, result: [] };
                  const all = Array.from(map.values()).map(v => JSON.parse(JSON.stringify(v)));
                  if (query !== undefined) {
                    req.result = all.filter(it => it.collection === query || it.status === query);
                  } else {
                    req.result = all;
                  }
                  queueMicrotask(() => {
                    if (req.onsuccess) req.onsuccess({ target: { result: req.result } });
                  });
                  return req;
                }
              };
            }
          };
        }
      };
      return tx;
    }
  };

  return {
    open(name, version) {
      const req = { onupgradeneeded: null, onsuccess: null, onerror: null };
      queueMicrotask(() => {
        if (req.onupgradeneeded) {
          req.onupgradeneeded({ target: { result: db } });
        }
        if (req.onsuccess) {
          req.onsuccess({ target: { result: db } });
        }
      });
      return req;
    }
  };
}

test('1. Ordre d\'envoi séquentiel des opérations', async () => {
  const mockIDB = createMockIndexedDB();
  offline.setOfflineDBInstance(null);

  await offline.enqueueOperation({
    id: 'op-1',
    type: 'take_intervention',
    url: '/api/interventions/INT-1/take',
    method: 'POST',
    created_at: '2026-10-18T10:00:00.000Z',
    user_id: 'user-tech-1',
    site_id: 'site-1'
  }, mockIDB);

  await offline.enqueueOperation({
    id: 'op-2',
    type: 'record_time',
    url: '/api/interventions/INT-1/technicians/user-tech-1/time',
    method: 'PUT',
    body: { duration_minutes: 45 },
    created_at: '2026-10-18T10:30:00.000Z',
    user_id: 'user-tech-1',
    site_id: 'site-1'
  }, mockIDB);

  await offline.enqueueOperation({
    id: 'op-3',
    type: 'complete_intervention',
    url: '/api/interventions/INT-1/complete',
    method: 'POST',
    created_at: '2026-10-18T11:00:00.000Z',
    user_id: 'user-tech-1',
    site_id: 'site-1'
  }, mockIDB);

  const sentOrder = [];
  const fakeFetch = async (url, options) => {
    sentOrder.push({ url, idempotency: options.headers['Idempotency-Key'] });
    return {
      status: 200,
      json: async () => ({ ok: true })
    };
  };

  const report = await offline.syncOutbox(fakeFetch, 'user-tech-1', 'site-1', mockIDB);
  assert.equal(report.sent, 3);
  assert.equal(report.conflicts, 0);
  assert.deepEqual(sentOrder.map(s => s.idempotency), ['op-1', 'op-2', 'op-3']);
});

test('2. Dépendances sur un identifiant temporaire', async () => {
  const mockIDB = createMockIndexedDB();
  offline.setOfflineDBInstance(null);

  // Étape A : Création locale d'une intervention hors-ligne avec identifiant temporaire
  const newInt = await offline.localCreateIntervention({
    title: 'Moteur en panne',
    description: 'Bruit anormal',
    equipment_id: 'EQ-42',
    occurred_at: '2026-10-18T10:00:00.000Z'
  }, 'user-tech-1', 'site-1', mockIDB);

  const tempId = newInt.id;
  assert.match(tempId, /^temp_/);

  // Étape B : Action dépendante (ex: prise en charge)
  await offline.enqueueOperation({
    id: 'op-take',
    type: 'take_intervention',
    url: `/api/interventions/${tempId}/take`,
    method: 'POST',
    depends_on: tempId,
    created_at: '2026-10-18T10:05:00.000Z',
    user_id: 'user-tech-1',
    site_id: 'site-1'
  }, mockIDB);

  const callLog = [];
  const fakeFetch = async (url, options) => {
    callLog.push({ url, options });
    if (url === '/api/interventions') {
      return {
        status: 201,
        json: async () => ({ id: 'INT-REAL-999', code: 'INT-2026-0999', title: 'Moteur en panne' })
      };
    }
    return {
      status: 200,
      json: async () => ({ ok: true })
    };
  };

  const report = await offline.syncOutbox(fakeFetch, 'user-tech-1', 'site-1', mockIDB);
  assert.equal(report.sent, 2);
  assert.equal(callLog.length, 2);
  // Vérifie que la 2ème opération a été réécrite avec l'identifiant réel 'INT-REAL-999'
  assert.equal(callLog[1].url, '/api/interventions/INT-REAL-999/take');
});

test('3. Passage en « À résoudre » en cas de refus ou conflit (409 / 412)', async () => {
  const mockIDB = createMockIndexedDB();
  offline.setOfflineDBInstance(null);

  await offline.enqueueOperation({
    id: 'op-conflict-1',
    type: 'patch_intervention',
    url: '/api/interventions/INT-BLOCKED',
    method: 'PATCH',
    body: { solution_report: 'Remplacement fusible' },
    if_unmodified_since: '2026-10-18T09:00:00.000Z',
    created_at: '2026-10-18T10:00:00.000Z',
    user_id: 'user-tech-1',
    site_id: 'site-1'
  }, mockIDB);

  const fakeFetch = async () => {
    return {
      status: 412,
      json: async () => ({ error: true, message: 'Conflit de modification (déjà modifié par un autre utilisateur)' })
    };
  };

  const report = await offline.syncOutbox(fakeFetch, 'user-tech-1', 'site-1', mockIDB);
  assert.equal(report.conflicts, 1);
  assert.equal(report.sent, 0);

  const conflicts = await offline.getConflictOperations(mockIDB);
  assert.equal(conflicts.length, 1);
  assert.equal(conflicts[0].status, 'conflict');
  assert.equal(conflicts[0].error_status, 412);
  assert.match(conflicts[0].error_message, /Conflit de modification/);
});

test('4. Effacement complet à la déconnexion', async () => {
  const mockIDB = createMockIndexedDB();
  offline.setOfflineDBInstance(null);

  await offline.setMeta('user_id', 'user-tech-1', mockIDB);
  await offline.putSnapshotItem('equipments', 'EQ-1', { id: 'EQ-1', name: 'Presse' }, mockIDB);
  await offline.enqueueOperation({
    id: 'op-x',
    type: 'take_intervention',
    url: '/api/interventions/1/take',
    user_id: 'user-tech-1'
  }, mockIDB);

  // Vérifier présence
  assert.equal(await offline.getMeta('user_id', mockIDB), 'user-tech-1');
  assert.ok(await offline.getSnapshotItem('equipments', 'EQ-1', mockIDB));
  const pending = await offline.getPendingOperations(mockIDB);
  assert.equal(pending.length, 1);

  // Effacement
  await offline.clearAllOfflineData(mockIDB);

  assert.equal(await offline.getMeta('user_id', mockIDB), null);
  assert.equal(await offline.getSnapshotItem('equipments', 'EQ-1', mockIDB), null);
  const remaining = await offline.getAllOperations(mockIDB);
  assert.equal(remaining.length, 0);
});

test('5. Pas d\'envoi sous une autre identité', async () => {
  const mockIDB = createMockIndexedDB();
  offline.setOfflineDBInstance(null);

  await offline.enqueueOperation({
    id: 'op-other-user',
    type: 'take_intervention',
    url: '/api/interventions/1/take',
    user_id: 'technicien-paul',
    site_id: 'site-1',
    created_at: '2026-10-18T10:00:00.000Z'
  }, mockIDB);

  let wasFetchCalled = false;
  const fakeFetch = async () => {
    wasFetchCalled = true;
    return { status: 200, json: async () => ({}) };
  };

  // Tentative de synchronisation par un autre utilisateur (technicien-marc)
  const report = await offline.syncOutbox(fakeFetch, 'technicien-marc', 'site-1', mockIDB);
  assert.equal(report.stoppedReason, 'USER_MISMATCH');
  assert.equal(wasFetchCalled, false);
  assert.equal(report.sent, 0);
});

test('6. Changement d\'identité détecté avant tout usage du cache', async () => {
  const mockIDB = createMockIndexedDB();
  offline.setOfflineDBInstance(null);

  // Cache vierge : pas de changement.
  assert.equal((await offline.checkCacheOwner('user-b', 'site-b', mockIDB)).changed, false);

  // Instantané chargé par A (site A), avec une opération non envoyée.
  await offline.saveSnapshot({ full: true, generated_at: '2026-10-18T10:00:00.000Z', equipments: [{ id: 'EQ-A' }] }, 'user-a', 'site-a', mockIDB);
  await offline.enqueueOperation({ id: 'op-a', type: 'take_intervention', url: '/api/interventions/1/take', user_id: 'user-a', site_id: 'site-a' }, mockIDB);

  // Même identité : aucun changement.
  assert.equal((await offline.checkCacheOwner('user-a', 'site-a', mockIDB)).changed, false);
  // Même utilisateur, autre site : changement.
  assert.equal((await offline.checkCacheOwner('user-a', 'site-b', mockIDB)).changed, true);

  // B se connecte : changement, et la file de A est remontée pour l'export.
  const check = await offline.checkCacheOwner('user-b', 'site-b', mockIDB);
  assert.equal(check.changed, true);
  assert.deepEqual(check.foreignOperations.map(op => op.id), ['op-a']);
});

test('7. Les opérations « À résoudre » comptent parmi les non synchronisées', async () => {
  const mockIDB = createMockIndexedDB();
  offline.setOfflineDBInstance(null);

  await offline.enqueueOperation({ id: 'op-1', type: 'take_intervention', url: '/api/interventions/1/take', user_id: 'u', created_at: '2026-10-18T10:00:00.000Z' }, mockIDB);
  await offline.enqueueOperation({ id: 'op-2', type: 'take_intervention', url: '/api/interventions/2/take', user_id: 'u', created_at: '2026-10-18T10:01:00.000Z' }, mockIDB);
  await offline.markOperationConflict('op-1', 'Clôturée entre-temps', 409, null, mockIDB);
  await offline.markOperationConflict('op-2', 'Clôturée entre-temps', 409, null, mockIDB);

  assert.equal((await offline.getPendingOperations(mockIDB)).length, 0);
  assert.equal((await offline.getUnsyncedOperations(mockIDB)).length, 2);
});
