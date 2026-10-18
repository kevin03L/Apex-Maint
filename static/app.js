/* ============================================================================
   Apex Maint — IHM web (sans framework ni étape de compilation)
   ----------------------------------------------------------------------------
   Gabarit de l'application : menu, routeur par ancre (#/…), état du serveur.
   Règles (voir AGENTS.md) :
   - toute donnée insérée dans le HTML passe par esc(), tout argument JavaScript
     placé dans un attribut par jsArg() ;
   - aucun jeton de session n'est lu ni stocké ici : la session est un cookie
     HttpOnly envoyé par le navigateur (fetch avec credentials 'same-origin') ;
   - aucun script ni style en ligne (politique CSP default-src 'self').
   ============================================================================ */
'use strict';

// --- Échappement --------------------------------------------------------------
const ESC_MAP = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;', '`': '&#96;' };

/** Échappe une valeur pour une insertion dans du HTML (texte ou attribut entre guillemets). */
function esc(v) {
  if (v === null || v === undefined) return '';
  return String(v).replace(/[&<>"'`]/g, c => ESC_MAP[c]);
}

/** Encode une valeur comme argument JavaScript dans un attribut HTML (data-args="${jsArg(x)}"). */
function jsArg(v) {
  return esc(JSON.stringify(v === undefined ? null : v));
}

/** Icône SVG de la bibliothèque déclarée dans index.html. */
function icon(id) {
  return `<svg class="i" aria-hidden="true"><use href="#${esc(id)}"/></svg>`;
}

// --- Accès à l'API ------------------------------------------------------------
/** Appel JSON à l'API. Lève une erreur portant le code HTTP en cas d'échec. */
async function api(path, options = {}) {
  const res = await fetch(path, {
    credentials: 'same-origin',
    headers: { Accept: 'application/json', ...(options.headers || {}) },
    ...options,
  });
  if (!res.ok) {
    const err = new Error(`HTTP ${res.status}`);
    err.status = res.status;
    throw err;
  }
  return res.json();
}

// --- Modules de l'application -------------------------------------------------
// `phase` : phase de la feuille de route (docs/PLAN.md §4) qui livre le module.
const MODULES = [
  { grp: 'Pilotage' },
  { id: 'tableau-de-bord', label: 'Tableau de bord', icon: 'i-dash', phase: 0 },
  { grp: 'Maintenance' },
  { id: 'interventions', label: 'Interventions', icon: 'i-wrench', phase: 3 },
  { id: 'maintenance-preventive', label: 'Maintenance préventive', icon: 'i-cal', phase: 5 },
  { grp: 'Référentiel' },
  { id: 'equipements', label: 'Équipements', icon: 'i-gear', phase: 2 },
  { id: 'lignes-de-production', label: 'Lignes de production', icon: 'i-line', phase: 2 },
  { id: 'emplacements', label: 'Emplacements', icon: 'i-pin', phase: 2 },
  { id: 'fournisseurs', label: 'Fournisseurs', icon: 'i-truck', phase: 2 },
  { grp: 'Magasin' },
  { id: 'stock', label: 'Pièces & stock', icon: 'i-box', phase: 4 },
  { id: 'documents', label: 'Documents', icon: 'i-doc', phase: 6 },
  { grp: 'Analyse' },
  { id: 'reporting', label: 'Reporting', icon: 'i-chart', phase: 7 },
  { id: 'audit', label: "Journal d'audit", icon: 'i-audit', phase: 7 },
  { grp: 'Administration' },
  { id: 'parametres', label: 'Paramètres', icon: 'i-cog', phase: 7 },
  { id: 'utilisateurs', label: 'Utilisateurs', icon: 'i-users', phase: 1 },
];
const DEFAULT_ROUTE = 'tableau-de-bord';

/** Phase de développement actuelle : les modules des phases suivantes sont annoncés. */
const CURRENT_PHASE = 0;

// --- État du serveur ------------------------------------------------------------
const state = {
  health: null,
  online: false,
  checkedAt: null,
};

// --- Menu ---------------------------------------------------------------------
const NAV_FOLD_KEY = 'apex_maint_nav_folded';
const MOBILE_QUERY = window.matchMedia('(max-width: 767px)');

function renderNav() {
  const nav = document.getElementById('mainNav');
  nav.innerHTML = MODULES.map(m => {
    if (m.grp) return `<div class="grp">${esc(m.grp)}</div>`;
    const badge = m.phase > CURRENT_PHASE ? `<span class="n" title="Prévu en phase ${esc(m.phase)}">P${esc(m.phase)}</span>` : '';
    return `<a href="#/${esc(m.id)}" data-route="${esc(m.id)}" title="${esc(m.label)}">${icon(m.icon)}<span class="lab">${esc(m.label)}</span>${badge}</a>`;
  }).join('');
}

/** Restaure l'état replié du menu (bureau uniquement). */
function restoreNavFold() {
  try {
    if (localStorage.getItem(NAV_FOLD_KEY) === '1') document.body.classList.add('folded');
  } catch (_) { /* stockage indisponible : menu déplié */ }
}

function toggleNav() {
  if (MOBILE_QUERY.matches) {
    setMobileNav(!document.body.classList.contains('nav-open'));
    return;
  }
  const folded = document.body.classList.toggle('folded');
  try {
    localStorage.setItem(NAV_FOLD_KEY, folded ? '1' : '0');
  } catch (_) { /* préférence non conservée */ }
}

function setMobileNav(open) {
  document.body.classList.toggle('nav-open', open);
  document.getElementById('navBackdrop').hidden = !open;
}

// --- Routeur ------------------------------------------------------------------
function currentRoute() {
  const id = location.hash.replace(/^#\/?/, '');
  return MODULES.some(m => m.id === id) ? id : DEFAULT_ROUTE;
}

function render() {
  const route = currentRoute();
  const mod = MODULES.find(m => m.id === route);
  document.querySelectorAll('#mainNav a[data-route]').forEach(a => {
    const on = a.dataset.route === route;
    a.classList.toggle('on', on);
    if (on) a.setAttribute('aria-current', 'page');
    else a.removeAttribute('aria-current');
  });
  document.title = `${mod.label} — Apex Maint`;
  const view = document.getElementById('view');
  view.innerHTML = mod.phase > CURRENT_PHASE ? plannedPage(mod) : PAGES[mod.id]();
  if (MOBILE_QUERY.matches) setMobileNav(false);
}

function plannedPage(mod) {
  return `<section class="planned">
    ${icon(mod.icon)}
    <h1>${esc(mod.label)}</h1>
    <p>Module prévu en phase ${esc(mod.phase)} de la feuille de route.</p>
  </section>`;
}

const PAGES = {
  'tableau-de-bord': dashboardPage,
};

// --- Tableau de bord ----------------------------------------------------------
function dashboardPage() {
  const h = state.health;
  const status = state.online
    ? `<span class="state-ok">En ligne</span>`
    : `<span class="state-ko">Injoignable</span>`;
  return `<div class="page-head">${icon('i-dash')}<h1>Tableau de bord</h1></div>
    <div class="cards">
      <section class="card">
        <h2>${icon('i-server')} Serveur</h2>
        <dl>
          <dt>État</dt><dd>${status}</dd>
          <dt>Produit</dt><dd>${esc(h ? h.product : '—')}</dd>
          <dt>Version</dt><dd class="mono">${esc(h ? h.version : '—')}</dd>
          <dt>Vérifié à</dt><dd>${esc(state.checkedAt ? formatTime(state.checkedAt) : '—')}</dd>
        </dl>
      </section>
      <section class="card">
        <h2>${icon('i-wrench')} Interventions</h2>
        <p class="hint">Indicateurs disponibles à partir de la phase 3.</p>
      </section>
      <section class="card">
        <h2>${icon('i-box')} Stock</h2>
        <p class="hint">Alertes de stock bas disponibles à partir de la phase 4.</p>
      </section>
    </div>`;
}

// --- Surveillance du serveur --------------------------------------------------
const HEALTH_PERIOD_MS = 30000;

async function checkHealth() {
  try {
    state.health = await api('/api/health');
    state.online = true;
  } catch (_) {
    state.online = false;
  }
  state.checkedAt = new Date();
  renderServerState();
  if (currentRoute() === 'tableau-de-bord') render();
}

function renderServerState() {
  const pill = document.getElementById('serverPill');
  pill.classList.toggle('offline', !state.online);
  pill.querySelector('.lab').textContent = state.online ? 'En ligne' : 'Serveur injoignable';
  if (state.health) {
    document.getElementById('statusProduct').textContent = state.health.product || 'Apex Maint';
    document.getElementById('statusVersion').textContent = state.health.version || '';
  }
  document.getElementById('statusCheck').textContent =
    `Dernière vérification : ${formatTime(state.checkedAt)}`;
}

// --- Thème --------------------------------------------------------------------
// applyTheme(), currentTheme() et toggleTheme() viennent de theme.js.

/** Le bouton montre le thème vers lequel il bascule. */
function renderThemeToggle() {
  const btn = document.getElementById('themeToggle');
  const light = currentTheme() === 'light';
  btn.innerHTML = icon(light ? 'i-moon' : 'i-sun');
  btn.setAttribute('aria-label', light ? 'Passer au thème sombre' : 'Passer au thème clair');
  btn.title = btn.getAttribute('aria-label');
}

// --- Horloge ------------------------------------------------------------------
function formatTime(d) {
  return d.toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

function tickClock() {
  document.getElementById('clock').textContent = new Date().toLocaleString('fr-FR', {
    weekday: 'short', day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit',
  });
}

// --- Démarrage ----------------------------------------------------------------
function init() {
  renderNav();
  restoreNavFold();
  document.getElementById('navToggle').addEventListener('click', toggleNav);
  document.getElementById('navBackdrop').addEventListener('click', () => setMobileNav(false));
  renderThemeToggle();
  document.getElementById('themeToggle').addEventListener('click', () => {
    toggleTheme();
    renderThemeToggle();
  });
  // Le thème peut aussi changer avec celui du système (theme.js) : on resynchronise l'icône.
  new MutationObserver(renderThemeToggle).observe(document.documentElement, { attributeFilter: ['data-theme'] });
  MOBILE_QUERY.addEventListener('change', () => setMobileNav(false));
  window.addEventListener('hashchange', render);
  render();
  tickClock();
  setInterval(tickClock, 15000);
  checkHealth();
  setInterval(checkHealth, HEALTH_PERIOD_MS);
}

document.addEventListener('DOMContentLoaded', init);
