/* ============================================================================
   Apex Maint — Moteur Hors-ligne & IndexedDB (Phase 9 : Terrain)
   Règles impératives :
   - File d'attente d'opérations séquentielle (outbox).
   - En-têtes Idempotency-Key, X-Occurred-At et If-Unmodified-Since.
   - Gestion des dépendances sur identifiants temporaires.
   - Détection et enregistrement des conflits (« À résoudre »).
   - Effacement sécurisé des données lors de la déconnexion.
   - Séparation stricte des utilisateurs (pas d'envoi sous une autre identité).
   ============================================================================ */
'use strict';

/* global globalThis, module */

const DB_NAME = 'apex_maint_offline';
const DB_VERSION = 1;

let idbInstance = null;

/** Détermine l'implémentation d'IndexedDB disponible (navigateur ou mock Node.js). */
function getIDB() {
  if (typeof indexedDB !== 'undefined') return indexedDB;
  if (typeof globalThis !== 'undefined' && globalThis.indexedDB) return globalThis.indexedDB;
  return null;
}

/** Génère un identifiant unique (UUID v4/v7 ou pseudo-aléatoire sécurisé). */
function generateUuid() {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, c => {
    const r = (Math.random() * 16) | 0;
    const v = c === 'x' ? r : (r & 0x3) | 0x8;
    return v.toString(16);
  });
}

/** Ouvre ou réutilise la connexion IndexedDB. */
function openOfflineDB(customIDB) {
  return new Promise((resolve, reject) => {
    if (idbInstance) {
      resolve(idbInstance);
      return;
    }
    const idb = customIDB || getIDB();
    if (!idb) {
      reject(new Error('IndexedDB n\'est pas supporté dans cet environnement.'));
      return;
    }

    const req = idb.open(DB_NAME, DB_VERSION);
    req.onupgradeneeded = event => {
      const db = event.target.result;
      if (!db.objectStoreNames.contains('meta')) {
        db.createObjectStore('meta', { keyPath: 'key' });
      }
      if (!db.objectStoreNames.contains('snapshot')) {
        const snapStore = db.createObjectStore('snapshot', { keyPath: 'store_key' });
        snapStore.createIndex('by_collection', 'collection', { unique: false });
        snapStore.createIndex('by_item_id', 'id', { unique: false });
      }
      if (!db.objectStoreNames.contains('outbox')) {
        const outboxStore = db.createObjectStore('outbox', { keyPath: 'id' });
        outboxStore.createIndex('by_status', 'status', { unique: false });
        outboxStore.createIndex('by_created_at', 'created_at', { unique: false });
        outboxStore.createIndex('by_temp_id', 'temp_id', { unique: false });
        outboxStore.createIndex('by_depends_on', 'depends_on', { unique: false });
      }
    };

    req.onsuccess = event => {
      idbInstance = event.target.result;
      resolve(idbInstance);
    };

    req.onerror = event => {
      reject(event.target.error || new Error('Erreur d\'ouverture IndexedDB'));
    };
  });
}

/** Réinitialise l'instance (utilisé pour les tests). */
function setOfflineDBInstance(instance) {
  idbInstance = instance;
}

/** Enregistre une valeur dans le magasin 'meta'. */
async function setMeta(key, value, customIDB) {
  const db = await openOfflineDB(customIDB);
  return new Promise((resolve, reject) => {
    const tx = db.transaction('meta', 'readwrite');
    const store = tx.objectStore('meta');
    const req = store.put({ key, value });
    req.onsuccess = () => resolve();
    req.onerror = e => reject(e.target.error);
  });
}

/** Récupère une valeur du magasin 'meta'. */
async function getMeta(key, customIDB) {
  const db = await openOfflineDB(customIDB);
  return new Promise((resolve, reject) => {
    const tx = db.transaction('meta', 'readonly');
    const store = tx.objectStore('meta');
    const req = store.get(key);
    req.onsuccess = () => resolve(req.result ? req.result.value : null);
    req.onerror = e => reject(e.target.error);
  });
}

/** Efface l'ensemble des données hors-ligne (déconnexion ou changement d'utilisateur). */
async function clearAllOfflineData(customIDB) {
  const db = await openOfflineDB(customIDB);
  return new Promise((resolve, reject) => {
    const tx = db.transaction(['meta', 'snapshot', 'outbox'], 'readwrite');
    tx.objectStore('meta').clear();
    tx.objectStore('snapshot').clear();
    tx.objectStore('outbox').clear();
    tx.oncomplete = () => resolve();
    tx.onerror = e => reject(e.target.error);
  });
}

/** Enregistre l'instantané reçu du serveur dans le magasin 'snapshot'. */
async function saveSnapshot(snapshotData, currentUserId, currentSiteId, customIDB) {
  const db = await openOfflineDB(customIDB);
  return new Promise((resolve, reject) => {
    const tx = db.transaction(['meta', 'snapshot'], 'readwrite');
    const metaStore = tx.objectStore('meta');
    const snapStore = tx.objectStore('snapshot');

    metaStore.put({ key: 'last_sync_at', value: snapshotData.generated_at || new Date().toISOString() });
    metaStore.put({ key: 'user_id', value: currentUserId });
    metaStore.put({ key: 'site_id', value: currentSiteId });

    if (snapshotData.full) {
      snapStore.clear();
    }

    const collections = [
      'equipments',
      'production_lines',
      'counters',
      'parts',
      'users',
      'interventions',
      'technicians'
    ];

    const writeCollections = () => {
      for (const col of collections) {
        const items = snapshotData[col] || [];
        for (const item of items) {
          // En cas de suppression logique dans un delta
          if (item.deleted_at) {
            const storeKey = `${col}:${item.id || (item.intervention_id + '_' + item.user_id)}`;
            snapStore.delete(storeKey);
          } else {
            const itemId = item.id || (item.intervention_id + '_' + item.user_id);
            const storeKey = `${col}:${itemId}`;
            snapStore.put({
              store_key: storeKey,
              collection: col,
              id: itemId,
              data: item
            });
          }
        }
      }
    };
    if (!snapshotData.full) {
      // Les affectations des interventions renvoyées remplacent la liste précédente.
      const replaced = new Set((snapshotData.interventions || []).map(item => item.id));
      const req = snapStore.index('by_collection').getAll('technicians');
      req.onsuccess = () => {
        for (const row of req.result || []) {
          if (replaced.has(row.data.intervention_id)) snapStore.delete(row.store_key);
        }
        writeCollections();
      };
    } else {
      writeCollections();
    }

    // Si delta avec intervention_ids : purger les interventions non listées
    if (!snapshotData.full && Array.isArray(snapshotData.intervention_ids)) {
      const allowedIds = new Set(snapshotData.intervention_ids);
      const req = snapStore.index('by_collection').getAll('interventions');
      req.onsuccess = () => {
        const rows = req.result || [];
        for (const row of rows) {
          if (!allowedIds.has(row.data.id)) {
            snapStore.delete(row.store_key);
          }
        }
      };
    }

    tx.oncomplete = () => resolve();
    tx.onerror = e => reject(e.target.error);
  });
}

/** Récupère tous les éléments d'une collection depuis l'instantané local. */
async function getSnapshotCollection(collection, customIDB) {
  const db = await openOfflineDB(customIDB);
  return new Promise((resolve, reject) => {
    const tx = db.transaction('snapshot', 'readonly');
    const store = tx.objectStore('snapshot');
    const idx = store.index('by_collection');
    const req = idx.getAll(collection);
    req.onsuccess = () => {
      const list = (req.result || []).map(r => r.data);
      resolve(list);
    };
    req.onerror = e => reject(e.target.error);
  });
}

/** Récupère un élément particulier de l'instantané local. */
async function getSnapshotItem(collection, id, customIDB) {
  const db = await openOfflineDB(customIDB);
  return new Promise((resolve, reject) => {
    const tx = db.transaction('snapshot', 'readonly');
    const store = tx.objectStore('snapshot');
    const req = store.get(`${collection}:${id}`);
    req.onsuccess = () => resolve(req.result ? req.result.data : null);
    req.onerror = e => reject(e.target.error);
  });
}

/**
 * Lit une intervention locale avec ses intervenants. Le serveur livre les affectations dans la
 * collection séparée « technicians » : elles sont rattachées ici à l'intervention, avec le nom
 * de l'intervenant tiré de la collection « users ».
 */
async function getLocalIntervention(id, customIDB) {
  const item = await getSnapshotItem('interventions', id, customIDB);
  if (!item || Array.isArray(item.technicians)) return item;
  const [technicians, users] = await Promise.all([
    getSnapshotCollection('technicians', customIDB),
    getSnapshotCollection('users', customIDB)
  ]);
  const names = new Map(users.map(u => [u.id, u.display_name]));
  item.technicians = technicians
    .filter(t => t.intervention_id === id)
    .map(t => ({ ...t, display_name: names.get(t.user_id) || t.user_id }));
  return item;
}

/** Met à jour un élément dans l'instantané local. */
async function putSnapshotItem(collection, id, data, customIDB) {
  const db = await openOfflineDB(customIDB);
  return new Promise((resolve, reject) => {
    const tx = db.transaction('snapshot', 'readwrite');
    const store = tx.objectStore('snapshot');
    const storeKey = `${collection}:${id}`;
    const req = store.put({
      store_key: storeKey,
      collection,
      id,
      data
    });
    req.onsuccess = () => resolve();
    req.onerror = e => reject(e.target.error);
  });
}

// ----------------------------------------------------------------------------
// GESTION DE LA FILE D'ATTENTE (OUTBOX)
// ----------------------------------------------------------------------------

/**
 * Enfile une nouvelle opération à destination du serveur.
 * Paramètres :
 * - type : 'create_intervention' | 'take_intervention' | 'record_time' | 'patch_intervention' | 'complete_intervention'
 * - url : chemin de l'API (ex: '/api/interventions')
 * - method : POST | PUT | PATCH
 * - body : corps JSON
 * - temp_id : identifiant provisoire si création
 * - depends_on : identifiant temporaire dont dépend cette opération
 * - user_id : identifiant de l'utilisateur ayant réalisé l'action
 * - site_id : identifiant du site
 * - occurred_at : date ISO réelle de l'action
 * - if_unmodified_since : version serveur pour détection de conflit
 */
async function enqueueOperation(opParams, customIDB) {
  const db = await openOfflineDB(customIDB);
  const occurredAt = opParams.occurred_at || new Date().toISOString();
  const createdAt = opParams.created_at || occurredAt;
  const op = {
    id: opParams.id || generateUuid(),
    type: opParams.type,
    url: opParams.url,
    method: opParams.method || 'POST',
    body: opParams.body || null,
    temp_id: opParams.temp_id || null,
    depends_on: opParams.depends_on || null,
    user_id: opParams.user_id,
    site_id: opParams.site_id,
    occurred_at: occurredAt,
    if_unmodified_since: opParams.if_unmodified_since || null,
    created_at: createdAt,
    status: 'pending', // 'pending' | 'conflict' | 'synced'
    error_message: null,
    error_status: null,
    conflict_data: null
  };

  return new Promise((resolve, reject) => {
    const tx = db.transaction('outbox', 'readwrite');
    const store = tx.objectStore('outbox');
    const req = store.put(op);
    req.onsuccess = () => resolve(op);
    req.onerror = e => reject(e.target.error);
  });
}

/** Récupère toutes les opérations de l'outbox, triées chronologiquement. */
async function getAllOperations(customIDB, status) {
  const db = await openOfflineDB(customIDB);
  return new Promise((resolve, reject) => {
    const tx = db.transaction('outbox', 'readonly');
    const store = tx.objectStore('outbox');
    const req = status === undefined ? store.getAll() : store.index('by_status').getAll(status);
    req.onsuccess = () => {
      const items = (req.result || []).sort((a, b) => a.created_at.localeCompare(b.created_at));
      resolve(items);
    };
    req.onerror = e => reject(e.target.error);
  });
}

/** Récupère les opérations en attente (status === 'pending'). */
async function getPendingOperations(customIDB) {
  return getAllOperations(customIDB, 'pending');
}

/** Récupère les opérations en conflit (« À résoudre »). */
async function getConflictOperations(customIDB) {
  return getAllOperations(customIDB, 'conflict');
}

/** Récupère les opérations non synchronisées : en attente et « À résoudre ». */
async function getUnsyncedOperations(customIDB) {
  const all = await getAllOperations(customIDB);
  return all.filter(op => op.status === 'pending' || op.status === 'conflict');
}

/**
 * Vérifie que le cache local appartient bien à l'identité connectée (utilisateur et site).
 * Retourne { changed, foreignOperations } : changed vaut true si l'instantané ou la file
 * proviennent d'une autre identité ; foreignOperations liste les opérations non synchronisées
 * d'un autre utilisateur, à proposer à l'export avant effacement. Un cache vierge n'est pas
 * un changement.
 */
async function checkCacheOwner(currentUserId, currentSiteId, customIDB) {
  const storedUserId = await getMeta('user_id', customIDB);
  const storedSiteId = await getMeta('site_id', customIDB);
  const unsynced = await getUnsyncedOperations(customIDB);
  const foreignOperations = unsynced.filter(op => op.user_id && op.user_id !== currentUserId);
  const ownerChanged = storedUserId !== null && storedUserId !== undefined
    && (storedUserId !== currentUserId || (storedSiteId || null) !== (currentSiteId || null));
  return { changed: ownerChanged || foreignOperations.length > 0, foreignOperations };
}

/** Supprime une opération de l'outbox. */
async function removeOperation(opId, customIDB) {
  const db = await openOfflineDB(customIDB);
  return new Promise((resolve, reject) => {
    const tx = db.transaction('outbox', 'readwrite');
    const store = tx.objectStore('outbox');
    const req = store.delete(opId);
    req.onsuccess = () => resolve();
    req.onerror = e => reject(e.target.error);
  });
}

/** Marque une opération en statut de conflit. */
async function markOperationConflict(opId, errorMessage, errorStatus, conflictData, customIDB) {
  const db = await openOfflineDB(customIDB);
  return new Promise((resolve, reject) => {
    const tx = db.transaction('outbox', 'readwrite');
    const store = tx.objectStore('outbox');
    const req = store.get(opId);
    req.onsuccess = () => {
      const op = req.result;
      if (!op) {
        resolve();
        return;
      }
      op.status = 'conflict';
      op.error_message = errorMessage || 'Erreur de synchronisation';
      op.error_status = errorStatus || null;
      op.conflict_data = conflictData || null;
      store.put(op);
      resolve(op);
    };
    req.onerror = e => reject(e.target.error);
  });
}

/**
 * Réécrit les références d'un identifiant temporaire vers un identifiant définitif
 * dans l'outbox et dans le snapshot local.
 */
async function rewriteTemporaryId(tempId, realId, realItem, customIDB) {
  const db = await openOfflineDB(customIDB);
  return new Promise((resolve, reject) => {
    const tx = db.transaction(['outbox', 'snapshot'], 'readwrite');
    const outboxStore = tx.objectStore('outbox');
    const snapStore = tx.objectStore('snapshot');

    // 1. Réécriture dans l'outbox
    const req = outboxStore.getAll();
    req.onsuccess = () => {
      const ops = req.result || [];
      for (const op of ops) {
        let changed = false;
        if (op.depends_on === tempId) {
          op.depends_on = realId;
          changed = true;
        }
        if (op.url && op.url.includes(tempId)) {
          op.url = op.url.replace(tempId, realId);
          changed = true;
        }
        if (op.body && typeof op.body === 'object') {
          if (op.body.intervention_id === tempId) {
            op.body.intervention_id = realId;
            changed = true;
          }
        }
        if (changed) {
          outboxStore.put(op);
        }
      }
    };

    // 2. Réécriture dans le snapshot local
    const oldKey = `interventions:${tempId}`;
    const newKey = `interventions:${realId}`;
    snapStore.get(oldKey).onsuccess = e => {
      const row = e.target.result;
      if (row) {
        snapStore.delete(oldKey);
        const updatedData = realItem || Object.assign({}, row.data, { id: realId });
        snapStore.put({
          store_key: newKey,
          collection: 'interventions',
          id: realId,
          data: updatedData
        });
      }
    };

    tx.oncomplete = () => resolve();
    tx.onerror = e => reject(e.target.error);
  });
}

// ----------------------------------------------------------------------------
// SYNCHRONISATION COMPLÈTE
// ----------------------------------------------------------------------------

/** Chaîne les préconditions sur la réponse serveur et les conserve pour une reprise. */
async function acknowledgeOperation(op, serverItem, pendingOps, customIDB) {
  const db = await openOfflineDB(customIDB);
  return new Promise((resolve, reject) => {
    const tx = db.transaction(['outbox', 'snapshot'], 'readwrite');
    const outbox = tx.objectStore('outbox');
    const created = op.temp_id && op.temp_id.startsWith('temp_');
    const interventionId = serverItem && (op.type === 'record_time' ? serverItem.intervention_id : serverItem.id);
    if (serverItem && serverItem.updated_at && (created || op.if_unmodified_since)
        && interventionId === (created ? serverItem.id : op.depends_on)) {
      const rebase = next => next.id !== op.id && next.status === 'pending'
        && next.depends_on === interventionId
        && (created || next.if_unmodified_since === op.if_unmodified_since);
      outbox.getAll().onsuccess = e => {
        for (const next of e.target.result || []) {
          if (rebase(next)) {
            next.if_unmodified_since = serverItem.updated_at;
            outbox.put(next);
          }
        }
      };
      for (const next of pendingOps) {
        if (rebase(next)) next.if_unmodified_since = serverItem.updated_at;
      }
      const snapshot = tx.objectStore('snapshot');
      snapshot.get(`interventions:${interventionId}`).onsuccess = e => {
        const row = e.target.result;
        if (row) {
          // Garder les champs locaux encore en attente, mais mémoriser la version acquittée.
          row.data.server_updated_at = serverItem.updated_at;
          snapshot.put(row);
        }
      };
    }
    outbox.delete(op.id);
    tx.oncomplete = () => resolve();
    tx.onerror = e => reject(e.target.error);
  });
}

/**
 * Exécute la synchronisation séquentielle de la file d'attente vers le serveur.
 * Arguments :
 * - fetchFn : fonction fetch personnalisée ou native (retournant une réponse type Fetch Response)
 * - currentUserId : identifiant de l'utilisateur connecté
 * - currentSiteId : identifiant du site courant
 * Renvoie un rapport { sent: number, conflicts: number, blocked: number, stoppedReason: string|null }
 */
async function syncOutbox(fetchFn, currentUserId, currentSiteId, customIDB) {
  const pendingOps = await getPendingOperations(customIDB);
  const report = {
    total: pendingOps.length,
    sent: 0,
    conflicts: 0,
    blocked: 0,
    stoppedReason: null
  };

  if (pendingOps.length === 0) {
    return report;
  }

  // Vérifier la cohérence de l'utilisateur
  for (const op of pendingOps) {
    if (op.user_id && op.user_id !== currentUserId) {
      report.stoppedReason = 'USER_MISMATCH';
      return report;
    }
  }

  // Ensemble des identifiants temporaires ou interventions en conflit (qui bloquent les opérations dépendantes)
  const blockedIds = new Set();
  const existingConflicts = await getConflictOperations(customIDB);
  for (const c of existingConflicts) {
    if (c.temp_id) blockedIds.add(c.temp_id);
    if (c.depends_on) blockedIds.add(c.depends_on);
  }

  for (const op of pendingOps) {
    // Si l'opération dépend d'une intervention bloquée par un conflit antérieur
    if (op.depends_on && blockedIds.has(op.depends_on)) {
      report.blocked++;
      continue;
    }

    const headers = {
      'Content-Type': 'application/json',
      'Idempotency-Key': op.id,
      'X-Occurred-At': op.occurred_at
    };

    if (op.if_unmodified_since) {
      headers['If-Unmodified-Since'] = op.if_unmodified_since;
    }

    let response;
    try {
      response = await fetchFn(op.url, {
        method: op.method,
        headers,
        body: op.body ? JSON.stringify(op.body) : undefined
      });
    } catch (netErr) {
      // Erreur réseau : arrêt de la boucle de synchronisation, la file reste en attente
      report.stoppedReason = 'NETWORK_ERROR';
      break;
    }

    // Gestion du code de retour
    if (response.status === 401) {
      report.stoppedReason = 'SESSION_EXPIRED';
      break;
    }

    if (response.status >= 200 && response.status < 300) {
      // Succès
      let resData = null;
      try {
        resData = await response.json();
      } catch (_) {
        resData = {};
      }

      // Si c'était une création avec identifiant temporaire
      if (op.temp_id && resData && resData.id) {
        await rewriteTemporaryId(op.temp_id, resData.id, resData, customIDB);
        // Répercuter également sur les opérations suivantes en mémoire dans pendingOps
        for (const nextOp of pendingOps) {
          if (nextOp.depends_on === op.temp_id) {
            nextOp.depends_on = resData.id;
          }
          if (nextOp.url && nextOp.url.includes(op.temp_id)) {
            nextOp.url = nextOp.url.replace(op.temp_id, resData.id);
          }
          if (nextOp.body && typeof nextOp.body === 'object') {
            if (nextOp.body.intervention_id === op.temp_id) {
              nextOp.body.intervention_id = resData.id;
            }
          }
        }
      }

      await acknowledgeOperation(op, resData, pendingOps, customIDB);
      report.sent++;
    } else if (response.status === 409 || response.status === 403 || response.status === 400 || response.status === 412 || response.status === 422) {
      // Conflit ou refus métier -> passage en "À résoudre"
      let errorMsg = `Erreur ${response.status}`;
      let errBody = null;
      try {
        errBody = await response.json();
        if (errBody && errBody.message) {
          errorMsg = errBody.message;
        }
      } catch (_) {
        // Pas de JSON
      }

      await markOperationConflict(op.id, errorMsg, response.status, errBody, customIDB);
      report.conflicts++;

      // Bloquer les opérations dépendant de la même intervention
      if (op.temp_id) blockedIds.add(op.temp_id);
      if (op.depends_on) blockedIds.add(op.depends_on);
    } else {
      // Autre erreur serveur (5xx ou inattendu) : on stoppe la file pour réessayer plus tard
      report.stoppedReason = `SERVER_ERROR_${response.status}`;
      break;
    }
  }

  return report;
}

// ----------------------------------------------------------------------------
// ACTIONS LOCALES TERRAIN (AVEC APPLICATION LOCALE DIRECTE)
// ----------------------------------------------------------------------------

/**
 * Déclare une intervention hors ligne :
 * - Crée un enregistrement temporaire dans l'instantané local
 * - Enfile l'opération POST /api/interventions
 */
async function localCreateIntervention(params, currentUserId, currentSiteId, customIDB) {
  const tempId = 'temp_' + generateUuid();
  const occurredAt = params.occurred_at || new Date().toISOString();
  // Le site choisi dans le formulaire (administrateur) prime sur celui de l'utilisateur.
  const siteId = params.site_id || currentSiteId;
  const parentId = params.parent_id || null;

  const newIntervention = {
    id: tempId,
    site_id: siteId,
    parent_id: parentId,
    code: 'Référence provisoire',
    title: params.title || 'Intervention',
    description: params.description || '',
    type: params.type || 'corrective',
    priority: params.priority || 'medium',
    status: params.type === 'a_posteriori' ? 'completed' : 'open',
    machine_state: params.machine_state || 'unknown',
    equipment_id: params.equipment_id || null,
    reported_at: params.reported_at || occurredAt,
    started_at: params.started_at || null,
    completed_at: params.completed_at || null,
    solution_report: params.solution_report ?? null,
    duration_minutes: params.duration_minutes ?? 0,
    technician_ids: params.technician_ids || [],
    updated_at: occurredAt,
    server_updated_at: null,
    deleted_at: null
  };

  await putSnapshotItem('interventions', tempId, newIntervention, customIDB);

  await enqueueOperation({
    type: 'create_intervention',
    // Une sous-intervention passe par la route de son parent, éventuellement encore provisoire :
    // l'identifiant temporaire de l'URL est réécrit après la synchronisation du parent.
    url: parentId ? `/api/interventions/${parentId}/sub-interventions` : '/api/interventions',
    method: 'POST',
    depends_on: parentId && parentId.startsWith('temp_') ? parentId : null,
    body: {
      site_id: siteId,
      equipment_id: params.equipment_id || null,
      title: params.title,
      description: params.description || null,
      type: params.type || 'corrective',
      priority: params.priority || 'medium',
      machine_state: params.machine_state || null,
      reported_at: params.reported_at || occurredAt,
      started_at: params.started_at || null,
      completed_at: params.completed_at || null,
      solution_report: params.solution_report ?? null,
      duration_minutes: params.duration_minutes ?? null,
      technician_ids: params.technician_ids || []
    },
    temp_id: tempId,
    user_id: currentUserId,
    site_id: currentSiteId,
    occurred_at: occurredAt
  }, customIDB);

  return newIntervention;
}

/**
 * Prise en charge d'une intervention hors ligne :
 * - Statut passe à 'in_progress', started_at renseigné
 * - Enfile l'opération POST /api/interventions/:id/take
 */
async function localTakeIntervention(interventionId, currentUserId, currentSiteId, customIDB) {
  const item = await getSnapshotItem('interventions', interventionId, customIDB);
  const occurredAt = new Date().toISOString();

  if (item) {
    item.status = 'in_progress';
    if (!item.started_at) item.started_at = occurredAt;
    if (item.server_updated_at === undefined) item.server_updated_at = item.updated_at || null;
    item.updated_at = occurredAt;
    await putSnapshotItem('interventions', interventionId, item, customIDB);
  }

  await enqueueOperation({
    type: 'take_intervention',
    url: `/api/interventions/${interventionId}/take`,
    method: 'POST',
    body: {},
    depends_on: interventionId,
    user_id: currentUserId,
    site_id: currentSiteId,
    occurred_at: occurredAt,
    if_unmodified_since: interventionId.startsWith('temp_') || !item ? null : item.server_updated_at
  }, customIDB);
}

/**
 * Saisie de temps hors ligne :
 * - Enfile PUT /api/interventions/:id/technicians/:userId/time
 */
async function localRecordTime(interventionId, durationMinutes, currentUserId, currentSiteId, customIDB) {
  const occurredAt = new Date().toISOString();
  const item = await getLocalIntervention(interventionId, customIDB);
  if (item) {
    const technicians = item.technicians;
    const technician = technicians.find(t => t.user_id === currentUserId);
    const previous = technician ? technician.duration_minutes || 0 : 0;
    item.duration_minutes = (item.duration_minutes || 0) - previous + (durationMinutes || 0);
    if (technician) technician.duration_minutes = durationMinutes;
    else technicians.push({ user_id: currentUserId, duration_minutes: durationMinutes });
    item.technicians = technicians;
    if (item.server_updated_at === undefined) item.server_updated_at = item.updated_at || null;
    item.updated_at = occurredAt;
    await putSnapshotItem('interventions', interventionId, item, customIDB);
  }

  await enqueueOperation({
    type: 'record_time',
    url: `/api/interventions/${interventionId}/technicians/${currentUserId}/time`,
    method: 'PUT',
    body: { duration_minutes: durationMinutes },
    depends_on: interventionId,
    user_id: currentUserId,
    site_id: currentSiteId,
    occurred_at: occurredAt,
    if_unmodified_since: interventionId.startsWith('temp_') || !item ? null : item.server_updated_at
  }, customIDB);
}

/**
 * Mise à jour du compte rendu ou description hors ligne :
 * - Enfile PATCH /api/interventions/:id avec If-Unmodified-Since
 */
async function localPatchIntervention(interventionId, fields, currentUserId, currentSiteId, customIDB) {
  const item = await getSnapshotItem('interventions', interventionId, customIDB);
  const occurredAt = new Date().toISOString();
  // L'heure locale sert à l'affichage ; seule la version serveur protège le PATCH.
  const ifUnmodifiedSince = interventionId.startsWith('temp_') || !item ? null
    : item.server_updated_at !== undefined ? item.server_updated_at : item.updated_at;

  if (item) {
    item.server_updated_at = ifUnmodifiedSince;
    if (fields.description !== undefined) item.description = fields.description;
    if (fields.solution_report !== undefined) item.solution_report = fields.solution_report;
    item.updated_at = occurredAt;
    await putSnapshotItem('interventions', interventionId, item, customIDB);
  }

  await enqueueOperation({
    type: 'patch_intervention',
    url: `/api/interventions/${interventionId}`,
    method: 'PATCH',
    body: fields,
    depends_on: interventionId,
    user_id: currentUserId,
    site_id: currentSiteId,
    occurred_at: occurredAt,
    if_unmodified_since: ifUnmodifiedSince
  }, customIDB);
}

/**
 * Clôture d'une intervention hors ligne :
 * - Statut passe à 'completed', completed_at renseigné
 * - Enfile POST /api/interventions/:id/complete
 */
async function localCompleteIntervention(interventionId, params, currentUserId, currentSiteId, customIDB) {
  const item = await getSnapshotItem('interventions', interventionId, customIDB);
  const occurredAt = new Date().toISOString();

  if (item) {
    item.status = 'completed';
    item.completed_at = occurredAt;
    if (params && params.solution_report) item.solution_report = params.solution_report;
    if (item.server_updated_at === undefined) item.server_updated_at = item.updated_at || null;
    item.updated_at = occurredAt;
    await putSnapshotItem('interventions', interventionId, item, customIDB);
  }

  await enqueueOperation({
    type: 'complete_intervention',
    url: `/api/interventions/${interventionId}/complete`,
    method: 'POST',
    body: params || {},
    depends_on: interventionId,
    user_id: currentUserId,
    site_id: currentSiteId,
    occurred_at: occurredAt,
    if_unmodified_since: interventionId.startsWith('temp_') || !item ? null : item.server_updated_at
  }, customIDB);
}

// ----------------------------------------------------------------------------
// EXPORTS
// ----------------------------------------------------------------------------

if (typeof module !== 'undefined' && module.exports) {
  module.exports = {
    DB_NAME,
    DB_VERSION,
    generateUuid,
    openOfflineDB,
    setOfflineDBInstance,
    setMeta,
    getMeta,
    clearAllOfflineData,
    saveSnapshot,
    getSnapshotCollection,
    getSnapshotItem,
    getLocalIntervention,
    putSnapshotItem,
    enqueueOperation,
    getAllOperations,
    getPendingOperations,
    getConflictOperations,
    getUnsyncedOperations,
    checkCacheOwner,
    removeOperation,
    markOperationConflict,
    rewriteTemporaryId,
    syncOutbox,
    localCreateIntervention,
    localTakeIntervention,
    localRecordTime,
    localPatchIntervention,
    localCompleteIntervention
  };
}

if (typeof window !== 'undefined') {
  window.ApexOffline = {
    openOfflineDB,
    setMeta,
    getMeta,
    clearAllOfflineData,
    saveSnapshot,
    getSnapshotCollection,
    getSnapshotItem,
    getLocalIntervention,
    putSnapshotItem,
    enqueueOperation,
    getAllOperations,
    getPendingOperations,
    getConflictOperations,
    getUnsyncedOperations,
    checkCacheOwner,
    removeOperation,
    markOperationConflict,
    rewriteTemporaryId,
    syncOutbox,
    localCreateIntervention,
    localTakeIntervention,
    localRecordTime,
    localPatchIntervention,
    localCompleteIntervention
  };
}
