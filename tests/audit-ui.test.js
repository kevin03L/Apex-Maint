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
