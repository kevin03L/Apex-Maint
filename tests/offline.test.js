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

/** Mock IndexedDB en mémoire pour node:test */
function createMockIndexedDB() {
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
