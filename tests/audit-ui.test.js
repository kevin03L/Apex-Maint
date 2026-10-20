'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const app = fs.readFileSync(require.resolve('../static/app.js'), 'utf8');

test('U6. Le résumé terrain partage une lecture et conserve les deux décomptes', async () => {
  let reads = 0;
  const summary = {};
  const context = vm.createContext({
    document: { getElementById: () => summary },
    ApexOffline: { getUnsyncedOperations: async () => {
      reads++;
      return [{ status: 'pending' }, { status: 'conflict' }, { status: 'pending' }];
    } }
  });
  vm.runInContext('async ' + fonction('initTerrainPageView', 'synchronisationPage'), context);
  await vm.runInContext('initTerrainPageView()', context);
  assert.equal(reads, 1);
  assert.equal(summary.textContent, '2 opération(s) en attente, 1 à résoudre.');
});

function fonction(name, next) {
  return app.slice(app.indexOf(`function ${name}(`), app.indexOf(`function ${next}(`));
}

test('A12. Le formulaire conserve le PATCH en ligne et utilise la file hors ligne', async () => {
  for (const online of [true, false]) {
    const calls = [];
    const context = vm.createContext({
      navigator: { onLine: online }, state: { currentUser: { id: 'tech' } },
      document: { getElementById: () => ({}) },
      ensureOfflineCacheOwner: async () => {}, hasPermission: () => true,
      api: async (...args) => calls.push(['api', ...args]),
      ApexOffline: { getLocalIntervention: async () => ({ site_id: 'site' }),
        localPatchIntervention: async (...args) => calls.push(['local', ...args]) },
      closeModal() {}, notify() {}, loadInterventionDetail: async () => {},
    });
    vm.runInContext('async ' + fonction('handleEditInterventionSubmit', 'interventionProgress').replace(/async\s*$/, ''), context);
    context.form = { dataset: { args: '"i"' }, title: { value: 'Titre' }, description: { value: 'Diagnostic' },
      priority: { value: 'high' }, machine_state: { value: 'ARRET' }, solution_report: { value: 'Réparé' } };
    await vm.runInContext('handleEditInterventionSubmit(form)', context);
    assert.equal(calls.length, 1);
    assert.equal(calls[0][0], online ? 'api' : 'local');
    assert.deepEqual(JSON.parse(JSON.stringify(online ? calls[0][2].body : calls[0][2])), online
      ? { title: 'Titre', description: 'Diagnostic', priority: 'high', machine_state: 'ARRET' }
      : { description: 'Diagnostic', solution_report: 'Réparé' });
  }
});

test('A13. Une autorisation différée après fermeture libère la caméra', async () => {
  let allow;
  let stopped = 0;
  let played = 0;
  const video = { isConnected: true, play: async () => played++ };
  const context = vm.createContext({
    navigator: { mediaDevices: { getUserMedia: () => new Promise(resolve => { allow = resolve; }) } },
    document: { getElementById: () => video },
    openModal() {}, closeModal() {}, notify() {}, startQrCodeDetectionLoop() {},
    openManualEquipmentCodeModal() {}, cancelAnimationFrame() {},
  });
  const start = app.indexOf('let scannerActiveStream = null;');
  const end = app.indexOf('function closeQrScannerModal()', start);
  vm.runInContext(app.slice(start, end), context);
  const pending = vm.runInContext('openQrScannerModal()', context);
  vm.runInContext('stopQrScanner()', context);
  allow({ getTracks: () => [{ stop() { stopped++; } }] });
  await pending;
  assert.equal(stopped, 1);
  assert.equal(played, 0);
});

test('U7. Les échéances conservent les pages de 25, leur ordre et leurs curseurs dans l’IHM', async () => {
  for (const due of ['overdue', 'soon']) {
    const expected = Array.from({ length: 57 }, (_, i) => ({ id: `plan-${i}`, state: due === 'soon' ? 'due_soon' : due }));
    const renders = [];
    let offset = 0;
    const state = {
      maintenancePlans: [], maintenancePlansNextCursor: null,
      selectedSiteId: 'a', maintenancePlansSearchQuery: ' Entretien ',
      maintenancePlansEquipmentFilter: 'eq', maintenancePlansFrequencyFilter: 'daily',
      maintenancePlansDueFilter: due, maintenancePlansActiveFilter: 'true'
    };
    const context = vm.createContext({
      state, URLSearchParams, document: { getElementById: () => ({}) }, icon: () => '',
      renderMaintenancePlansTable: () => {
        assert.equal(state.maintenancePlansLoading, false);
        renders.push(Array.from(state.maintenancePlans));
      },
      api: async path => {
        const params = new URL(path, 'http://localhost').searchParams;
        assert.equal(params.get('due'), due);
        assert.equal(params.get('limit'), '25');
        assert.equal(params.get('site_id'), 'a');
        assert.equal(params.get('q'), 'Entretien');
        assert.equal(params.get('equipment_id'), 'eq');
        assert.equal(params.get('frequency'), 'daily');
        assert.equal(params.get('active'), 'true');
        assert.equal(params.get('cursor'), offset ? `curseur-${offset}` : null);
        const items = expected.slice(offset, offset + 25);
        offset += items.length;
        return { items, next_cursor: offset < expected.length ? `curseur-${offset}` : null };
      }
    });
    vm.runInContext('async ' + fonction('loadMaintenancePlans', 'renderMaintenancePlansTable'), context);
    await vm.runInContext('loadMaintenancePlans(true)', context);
    while (state.maintenancePlansNextCursor) await vm.runInContext('loadMaintenancePlans(false)', context);
    assert.deepEqual(renders, [expected.slice(0, 25), expected.slice(0, 50), expected]);
    assert.deepEqual(Array.from(state.maintenancePlans), expected);
  }
});

test('B1. Le rechargement attend le nouveau contrôleur et le worker accepte le message', () => {
  let reloads = 0;
  let controllerchange;
  let message;
  const listeners = {};
  const context = vm.createContext({
    state: { _pendingServiceWorker: { postMessage(value) { message = value; } } },
    navigator: { serviceWorker: { addEventListener(event, callback, options) {
      assert.equal(event, 'controllerchange'); assert.equal(options.once, true); controllerchange = callback;
    } } }, window: { location: { reload() { reloads++; } } }
  });
  vm.runInContext(fonction('applyAppUpdate', 'goToBackupsSettings'), context);
  vm.runInContext('applyAppUpdate()', context);
  assert.equal(reloads, 0);
  let activations = 0;
  vm.runInNewContext(fs.readFileSync(require.resolve('../static/sw.js'), 'utf8'), {
    self: { addEventListener(event, callback) { listeners[event] = callback; }, skipWaiting() { activations++; } },
    URL, console
  });
  listeners.message({ data: message });
  assert.equal(activations, 1);
  controllerchange();
  assert.equal(reloads, 1);
});

test('M2. Les actions hors ligne utilisent les fonctions locales et refusent le temps d’un tiers', async () => {
  const calls = [];
  const context = vm.createContext({
    navigator: { onLine: false }, state: { currentUser: { id: 'tech' } },
    ensureOfflineCacheOwner: async () => {}, hasPermission: () => true,
    api: async () => { throw Error('Appel réseau inattendu'); },
    ApexOffline: {
      getLocalIntervention: async () => ({ site_id: 'site' }),
      localTakeIntervention: async (...args) => calls.push(['take', ...args]),
      localRecordTime: async (...args) => calls.push(['time', ...args]),
      localCompleteIntervention: async (...args) => calls.push(['complete', ...args])
    }
  });
  vm.runInContext('async ' + fonction('interventionProgress', 'takeIntervention').replace(/async\s*$/, ''), context);
  await vm.runInContext("interventionProgress('/api/interventions/i/take', {body:{}})", context);
  await vm.runInContext("interventionProgress('/api/interventions/i/technicians/tech/time', {body:{duration_minutes:45}})", context);
  await vm.runInContext("interventionProgress('/api/interventions/i/complete', {body:{solution_report:'Réparé'}})", context);
  await assert.rejects(vm.runInContext("interventionProgress('/api/interventions/i/technicians/autre/time', {body:{duration_minutes:45}})", context));
  assert.deepEqual(calls.map(c => c[0]), ['take', 'time', 'complete']);
});
