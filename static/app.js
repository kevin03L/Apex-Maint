/* ============================================================================
   Apex Maint — IHM web (sans framework ni étape de compilation)
   ----------------------------------------------------------------------------
   Phase 1 : Authentification, sites, utilisateurs et matrice de permissions.
   Règles impératives (voir AGENTS.md) :
   - toute donnée dynamique insérée dans le HTML passe par esc() ;
   - tout argument d'action dans un attribut passe par data-args="${jsArg(...)}" ;
   - aucun jeton de session n'est lu ni stocké en JavaScript : la session est un
     cookie HttpOnly envoyé automatiquement par fetch (credentials: 'same-origin') ;
   - aucun script ni style en ligne (politique CSP default-src 'self') ;
   - aucune directive onclick / onchange / onsubmit : délégation d'événements sur
     data-action ;
   - textes et messages en français, icônes vectorielles SVG (aucun émoji) ;
   - responsive jusqu'à 375 px de large.
   ============================================================================ */
'use strict';

// ============================================================================
// 1. ÉCHAPPEMENT ET UTILITAIRES
// ============================================================================

const ESC_MAP = {
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;',
  '"': '&quot;',
  "'": '&#39;',
  '`': '&#96;',
};

/** Échappe une valeur pour une insertion sûre dans du HTML. */
function esc(v) {
  if (v === null || v === undefined) return '';
  return String(v).replace(/[&<>"'`]/g, c => ESC_MAP[c]);
}

/** Encode une valeur en JSON sûr pour un attribut HTML (data-args="${jsArg(x)}"). */
function jsArg(v) {
  return esc(JSON.stringify(v === undefined ? null : v));
}

/** Icône SVG de la bibliothèque vectorielle déclarée dans index.html. */
function icon(id) {
  return `<svg class="i" aria-hidden="true"><use href="#${esc(id)}"/></svg>`;
}

/** Formate une date RFC 3339 / ISO en date lisible en français. */
function formatDate(d) {
  if (!d) return '—';
  try {
    const dt = new Date(d);
    if (isNaN(dt.getTime())) return String(d);
    return dt.toLocaleString('fr-FR', {
      day: '2-digit',
      month: '2-digit',
      year: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  } catch (_) {
    return String(d);
  }
}

/** Formate une heure uniquement. */
function formatTime(d) {
  if (!d) return '—';
  try {
    const dt = new Date(d);
    return dt.toLocaleTimeString('fr-FR', {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
  } catch (_) {
    return '—';
  }
}

/** Libellé convivial d'un rôle. */
function formatRole(role) {
  switch (role) {
    case 'admin':
      return 'Administrateur';
    case 'responsable':
      return 'Responsable';
    case 'technicien':
      return 'Technicien';
    case 'production':
      return 'Production';
    default:
      return role || '—';
  }
}

/** Classe CSS pour le badge de rôle. */
function roleBadgeClass(role) {
  switch (role) {
    case 'admin':
      return 'badge badge-role-admin';
    case 'responsable':
      return 'badge badge-role-responsable';
    case 'technicien':
      return 'badge badge-role-technicien';
    case 'production':
      return 'badge badge-role-production';
    default:
      return 'badge';
  }
}

/** Gestionnaire de délai de saisie (debounce) par clé unique. */
const debounceTimers = new Map();
function debounce(key, fn, delayMs = 300) {
  if (debounceTimers.has(key)) {
    clearTimeout(debounceTimers.get(key));
  }
  debounceTimers.set(key, setTimeout(() => {
    debounceTimers.delete(key);
    fn();
  }, delayMs));
}

/** Badge coloré pour les statuts d'équipement (operational, degraded, out_of_service). */
function statusBadge(status) {
  switch (status) {
    case 'operational':
      return `<span class="badge badge-state-ok">En service</span>`;
    case 'degraded':
      return `<span class="badge badge-state-warn">Dégradé</span>`;
    case 'out_of_service':
      return `<span class="badge badge-state-ko">Hors service</span>`;
    default:
      return `<span class="badge">${esc(status || '—')}</span>`;
  }
}

/** Formate une durée en minutes en chaîne lisible (ex: "1 h 30 min"). */
function formatDurationMinutes(minutes) {
  if (minutes === null || minutes === undefined || minutes === '') return '—';
  const num = parseInt(minutes, 10);
  if (isNaN(num)) return '—';
  if (num === 0) return '0 min';
  if (num < 60) return `${num} min`;
  const h = Math.floor(num / 60);
  const m = num % 60;
  return m === 0 ? `${h} h` : `${h} h ${m} min`;
}

/** Libellé convivial pour un statut d'intervention. */
function formatInterventionStatus(status) {
  switch (status) {
    case 'open':
      return 'Ouverte';
    case 'in_progress':
      return 'En cours';
    case 'completed':
      return 'Clôturée';
    case 'cancelled':
      return 'Annulée';
    default:
      return status || '—';
  }
}

/** Badge stylé pour un statut d'intervention. */
function interventionStatusBadge(status) {
  return `<span class="badge badge-status-${esc(status)}">${esc(formatInterventionStatus(status))}</span>`;
}

/** Libellé convivial pour une priorité d'intervention. */
function formatInterventionPriority(priority) {
  switch (priority) {
    case 'low':
      return 'Basse';
    case 'medium':
      return 'Moyenne';
    case 'high':
      return 'Haute';
    case 'critical':
      return 'Critique';
    default:
      return priority || '—';
  }
}

/** Badge stylé pour une priorité d'intervention. */
function interventionPriorityBadge(priority) {
  return `<span class="badge badge-priority-${esc(priority)}">${esc(formatInterventionPriority(priority))}</span>`;
}

/** Libellé convivial pour un état machine. */
function formatMachineState(state) {
  switch (state) {
    case 'ARRET':
      return 'Arrêt machine';
    case 'DEGRADE':
      return 'Dégradé';
    case 'FONCTIONNEL':
      return 'Fonctionnel';
    default:
      return state || '—';
  }
}

/** Badge stylé pour un état machine. */
function machineStateBadge(machineState) {
  const norm = (machineState || 'FONCTIONNEL').toUpperCase();
  const cls = norm === 'ARRET' ? 'badge-machine-arret' : (norm === 'DEGRADE' ? 'badge-machine-degrade' : 'badge-machine-fonctionnel');
  return `<span class="badge ${cls}">${esc(formatMachineState(norm))}</span>`;
}

/** Libellé convivial pour un type d'intervention. */
function formatInterventionType(type) {
  switch (type) {
    case 'corrective':
      return 'Corrective';
    case 'preventive':
      return 'Préventive';
    case 'a_posteriori':
      return 'A posteriori';
    case 'hors_equipement':
      return 'Hors équipement';
    default:
      return type || '—';
  }
}

/** Badge stylé pour un type d'intervention. */
function interventionTypeBadge(type) {
  return `<span class="badge badge-type">${esc(formatInterventionType(type))}</span>`;
}

/** Libellé lisible pour un type d'emplacement. */
function locationKindLabel(kind) {
  switch (kind) {
    case 'magasin':
      return 'Magasin';
    case 'allee':
      return 'Allée';
    case 'rack':
      return 'Rack';
    case 'bac':
      return 'Bac';
    default:
      return kind || '—';
  }
}

/** Badge stylé pour un type d'emplacement. */
function locationKindBadge(kind) {
  return `<span class="badge badge-kind badge-kind-${esc(kind)}">${esc(locationKindLabel(kind))}</span>`;
}

/** Déduit le type d'emplacement enfant à partir du type du parent. */
function nextLocationKind(parentKind) {
  switch (parentKind) {
    case 'magasin':
      return 'allee';
    case 'allee':
      return 'rack';
    case 'rack':
      return 'bac';
    default:
      return null;
  }
}

/**
 * Barre d'outils réutilisable pour listes et vues de référentiel.
 */
function renderToolbar(opts) {
  return `
    <div class="toolbar">
      <div class="toolbar-group">
        ${opts.searchAction ? `
          <div class="search-input-wrap">
            ${icon('i-search')}
            <input type="search" placeholder="${esc(opts.searchPlaceholder || 'Rechercher…')}" value="${esc(opts.searchValue || '')}" data-action="${esc(opts.searchAction)}" aria-label="${esc(opts.searchPlaceholder || 'Recherche')}" />
          </div>
        ` : ''}
        ${opts.extraFiltersHtml || ''}
      </div>
      <div class="toolbar-actions">
        ${opts.viewSwitcherHtml || ''}
        ${opts.canCreate ? `
          <button type="button" class="btn primary" data-action="${esc(opts.primaryAction)}">
            ${icon(opts.primaryIcon || 'i-plus')} <span>${esc(opts.primaryLabel)}</span>
          </button>
        ` : ''}
      </div>
    </div>
  `;
}

/**
 * Rendu du bloc de pagination « Charger plus » sur curseur opaque.
 */
function renderLoadMore(nextCursor, action, loading = false) {
  if (!nextCursor) return '';
  return `
    <div class="load-more-wrap">
      <button type="button" class="btn btn-load-more" data-action="${esc(action)}" data-args="${jsArg(nextCursor)}" ${loading ? 'disabled' : ''}>
        ${icon(loading ? 'i-clock' : 'i-refresh')}
        <span>${loading ? 'Chargement…' : 'Charger plus'}</span>
      </button>
    </div>
  `;
}

/**
 * Ouvre une boîte de dialogue modale standardisée avec gestion d'erreurs du serveur.
 */
function openFormModal({ title, formId, submitAction, submitArgs, submitLabel, fieldsHtml, errorBoxId }) {
  const errId = errorBoxId || `${formId}ErrorBox`;
  const errTextId = `${errId}Text`;

  const bodyHtml = `
    <form class="form-grid" id="${esc(formId)}" data-action="${esc(submitAction)}" ${submitArgs !== undefined ? `data-args="${jsArg(submitArgs)}"` : ''}>
      <div id="${esc(errId)}" class="form-error" hidden>
        ${icon('i-alert')}
        <span id="${esc(errTextId)}"></span>
      </div>
      ${fieldsHtml}
    </form>
  `;

  const footerHtml = `
    <button type="button" class="btn" data-action="close-modal">Annuler</button>
    <button type="submit" form="${esc(formId)}" class="btn primary" id="${esc(formId)}SubmitBtn">${esc(submitLabel)}</button>
  `;

  openModal(title, bodyHtml, footerHtml);
}

/**
 * Sélecteur de site pour les formulaires modaux (création par l'administrateur).
 */
function renderSiteFormField(selectedId = '') {
  if (!isAdmin()) {
    const siteId = state.site ? state.site.id : '';
    return siteId ? `<input type="hidden" name="site_id" value="${esc(siteId)}" />` : '';
  }
  return `
    <div class="form-group">
      <label for="fSiteId">Site de rattachement *</label>
      <select id="fSiteId" name="site_id" required data-action="on-modal-site-change">
        <option value="" ${!selectedId ? 'selected disabled' : ''}>-- Choisir un site --</option>
        ${state.sites.map(s => `<option value="${esc(s.id)}" ${s.id === selectedId ? 'selected' : ''}>${esc(s.name)}</option>`).join('')}
      </select>
    </div>
  `;
}

// ============================================================================
// 2. ACCÈS À L'API HTTP
// ============================================================================

/**
 * Appel JSON à l'API REST avec transport des cookies HttpOnly (same-origin).
 * Intercepte les erreurs 401 pour ramener à l'écran de connexion et les erreurs
 * 403 avec code password_change_required pour imposer le changement de mot de passe.
 */
async function api(path, options = {}) {
  const headers = { Accept: 'application/json', ...(options.headers || {}) };
  const fetchOpts = {
    credentials: 'same-origin',
    ...options,
    headers,
  };

  if (fetchOpts.body && typeof fetchOpts.body === 'object' && !(fetchOpts.body instanceof FormData)) {
    headers['Content-Type'] = 'application/json';
    fetchOpts.body = JSON.stringify(fetchOpts.body);
  }

  let res;
  try {
    res = await fetch(path, fetchOpts);
  } catch (_) {
    const err = new Error('Serveur injoignable ou erreur réseau.');
    err.status = 0;
    throw err;
  }

  // 401 : Session absente, expirée ou révoquée
  if (res.status === 401) {
    if (path !== '/api/auth/login') {
      handleSessionExpired();
    }
    const errBody = await parseJsonSafe(res);
    const err = new Error(errBody && errBody.message ? errBody.message : 'Identifiant ou mot de passe incorrect.');
    err.status = 401;
    err.code = errBody && errBody.code;
    throw err;
  }

  // 403 : Changement de mot de passe imposé ou accès refusé
  if (res.status === 403) {
    const errBody = await parseJsonSafe(res);
    if (errBody && errBody.code === 'password_change_required') {
      showPasswordChangeRequiredScreen();
    }
    const err = new Error(errBody && errBody.message ? errBody.message : 'Accès refusé.');
    err.status = 403;
    err.code = errBody && errBody.code;
    throw err;
  }

  // Réponses d'erreur diverses (400, 404, 409, 429...)
  if (!res.ok) {
    const errBody = await parseJsonSafe(res);
    const msg = errBody && errBody.message ? errBody.message : `Erreur HTTP ${res.status}`;
    const err = new Error(msg);
    err.status = res.status;
    err.code = errBody && errBody.code;
    err.data = errBody;
    throw err;
  }

  if (res.status === 204) return null;
  return parseJsonSafe(res);
}

/** Tente de lire le corps d'une réponse sous forme JSON sans lever d'exception. */
async function parseJsonSafe(res) {
  try {
    return await res.json();
  } catch (_) {
    return null;
  }
}

// ============================================================================
// 3. SYSTÈME DE NOTIFICATIONS (TOASTS)
// ============================================================================

/**
 * Affiche une notification toast non bloquante.
 * kind : 'info' | 'success' | 'error'
 */
function notify(message, kind = 'info') {
  const box = document.getElementById('toastBox');
  if (!box) return;

  const t = document.createElement('div');
  t.className = `toast toast-${kind}`;

  const iconId = kind === 'success' ? 'i-check' : kind === 'error' ? 'i-alert' : 'i-server';
  t.innerHTML = `${icon(iconId)}<span class="toast-msg">${esc(message)}</span>`;
  box.appendChild(t);

  setTimeout(() => {
    t.remove();
  }, 4000);
}

// ============================================================================
// 4. BOÎTES DE DIALOGUE MODALES ET CONFIRMATIONS
// ============================================================================

let currentConfirmResolve = null;

/**
 * Ouvre la modale générique de l'application.
 */
function openModal(title, bodyHtml, footerHtml = '') {
  const overlay = document.getElementById('modalOverlay');
  const titleEl = document.getElementById('modalTitle');
  const bodyEl = document.getElementById('modalBody');
  const footerEl = document.getElementById('modalFooter');

  titleEl.textContent = title;
  bodyEl.innerHTML = bodyHtml;
  footerEl.innerHTML = footerHtml;
  overlay.hidden = false;

  setTimeout(() => {
    const firstInput = bodyEl.querySelector('input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), button.primary');
    if (firstInput) firstInput.focus();
  }, 30);
}

/**
 * Ferme la modale générique de l'application.
 */
function closeModal() {
  const overlay = document.getElementById('modalOverlay');
  if (overlay) overlay.hidden = true;
  if (currentConfirmResolve) {
    currentConfirmResolve(false);
    currentConfirmResolve = null;
  }
}

/**
 * Affiche une boîte de dialogue de confirmation modale.
 * Renvoie une promesse résolue avec true ou false.
 */
function confirmDialog(title, message, confirmLabel = 'Confirmer', danger = false) {
  return new Promise(resolve => {
    currentConfirmResolve = resolve;
    const bodyHtml = `<p>${esc(message)}</p>`;
    const btnClass = danger ? 'btn primary btn-danger' : 'btn primary';
    const footerHtml = `
      <button type="button" class="btn" data-action="close-modal">Annuler</button>
      <button type="button" class="${btnClass}" data-action="confirm-dialog-ok">${esc(confirmLabel)}</button>
    `;
    openModal(title, bodyHtml, footerHtml);
  });
}

// ============================================================================
// 5. MODULES, PERMISSIONS ET RÔLES
// ============================================================================

/** Phase actuelle du projet (Phase 3 : Module Interventions). */
const CURRENT_PHASE = 3;

/** Liste de tous les modules de l'application avec leur permission de vue requise. */
const MODULES = [
  { grp: 'Pilotage' },
  { id: 'tableau-de-bord', label: 'Tableau de bord', icon: 'i-dash', phase: 0, perm: 'dashboard.view' },
  { grp: 'Maintenance' },
  { id: 'interventions', label: 'Interventions', icon: 'i-wrench', phase: 3, perm: 'interventions.view' },
  { id: 'maintenance-preventive', label: 'Maintenance préventive', icon: 'i-cal', phase: 5, perm: 'maintenance.view' },
  { grp: 'Référentiel' },
  { id: 'equipements', label: 'Équipements', icon: 'i-gear', phase: 2, perm: 'equipments.view' },
  { id: 'lignes-de-production', label: 'Lignes de production', icon: 'i-line', phase: 2, perm: 'production_lines.view' },
  { id: 'emplacements', label: 'Emplacements', icon: 'i-pin', phase: 2, perm: 'locations.view' },
  { id: 'fournisseurs', label: 'Fournisseurs', icon: 'i-truck', phase: 2, perm: 'suppliers.view' },
  { grp: 'Magasin' },
  { id: 'stock', label: 'Pièces & stock', icon: 'i-box', phase: 4, perm: 'stock.view' },
  { id: 'documents', label: 'Documents', icon: 'i-doc', phase: 6, perm: 'documents.view' },
  { grp: 'Analyse' },
  { id: 'reporting', label: 'Reporting', icon: 'i-chart', phase: 7, perm: 'reporting.view' },
  { id: 'audit', label: "Journal d'audit", icon: 'i-audit', phase: 7, perm: 'audit.view' },
  { grp: 'Administration' },
  { id: 'sites', label: 'Sites', icon: 'i-site', phase: 1, adminOnly: true },
  { id: 'utilisateurs', label: 'Utilisateurs', icon: 'i-users', phase: 1, perm: 'users.view' },
  { id: 'parametres', label: 'Paramètres', icon: 'i-cog', phase: 7, perm: 'settings.view' },
];

const DEFAULT_ROUTE = 'tableau-de-bord';

/** Domaines de permissions pour la matrice RBAC (34 permissions complètes de Permission::ALL). */
const PERMISSION_DOMAINS = [
  {
    name: 'Pilotage & Analyse',
    permissions: [
      { key: 'dashboard.view', label: 'Consulter le tableau de bord' },
      { key: 'reporting.view', label: 'Consulter les rapports et statistiques' },
      { key: 'audit.view', label: "Consulter le journal d'audit" },
    ],
  },
  {
    name: 'Maintenance & Interventions',
    permissions: [
      { key: 'interventions.view', label: 'Consulter les interventions' },
      { key: 'interventions.create', label: 'Créer une intervention' },
      { key: 'interventions.edit', label: 'Modifier une intervention' },
      { key: 'interventions.delete', label: 'Supprimer une intervention' },
      { key: 'maintenance.view', label: 'Consulter la maintenance préventive' },
      { key: 'maintenance.manage', label: 'Gérer la maintenance préventive' },
    ],
  },
  {
    name: 'Équipements & Référentiel',
    permissions: [
      { key: 'equipments.view', label: 'Consulter les équipements' },
      { key: 'equipments.create', label: 'Créer un équipement' },
      { key: 'equipments.edit', label: 'Modifier un équipement' },
      { key: 'equipments.delete', label: 'Supprimer un équipement' },
      { key: 'production_lines.view', label: 'Consulter les lignes de production' },
      { key: 'production_lines.create', label: 'Créer une ligne de production' },
      { key: 'production_lines.edit', label: 'Modifier une ligne de production' },
      { key: 'production_lines.delete', label: 'Supprimer une ligne de production' },
      { key: 'locations.view', label: 'Consulter les emplacements' },
      { key: 'locations.manage', label: 'Gérer les emplacements' },
    ],
  },
  {
    name: 'Magasin & Pièces',
    permissions: [
      { key: 'parts.view', label: 'Consulter les pièces' },
      { key: 'parts.create', label: 'Créer une pièce' },
      { key: 'parts.edit', label: 'Modifier une pièce' },
      { key: 'parts.delete', label: 'Supprimer une pièce' },
      { key: 'stock.view', label: 'Consulter le stock' },
      { key: 'stock.movements', label: 'Saisir des mouvements de stock' },
      { key: 'suppliers.view', label: 'Consulter les fournisseurs' },
      { key: 'suppliers.manage', label: 'Gérer les fournisseurs' },
    ],
  },
  {
    name: 'Documents',
    permissions: [
      { key: 'documents.view', label: 'Consulter les documents' },
      { key: 'documents.create', label: 'Ajouter des documents' },
      { key: 'documents.delete', label: 'Supprimer des documents' },
    ],
  },
  {
    name: 'Administration',
    permissions: [
      { key: 'users.view', label: 'Consulter les utilisateurs' },
      { key: 'users.manage', label: 'Gérer les utilisateurs et réinitialiser les mots de passe' },
      { key: 'settings.view', label: 'Consulter les paramètres système' },
      { key: 'settings.edit', label: 'Modifier les paramètres système' },
    ],
  },
];

// ============================================================================
// 6. ÉTAT GLOBAL DE L'APPLICATION
// ============================================================================

const state = {
  // Session utilisateur
  currentUser: null,
  permissions: new Set(),
  site: null,
  mustChangePassword: false,

  // Référentiels et caches
  sites: [],
  selectedSiteId: '', // '' = Tous les sites (admin uniquement)
  usersList: [],
  permissionsMatrix: null,

  // Lignes de production (Phase 2)
  productionLines: [],
  productionLinesNextCursor: null,
  productionLinesLoading: false,
  productionLinesSearchQuery: '',

  // Équipements (Phase 2)
  equipments: [],
  equipmentsNextCursor: null,
  equipmentsLoading: false,
  equipmentsSearchQuery: '',
  equipmentsStatusFilter: '',
  equipmentsLineFilter: '',
  equipmentsViewMode: 'list', // 'list' | 'tree'
  equipmentsTree: [],
  equipmentsTreeLoading: false,
  equipmentsTreeExpanded: new Set(),

  // Emplacements (Phase 2)
  locationsTree: [],
  locationsTreeLoading: false,
  locationsTreeExpanded: new Set(),

  // Fournisseurs (Phase 2)
  suppliers: [],
  suppliersNextCursor: null,
  suppliersLoading: false,
  suppliersSearchQuery: '',

  // Interventions (Phase 3)
  interventions: [],
  interventionsNextCursor: null,
  interventionsLoading: false,
  interventionsSearchQuery: '',
  interventionsStatusFilter: '',
  interventionsTypeFilter: '',
  interventionsPriorityFilter: '',
  interventionsEquipmentFilter: '',
  interventionsQuickTab: 'all', // 'all' | 'active' | 'mine' | 'arret' | 'critical'
  currentInterventionDetail: null,
  currentInterventionLoading: false,

  // Vue Utilisateurs
  usersTab: 'users', // 'users' | 'permissions'
  usersSearchQuery: '',
  usersRoleFilter: '',
  usersSiteFilter: '',

  // État du serveur
  health: null,
  online: false,
  checkedAt: null,
};

/** L'utilisateur courant possède-t-il une permission donnée ? */
function hasPermission(permKey) {
  if (!state.currentUser) return false;
  if (state.currentUser.role === 'admin') return true;
  return state.permissions.has(permKey);
}

/** L'utilisateur courant est-il administrateur ? */
function isAdmin() {
  return state.currentUser && state.currentUser.role === 'admin';
}

/** L'utilisateur courant est-il responsable de maintenance ? */
function isResponsable() {
  return state.currentUser && state.currentUser.role === 'responsable';
}

/** L'utilisateur courant a-t-il le droit de gérer les utilisateurs ? */
function canManageUsers() {
  return hasPermission('users.manage');
}

/** Un module est-il autorisé pour l'utilisateur courant ? */
function isModuleAllowed(mod) {
  if (!state.currentUser) return false;
  if (mod.adminOnly && !isAdmin()) return false;
  if (mod.perm && !hasPermission(mod.perm)) return false;
  return true;
}

// ============================================================================
// 7. GESTION DE L'AUTHENTIFICATION ET SESSIONS
// ============================================================================

/**
 * Vérifie l'état de la session au démarrage via GET /api/auth/me.
 */
async function checkAuth() {
  try {
    const me = await api('/api/auth/me');
    if (!me || !me.user) {
      showLoginScreen();
      return false;
    }

    state.currentUser = me.user;
    state.permissions = new Set(me.permissions || []);
    state.site = me.site || null;
    state.mustChangePassword = Boolean(me.must_change_password);

    if (state.mustChangePassword) {
      showPasswordChangeRequiredScreen();
      return false;
    }

    hideAuthOverlay();
    renderHeaderUser();
    await loadSites();
    renderNav();
    await render();
    return true;
  } catch (err) {
    if (err.status === 401) {
      showLoginScreen();
    } else {
      showLoginScreen('Serveur indisponible : vérifiez que le service Apex Maint est démarré.');
    }
    return false;
  }
}

/**
 * Affiche l'écran de connexion plein écran.
 */
function showLoginScreen(errorMessage = '') {
  const overlay = document.getElementById('authOverlay');
  const card = document.getElementById('authContent');

  card.innerHTML = `
    <div class="auth-header">
      <div class="logo">M</div>
      <h1>Apex Maint</h1>
      <p>Gestion de Maintenance Assistée par Ordinateur</p>
    </div>
    <form class="auth-form" data-action="login-submit">
      <div id="authErrorBox" class="auth-error" ${errorMessage ? '' : 'hidden'}>
        ${icon('i-alert')}
        <span id="authErrorText">${esc(errorMessage)}</span>
      </div>
      <div class="form-group">
        <label for="loginUsername">Identifiant</label>
        <input type="text" id="loginUsername" name="username" required autocomplete="username" autofocus />
      </div>
      <div class="form-group">
        <label for="loginPassword">Mot de passe</label>
        <input type="password" id="loginPassword" name="password" required autocomplete="current-password" />
      </div>
      <button type="submit" id="loginSubmitBtn" class="btn primary">
        <span>Se connecter</span>
      </button>
    </form>
  `;

  overlay.hidden = false;
  setTimeout(() => {
    const u = document.getElementById('loginUsername');
    if (u) u.focus();
  }, 40);
}

/**
 * Affiche l'écran dédié de changement de mot de passe imposé (must_change_password).
 */
function showPasswordChangeRequiredScreen(errorMessage = '') {
  const overlay = document.getElementById('authOverlay');
  const card = document.getElementById('authContent');

  card.innerHTML = `
    <div class="auth-header">
      <div class="logo">${icon('i-lock')}</div>
      <h1>Changement de mot de passe obligatoire</h1>
      <p>Votre mot de passe actuel doit être modifié pour sécuriser l'accès à votre compte.</p>
    </div>
    <div class="auth-rules">
      <div class="auth-rules-title">${icon('i-shield')} Règles de sécurité du mot de passe :</div>
      <ul>
        <li>Au moins 8 caractères ;</li>
        <li>Différent de votre mot de passe actuel ;</li>
        <li>Différent du mot de passe d'usine (« admin123 »).</li>
      </ul>
    </div>
    <form class="auth-form" data-action="force-password-submit">
      <div id="authErrorBox" class="auth-error" ${errorMessage ? '' : 'hidden'}>
        ${icon('i-alert')}
        <span id="authErrorText">${esc(errorMessage)}</span>
      </div>
      <div class="form-group">
        <label for="pwdForceCurrent">Mot de passe actuel</label>
        <input type="password" id="pwdForceCurrent" name="current_password" required autocomplete="current-password" autofocus />
      </div>
      <div class="form-group">
        <label for="pwdForceNew">Nouveau mot de passe</label>
        <input type="password" id="pwdForceNew" name="new_password" required minlength="8" autocomplete="new-password" />
      </div>
      <div class="form-group">
        <label for="pwdForceConfirm">Confirmer le nouveau mot de passe</label>
        <input type="password" id="pwdForceConfirm" name="confirm_password" required minlength="8" autocomplete="new-password" />
      </div>
      <button type="submit" id="forcePwdBtn" class="btn primary">
        <span>Enregistrer le nouveau mot de passe</span>
      </button>
    </form>
  `;

  overlay.hidden = false;
  setTimeout(() => {
    const p = document.getElementById('pwdForceCurrent');
    if (p) p.focus();
  }, 40);
}

/** Masque la surcouche d'authentification plein écran. */
function hideAuthOverlay() {
  const overlay = document.getElementById('authOverlay');
  if (overlay) overlay.hidden = true;
}

/** Gère l'expiration ou la révocation de la session courante. */
function handleSessionExpired() {
  state.currentUser = null;
  state.permissions.clear();
  state.sites = [];
  closeModal();
  showLoginScreen('Votre session a expiré ou a été révoquée : veuillez vous reconnecter.');
}

/**
 * Soumission de la connexion utilisateur.
 */
async function handleLoginSubmit(form) {
  const submitBtn = document.getElementById('loginSubmitBtn');
  const errorBox = document.getElementById('authErrorBox');
  const errorText = document.getElementById('authErrorText');

  const username = (form.username && form.username.value || '').trim();
  const password = (form.password && form.password.value || '');

  if (!username || !password) return;

  submitBtn.disabled = true;
  submitBtn.innerHTML = `${icon('i-clock')} <span>Connexion…</span>`;
  errorBox.hidden = true;

  try {
    await api('/api/auth/login', {
      method: 'POST',
      body: { username, password },
    });
    // Récupération du profil complet après pose du cookie
    await checkAuth();
  } catch (err) {
    submitBtn.disabled = false;
    submitBtn.innerHTML = `<span>Se connecter</span>`;
    errorBox.hidden = false;
    errorText.textContent = err.message || 'Identifiant ou mot de passe incorrect.';
  }
}

/**
 * Soumission du changement de mot de passe imposé.
 */
async function handleForcePasswordSubmit(form) {
  const btn = document.getElementById('forcePwdBtn');
  const errorBox = document.getElementById('authErrorBox');
  const errorText = document.getElementById('authErrorText');

  const current_password = form.current_password.value;
  const new_password = form.new_password.value;
  const confirm_password = form.confirm_password.value;

  if (new_password.length < 8) {
    errorBox.hidden = false;
    errorText.textContent = 'Le nouveau mot de passe doit comporter au moins 8 caractères.';
    return;
  }
  if (new_password === current_password) {
    errorBox.hidden = false;
    errorText.textContent = 'Le nouveau mot de passe doit être différent de votre mot de passe actuel.';
    return;
  }
  if (new_password === 'admin123') {
    errorBox.hidden = false;
    errorText.textContent = 'Le mot de passe d\'usine « admin123 » ne peut pas être réutilisé.';
    return;
  }
  if (new_password !== confirm_password) {
    errorBox.hidden = false;
    errorText.textContent = 'La confirmation ne correspond pas au nouveau mot de passe.';
    return;
  }

  btn.disabled = true;
  btn.innerHTML = `${icon('i-clock')} <span>Enregistrement…</span>`;
  errorBox.hidden = true;

  try {
    await api('/api/auth/password', {
      method: 'POST',
      body: { current_password, new_password },
    });
    notify('Mot de passe mis à jour avec succès.', 'success');
    await checkAuth();
  } catch (err) {
    btn.disabled = false;
    btn.innerHTML = `<span>Enregistrer le nouveau mot de passe</span>`;
    errorBox.hidden = false;
    errorText.textContent = err.message || 'Impossible de mettre à jour le mot de passe.';
  }
}

/**
 * Déconnexion de l'utilisateur courant.
 */
async function handleLogout() {
  try {
    await api('/api/auth/logout', { method: 'POST' });
  } catch (_) {
    // Si l'appel échoue, on continue la déconnexion locale
  }
  state.currentUser = null;
  state.permissions.clear();
  state.sites = [];
  closeUserDropdown();
  showLoginScreen('Vous êtes déconnecté.');
}

// ============================================================================
// 8. EN-TÊTE SUPÉRIEUR ET SÉLECTION DU SITE
// ============================================================================

/** Met à jour les informations du bouton utilisateur dans l'en-tête. */
function renderHeaderUser() {
  const user = state.currentUser;
  const userBtn = document.getElementById('userBtn');
  const userBtnLabel = document.getElementById('userBtnLabel');
  const userBtnRole = document.getElementById('userBtnRole');
  const dropdownName = document.getElementById('userDropdownName');
  const dropdownSub = document.getElementById('userDropdownSub');

  if (!user) {
    userBtn.disabled = true;
    userBtnLabel.textContent = 'Non connecté';
    userBtnRole.textContent = '';
    return;
  }

  userBtn.disabled = false;
  userBtnLabel.textContent = user.display_name || user.username;
  userBtnRole.textContent = formatRole(user.role);
  userBtnRole.className = `user-role-badge ${roleBadgeClass(user.role)}`;

  if (dropdownName) dropdownName.textContent = user.display_name || user.username;
  if (dropdownSub) {
    const siteText = state.site ? state.site.name : (isAdmin() ? 'Tous les sites (global)' : 'Aucun site');
    dropdownSub.textContent = `${user.username} • ${formatRole(user.role)} • ${siteText}`;
  }
}

/** Charge la liste des sites accessibles et met à jour le sélecteur d'en-tête. */
async function loadSites() {
  try {
    const sites = await api('/api/sites');
    state.sites = Array.isArray(sites) ? sites : [];
  } catch (_) {
    state.sites = [];
  }
  renderSiteSelect();
}

/** Met à jour l'élément <select id="siteSelect"> de l'en-tête. */
function renderSiteSelect() {
  const sel = document.getElementById('siteSelect');
  if (!sel) return;

  if (isAdmin()) {
    sel.disabled = false;
    let html = `<option value="">Tous les sites</option>`;
    html += state.sites.map(s => `
      <option value="${esc(s.id)}" ${s.id === state.selectedSiteId ? 'selected' : ''}>${esc(s.name)}</option>
    `).join('');
    sel.innerHTML = html;
  } else {
    sel.disabled = true;
    if (state.site) {
      sel.innerHTML = `<option value="${esc(state.site.id)}">${esc(state.site.name)}</option>`;
    } else {
      sel.innerHTML = `<option value="">Site principal</option>`;
    }
  }
}

/** Bascule l'affichage du menu déroulant utilisateur. */
function toggleUserDropdown(open) {
  const dd = document.getElementById('userDropdown');
  const btn = document.getElementById('userBtn');
  if (!dd || !btn) return;

  const willOpen = typeof open === 'boolean' ? open : dd.hidden;
  dd.hidden = !willOpen;
  btn.setAttribute('aria-expanded', willOpen ? 'true' : 'false');
}

/** Ferme le menu déroulant utilisateur. */
function closeUserDropdown() {
  toggleUserDropdown(false);
}

// ============================================================================
// 9. MENU DE NAVIGATION LATÉRAL
// ============================================================================

const NAV_FOLD_KEY = 'apex_maint_nav_folded';
const MOBILE_QUERY = window.matchMedia('(max-width: 767px)');

/** Génère le menu de navigation gauche filtré selon les permissions. */
function renderNav() {
  const nav = document.getElementById('mainNav');
  if (!nav) return;

  // Filtrage des modules autorisés
  const allowed = MODULES.filter(m => m.grp || isModuleAllowed(m));

  // Suppression des groupes vides
  const items = [];
  for (let i = 0; i < allowed.length; i++) {
    const item = allowed[i];
    if (item.grp) {
      const nextItem = allowed[i + 1];
      if (nextItem && !nextItem.grp) {
        items.push(item);
      }
    } else {
      items.push(item);
    }
  }

  nav.innerHTML = items.map(m => {
    if (m.grp) return `<div class="grp">${esc(m.grp)}</div>`;
    const badge = m.phase > CURRENT_PHASE ? `<span class="n" title="Prévu en phase ${esc(m.phase)}">P${esc(m.phase)}</span>` : '';
    return `<a href="#/${esc(m.id)}" data-route="${esc(m.id)}" title="${esc(m.label)}">${icon(m.icon)}<span class="lab">${esc(m.label)}</span>${badge}</a>`;
  }).join('');
}

/** Restaure l'état replié du menu depuis le stockage local (desktop uniquement). */
function restoreNavFold() {
  try {
    if (localStorage.getItem(NAV_FOLD_KEY) === '1') document.body.classList.add('folded');
  } catch (_) { /* stockage indisponible */ }
}

function toggleNav() {
  if (MOBILE_QUERY.matches) {
    setMobileNav(!document.body.classList.contains('nav-open'));
    return;
  }
  const folded = document.body.classList.toggle('folded');
  try {
    localStorage.setItem(NAV_FOLD_KEY, folded ? '1' : '0');
  } catch (_) { /* préférence non enregistrée */ }
}

function setMobileNav(open) {
  document.body.classList.toggle('nav-open', open);
  const backdrop = document.getElementById('navBackdrop');
  if (backdrop) backdrop.hidden = !open;
}

// ============================================================================
// 10. ROUTEUR ET PAGES DE L'APPLICATION
// ============================================================================

/** Informations de la route courante d'après le fragment d'URL. */
function currentRouteInfo() {
  const raw = (location.hash || '').replace(/^#\/?/, '').trim();
  const path = raw.split(/[?#]/)[0].replace(/\/+$/, '');
  if (path === 'profil') return { route: 'profil', id: null };
  const parts = path.split('/');
  const root = parts[0];
  const id = parts.length > 1 ? parts.slice(1).join('/') : null;
  const mod = MODULES.find(m => m.id === root);
  return {
    route: mod ? mod.id : DEFAULT_ROUTE,
    id: id || null,
  };
}

/** Identifiant de la route courante d'après le fragment d'URL. */
function currentRoute() {
  return currentRouteInfo().route;
}

/** Rendu de la vue correspondant à la route active. */
async function render() {
  if (!state.currentUser) return;

  const { route, id } = currentRouteInfo();
  closeUserDropdown();

  // Mise à jour de l'état actif dans le menu
  document.querySelectorAll('#mainNav a[data-route]').forEach(a => {
    const on = a.dataset.route === route;
    a.classList.toggle('on', on);
    if (on) a.setAttribute('aria-current', 'page');
    else a.removeAttribute('aria-current');
  });

  const view = document.getElementById('view');
  if (MOBILE_QUERY.matches) setMobileNav(false);

  // Route spéciale : Mon profil
  if (route === 'profil') {
    document.title = 'Mon profil — Apex Maint';
    view.innerHTML = profilePage();
    return;
  }

  const mod = MODULES.find(m => m.id === route);
  if (!mod || !isModuleAllowed(mod)) {
    document.title = 'Accès refusé — Apex Maint';
    view.innerHTML = accessDeniedPage();
    return;
  }

  document.title = `${mod.label} — Apex Maint`;

  if (mod.phase > CURRENT_PHASE) {
    view.innerHTML = plannedPage(mod);
    return;
  }

  if (route === 'tableau-de-bord') {
    view.innerHTML = dashboardPage();
  } else if (route === 'interventions') {
    if (id) {
      view.innerHTML = interventionDetailPage(id);
      await initInterventionDetailPageView(id);
    } else {
      view.innerHTML = interventionsPage();
      await initInterventionsPageView();
    }
  } else if (route === 'utilisateurs') {
    view.innerHTML = usersPage();
    await initUsersPageView();
  } else if (route === 'sites') {
    view.innerHTML = sitesPage();
    await initSitesPageView();
  } else if (route === 'lignes-de-production') {
    view.innerHTML = productionLinesPage();
    await initProductionLinesPageView();
  } else if (route === 'equipements') {
    view.innerHTML = equipmentsPage();
    await initEquipmentsPageView();
  } else if (route === 'emplacements') {
    view.innerHTML = locationsPage();
    await initLocationsPageView();
  } else if (route === 'fournisseurs') {
    view.innerHTML = suppliersPage();
    await initSuppliersPageView();
  } else {
    view.innerHTML = plannedPage(mod);
  }
}

/** Écran affiché lorsqu'une permission est manquante. */
function accessDeniedPage() {
  return `
    <div class="access-denied">
      ${icon('i-shield')}
      <h1>Accès refusé</h1>
      <p>Vous ne disposez pas des permissions nécessaires pour accéder à cette rubrique.</p>
      <a href="#/tableau-de-bord" class="btn primary">${icon('i-dash')} <span>Retour au tableau de bord</span></a>
    </div>
  `;
}

/** Écran affiché pour les modules des phases ultérieures. */
function plannedPage(mod) {
  return `
    <section class="planned">
      ${icon(mod.icon)}
      <h1>${esc(mod.label)}</h1>
      <p>Ce module est prévu en phase ${esc(mod.phase)} de la feuille de route d'Apex Maint.</p>
    </section>
  `;
}

/** Page : Tableau de bord */
function dashboardPage() {
  const h = state.health;
  const status = state.online
    ? `<span class="state-ok">En ligne</span>`
    : `<span class="state-ko">Injoignable</span>`;
  const user = state.currentUser;
  const siteLabel = state.site ? state.site.name : (isAdmin() ? 'Tous les sites' : 'Site principal');

  return `
    <div class="page-head">${icon('i-dash')}<h1>Tableau de bord</h1></div>
    <div class="cards">
      <section class="card">
        <h2>${icon('i-user')} Session active</h2>
        <dl>
          <dt>Utilisateur</dt><dd><b>${esc(user ? (user.display_name || user.username) : '—')}</b></dd>
          <dt>Identifiant</dt><dd class="mono">${esc(user ? user.username : '—')}</dd>
          <dt>Rôle</dt><dd><span class="${roleBadgeClass(user ? user.role : '')}">${esc(user ? formatRole(user.role) : '—')}</span></dd>
          <dt>Site</dt><dd>${esc(siteLabel)}</dd>
        </dl>
      </section>
      <section class="card">
        <h2>${icon('i-server')} Moteur serveur</h2>
        <dl>
          <dt>État</dt><dd>${status}</dd>
          <dt>Produit</dt><dd>${esc(h ? h.product : 'Apex Maint')}</dd>
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
    </div>
  `;
}

// ============================================================================
// 11. PAGE MON PROFIL
// ============================================================================

function profilePage() {
  const user = state.currentUser;
  if (!user) return accessDeniedPage();

  const siteLabel = state.site ? state.site.name : (isAdmin() ? 'Tous les sites (administrateur global)' : 'Site principal');
  const permsList = [...state.permissions].sort();

  return `
    <div class="page-head">${icon('i-user')}<h1>Mon profil</h1></div>
    <div class="profile-layout">
      <section class="card">
        <div class="profile-avatar-row">
          <div class="profile-avatar-large">${icon('i-user')}</div>
          <div>
            <h2>${esc(user.display_name || user.username)}</h2>
            <p class="hint mono">${esc(user.username)}</p>
          </div>
        </div>
        <dl class="profile-info-dl">
          <dt>Rôle</dt><dd><span class="${roleBadgeClass(user.role)}">${esc(formatRole(user.role))}</span></dd>
          <dt>Site rattaché</dt><dd>${esc(siteLabel)}</dd>
          <dt>Dernière visite</dt><dd>${user.last_login_at ? esc(formatDate(user.last_login_at)) : 'Session actuelle'}</dd>
          <dt>Date création</dt><dd>${user.created_at ? esc(formatDate(user.created_at)) : '—'}</dd>
        </dl>
        <div class="profile-perms-box">
          <div class="profile-perms-title">${icon('i-shield')} Permissions accordées (${permsList.length})</div>
          <div class="profile-perms-list">
            ${permsList.length ? permsList.map(p => `<span class="badge-perm mono">${esc(p)}</span>`).join('') : '<span class="hint">Aucune permission explicite</span>'}
          </div>
        </div>
      </section>

      <section class="card">
        <h2>${icon('i-key')} Changer mon mot de passe</h2>
        <p class="hint form-help">La modification de votre mot de passe révoquera automatiquement toutes vos autres sessions actives.</p>
        <form class="form-grid" data-action="own-password-submit">
          <div id="profilePwdErrorBox" class="form-error" hidden>
            ${icon('i-alert')}
            <span id="profilePwdErrorText"></span>
          </div>
          <div class="form-group">
            <label for="profPwdCurrent">Mot de passe actuel</label>
            <input type="password" id="profPwdCurrent" name="current_password" required autocomplete="current-password" />
          </div>
          <div class="form-row">
            <div class="form-group">
              <label for="profPwdNew">Nouveau mot de passe</label>
              <input type="password" id="profPwdNew" name="new_password" required minlength="8" autocomplete="new-password" />
              <span class="form-help">Minimum 8 caractères.</span>
            </div>
            <div class="form-group">
              <label for="profPwdConfirm">Confirmer</label>
              <input type="password" id="profPwdConfirm" name="confirm_password" required minlength="8" autocomplete="new-password" />
            </div>
          </div>
          <button type="submit" class="btn primary" id="profPwdSubmitBtn">
            <span>Enregistrer le mot de passe</span>
          </button>
        </form>
      </section>
    </div>
  `;
}

/** Traite le changement de mot de passe depuis la page profil ou la modale. */
async function handleOwnPasswordSubmit(form, errorBoxId, errorTextId, submitBtnId) {
  const errorBox = document.getElementById(errorBoxId);
  const errorText = document.getElementById(errorTextId);
  const btn = document.getElementById(submitBtnId);

  const current_password = form.current_password.value;
  const new_password = form.new_password.value;
  const confirm_password = form.confirm_password.value;

  if (new_password.length < 8) {
    if (errorBox) { errorBox.hidden = false; errorText.textContent = 'Le mot de passe doit comporter au moins 8 caractères.'; }
    return;
  }
  if (new_password === current_password) {
    if (errorBox) { errorBox.hidden = false; errorText.textContent = 'Le nouveau mot de passe doit être différent du mot de passe actuel.'; }
    return;
  }
  if (new_password === 'admin123') {
    if (errorBox) { errorBox.hidden = false; errorText.textContent = 'Le mot de passe d\'usine « admin123 » ne peut pas être réutilisé.'; }
    return;
  }
  if (new_password !== confirm_password) {
    if (errorBox) { errorBox.hidden = false; errorText.textContent = 'Les deux nouveaux mots de passe ne correspondent pas.'; }
    return;
  }

  if (btn) { btn.disabled = true; }
  if (errorBox) errorBox.hidden = true;

  try {
    await api('/api/auth/password', {
      method: 'POST',
      body: { current_password, new_password },
    });
    form.reset();
    closeModal();
    notify('Votre mot de passe a été modifié avec succès.', 'success');
  } catch (err) {
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = err.message || 'Impossible de modifier le mot de passe.';
    }
  } finally {
    if (btn) btn.disabled = false;
  }
}

/** Ouvre la modale de changement de mot de passe (depuis le menu supérieur). */
function openChangePasswordModal() {
  closeUserDropdown();
  const bodyHtml = `
    <form class="form-grid" id="modalPwdForm" data-action="modal-password-submit">
      <div id="modalPwdErrorBox" class="form-error" hidden>
        ${icon('i-alert')}
        <span id="modalPwdErrorText"></span>
      </div>
      <div class="form-group">
        <label for="modalCurrentPwd">Mot de passe actuel</label>
        <input type="password" id="modalCurrentPwd" name="current_password" required autocomplete="current-password" autofocus />
      </div>
      <div class="form-group">
        <label for="modalNewPwd">Nouveau mot de passe</label>
        <input type="password" id="modalNewPwd" name="new_password" required minlength="8" autocomplete="new-password" />
        <span class="form-help">8 caractères minimum, différent de l'actuel et de « admin123 ».</span>
      </div>
      <div class="form-group">
        <label for="modalConfirmPwd">Confirmer le nouveau mot de passe</label>
        <input type="password" id="modalConfirmPwd" name="confirm_password" required minlength="8" autocomplete="new-password" />
      </div>
    </form>
  `;
  const footerHtml = `
    <button type="button" class="btn" data-action="close-modal">Annuler</button>
    <button type="submit" form="modalPwdForm" class="btn primary" id="modalPwdSubmitBtn">Enregistrer</button>
  `;
  openModal('Changer mon mot de passe', bodyHtml, footerHtml);
}

// ============================================================================
// 12. PAGE UTILISATEURS ET MATRICE DES PERMISSIONS
// ============================================================================

function usersPage() {
  const isAdm = isAdmin();
  const canMg = canManageUsers();

  return `
    <div class="page-head">${icon('i-users')}<h1>Gestion des utilisateurs</h1></div>
    ${isAdm ? `
      <div class="tabs-bar">
        <button type="button" class="tab-btn ${state.usersTab === 'users' ? 'active' : ''}" data-action="switch-users-tab" data-args="${jsArg('users')}">
          ${icon('i-users')} <span>Comptes utilisateurs</span>
        </button>
        <button type="button" class="tab-btn ${state.usersTab === 'permissions' ? 'active' : ''}" data-action="switch-users-tab" data-args="${jsArg('permissions')}">
          ${icon('i-shield')} <span>Matrice des permissions</span>
        </button>
      </div>
    ` : ''}
    <div id="usersTabContent">
      <p class="hint">${icon('i-clock')} Chargement des données…</p>
    </div>
  `;
}

async function initUsersPageView() {
  if (state.usersTab === 'permissions' && isAdmin()) {
    await loadPermissionsMatrixView();
  } else {
    state.usersTab = 'users';
    await loadUsersListView();
  }
}

/** Charge et affiche la liste des utilisateurs. */
async function loadUsersListView() {
  const container = document.getElementById('usersTabContent');
  if (!container) return;

  try {
    const list = await api('/api/users');
    state.usersList = Array.isArray(list) ? list : [];
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">Erreur lors du chargement des utilisateurs : ${esc(err.message)}</p>`;
    return;
  }

  renderUsersTable();
}

/** Rendu du tableau des utilisateurs avec filtres et actions. */
function renderUsersTable() {
  const container = document.getElementById('usersTabContent');
  if (!container) return;

  const canMg = canManageUsers();
  const isAdm = isAdmin();

  // Filtrage selon recherche et filtres de sélection
  const q = (state.usersSearchQuery || '').trim().toLowerCase();
  const roleFilter = state.usersRoleFilter || '';
  const siteFilter = state.usersSiteFilter || (isAdm ? state.selectedSiteId : '');

  const filtered = state.usersList.filter(u => {
    if (roleFilter && u.role !== roleFilter) return false;
    if (siteFilter && u.site_id !== siteFilter) return false;
    if (q) {
      const match = [u.username, u.display_name, formatRole(u.role)]
        .some(val => String(val || '').toLowerCase().includes(q));
      if (!match) return false;
    }
    return true;
  });

  const sitesMap = new Map(state.sites.map(s => [s.id, s.name]));

  const rows = filtered.map(u => {
    const siteName = u.site_id ? (sitesMap.get(u.site_id) || 'Site inconnu') : (u.role === 'admin' ? 'Tous les sites' : '—');
    const isSelf = state.currentUser && state.currentUser.id === u.id;
    const isLastAdmin = u.role === 'admin' && state.usersList.filter(x => x.role === 'admin' && x.active).length <= 1;

    let stateBadge = `<span class="badge badge-state-ok">Actif</span>`;
    if (u.locked) {
      stateBadge = `<span class="badge badge-state-ko">Verrouillé</span>`;
    } else if (!u.active) {
      stateBadge = `<span class="badge badge-state-warn">Désactivé</span>`;
    }

    const actionButtons = [];
    if (canMg) {
      // Un gestionnaire non-admin ne peut pas modifier un compte admin
      const canEditThisUser = isAdm || u.role !== 'admin';

      if (canEditThisUser) {
        actionButtons.push(`
          <button type="button" class="btn sm" title="Modifier le compte" data-action="open-edit-user" data-args="${jsArg(u.id)}">
            ${icon('i-edit')} <span>Modifier</span>
          </button>
        `);
        actionButtons.push(`
          <button type="button" class="btn sm" title="Réinitialiser le mot de passe" data-action="open-reset-user-pwd" data-args="${jsArg(u.id)}">
            ${icon('i-key')} <span>Mot de passe</span>
          </button>
        `);
        if (!isSelf && !isLastAdmin) {
          const toggleLabel = u.active ? 'Désactiver' : 'Activer';
          actionButtons.push(`
            <button type="button" class="btn sm" data-action="toggle-user-active" data-args="${jsArg(u.id)}">
              <span>${esc(toggleLabel)}</span>
            </button>
          `);
          actionButtons.push(`
            <button type="button" class="btn sm danger" title="Supprimer le compte" data-action="delete-user" data-args="${jsArg(u.id)}">
              ${icon('i-trash')}
            </button>
          `);
        }
      }
    }

    return `
      <tr>
        <td>
          <b>${esc(u.display_name || u.username)}</b>
          <br/><span class="hint mono">${esc(u.username)}</span>
        </td>
        <td><span class="${roleBadgeClass(u.role)}">${esc(formatRole(u.role))}</span></td>
        <td>${esc(siteName)}</td>
        <td>${stateBadge}</td>
        <td class="mono">${u.last_login_at ? esc(formatDate(u.last_login_at)) : '<span class="hint">Jamais</span>'}</td>
        <td><div class="cell-actions">${actionButtons.join('') || '<span class="hint">—</span>'}</div></td>
      </tr>
    `;
  }).join('');

  container.innerHTML = `
    <div class="toolbar">
      <div class="toolbar-group">
        <div class="search-input-wrap">
          ${icon('i-search')}
          <input type="search" placeholder="Rechercher…" value="${esc(state.usersSearchQuery)}" data-action="filter-users-search" aria-label="Rechercher un utilisateur" />
        </div>
        <select data-action="filter-users-role" aria-label="Filtrer par rôle">
          <option value="">Tous les rôles</option>
          <option value="admin" ${state.usersRoleFilter === 'admin' ? 'selected' : ''}>Administrateur</option>
          <option value="responsable" ${state.usersRoleFilter === 'responsable' ? 'selected' : ''}>Responsable</option>
          <option value="technicien" ${state.usersRoleFilter === 'technicien' ? 'selected' : ''}>Technicien</option>
          <option value="production" ${state.usersRoleFilter === 'production' ? 'selected' : ''}>Production</option>
        </select>
        ${isAdm ? `
          <select data-action="filter-users-site" aria-label="Filtrer par site">
            <option value="">Tous les sites</option>
            ${state.sites.map(s => `<option value="${esc(s.id)}" ${(state.usersSiteFilter || state.selectedSiteId) === s.id ? 'selected' : ''}>${esc(s.name)}</option>`).join('')}
          </select>
        ` : ''}
      </div>
      ${canMg ? `
        <button type="button" class="btn primary" data-action="open-create-user">
          ${icon('i-plus')} <span>Nouvel utilisateur</span>
        </button>
      ` : ''}
    </div>

    <div class="table-container">
      <table class="data-table">
        <thead>
          <tr>
            <th>Utilisateur</th>
            <th>Rôle</th>
            <th>Site</th>
            <th>État</th>
            <th>Dernière connexion</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          ${rows || '<tr><td colspan="6" class="empty-cell">Aucun utilisateur correspondant.</td></tr>'}
        </tbody>
      </table>
    </div>
  `;
}

/** Ouvre la modale de création d'un utilisateur. */
function openCreateUserModal() {
  const isAdm = isAdmin();
  const sites = state.sites;
  const userSiteId = state.site ? state.site.id : (sites[0] ? sites[0].id : '');

  const bodyHtml = `
    <form class="form-grid" id="modalUserForm" data-action="create-user-submit">
      <div id="modalUserErrorBox" class="form-error" hidden>
        ${icon('i-alert')}
        <span id="modalUserErrorText"></span>
      </div>
      <div class="form-row">
        <div class="form-group">
          <label for="uLogin">Identifiant (login)</label>
          <input type="text" id="uLogin" name="username" required pattern="[A-Za-z0-9_.-]{3,32}" autocomplete="off" autofocus />
          <span class="form-help">3 à 32 caractères alphanumériques.</span>
        </div>
        <div class="form-group">
          <label for="uName">Nom complet affiché</label>
          <input type="text" id="uName" name="display_name" required autocomplete="off" />
        </div>
      </div>
      <div class="form-row">
        <div class="form-group">
          <label for="uRole">Rôle</label>
          <select id="uRole" name="role" required data-action="on-user-role-change">
            ${isAdm ? '<option value="admin">Administrateur</option>' : ''}
            <option value="responsable" selected>Responsable</option>
            <option value="technicien">Technicien</option>
            <option value="production">Production</option>
          </select>
        </div>
        <div class="form-group">
          <label for="uSite">Site de rattachement</label>
          <select id="uSite" name="site_id" ${!isAdm ? 'disabled' : ''}>
            ${isAdm ? '<option value="">Aucun site (admin global)</option>' : ''}
            ${sites.map(s => `<option value="${esc(s.id)}" ${s.id === userSiteId ? 'selected' : ''}>${esc(s.name)}</option>`).join('')}
          </select>
        </div>
      </div>
      <div class="form-group">
        <label for="uPass">Mot de passe temporaire</label>
        <input type="password" id="uPass" name="password" required minlength="8" autocomplete="new-password" />
        <span class="form-help">Minimum 8 caractères. L'utilisateur devra le changer à la première connexion.</span>
      </div>
    </form>
  `;

  const footerHtml = `
    <button type="button" class="btn" data-action="close-modal">Annuler</button>
    <button type="submit" form="modalUserForm" class="btn primary" id="modalUserSubmitBtn">Créer l'utilisateur</button>
  `;

  openModal('Créer un utilisateur', bodyHtml, footerHtml);
}

/** Ouvre la modale de modification d'un utilisateur. */
function openEditUserModal(userId) {
  const user = state.usersList.find(u => u.id === userId);
  if (!user) return;

  const isAdm = isAdmin();
  const sites = state.sites;
  const isSelf = state.currentUser && state.currentUser.id === user.id;

  const bodyHtml = `
    <form class="form-grid" id="modalUserForm" data-action="edit-user-submit" data-args="${jsArg(user.id)}" data-id="${esc(user.id)}">
      <div id="modalUserErrorBox" class="form-error" hidden>
        ${icon('i-alert')}
        <span id="modalUserErrorText"></span>
      </div>
      <div class="form-row">
        <div class="form-group">
          <label for="uLogin">Identifiant</label>
          <input type="text" id="uLogin" name="username" value="${esc(user.username)}" readonly disabled />
        </div>
        <div class="form-group">
          <label for="uName">Nom complet affiché</label>
          <input type="text" id="uName" name="display_name" value="${esc(user.display_name)}" required autofocus />
        </div>
      </div>
      <div class="form-row">
        <div class="form-group">
          <label for="uRole">Rôle</label>
          <select id="uRole" name="role" required ${isSelf ? 'disabled title="Impossible de modifier son propre rôle"' : ''} data-action="on-user-role-change">
            ${isAdm ? `<option value="admin" ${user.role === 'admin' ? 'selected' : ''}>Administrateur</option>` : ''}
            <option value="responsable" ${user.role === 'responsable' ? 'selected' : ''}>Responsable</option>
            <option value="technicien" ${user.role === 'technicien' ? 'selected' : ''}>Technicien</option>
            <option value="production" ${user.role === 'production' ? 'selected' : ''}>Production</option>
          </select>
        </div>
        <div class="form-group">
          <label for="uSite">Site de rattachement</label>
          <select id="uSite" name="site_id" ${!isAdm ? 'disabled' : ''}>
            ${isAdm ? `<option value="" ${!user.site_id ? 'selected' : ''}>Aucun site (admin global)</option>` : ''}
            ${sites.map(s => `<option value="${esc(s.id)}" ${s.id === user.site_id ? 'selected' : ''}>${esc(s.name)}</option>`).join('')}
          </select>
        </div>
      </div>
      <div class="form-group">
        <label for="uActive">État du compte</label>
        <select id="uActive" name="active" ${isSelf ? 'disabled title="Impossible de désactiver son propre compte"' : ''}>
          <option value="true" ${user.active ? 'selected' : ''}>Actif</option>
          <option value="false" ${!user.active ? 'selected' : ''}>Désactivé</option>
        </select>
      </div>
    </form>
  `;

  const footerHtml = `
    <button type="button" class="btn" data-action="close-modal">Annuler</button>
    <button type="submit" form="modalUserForm" class="btn primary" id="modalUserSubmitBtn">Enregistrer</button>
  `;

  openModal(`Modifier l'utilisateur ${user.username}`, bodyHtml, footerHtml);
}

/** Traite la soumission de création d'utilisateur. */
async function handleCreateUserSubmit(form) {
  const errorBox = document.getElementById('modalUserErrorBox');
  const errorText = document.getElementById('modalUserErrorText');
  const submitBtn = document.getElementById('modalUserSubmitBtn');

  const username = form.username.value.trim();
  const display_name = form.display_name.value.trim();
  const role = form.role.value;
  const site_id = form.site_id ? (form.site_id.value || null) : (state.site ? state.site.id : null);
  const password = form.password.value;

  if (password.length < 8) {
    if (errorBox) { errorBox.hidden = false; errorText.textContent = 'Le mot de passe doit comporter au moins 8 caractères.'; }
    return;
  }

  if (role !== 'admin' && !site_id) {
    if (errorBox) { errorBox.hidden = false; errorText.textContent = 'Un site de rattachement est obligatoire pour ce rôle.'; }
    return;
  }

  submitBtn.disabled = true;
  if (errorBox) errorBox.hidden = true;

  try {
    await api('/api/users', {
      method: 'POST',
      body: { username, display_name, role, site_id, password, active: true },
    });
    closeModal();
    notify(`Utilisateur « ${username} » créé avec succès.`, 'success');
    await loadUsersListView();
  } catch (err) {
    submitBtn.disabled = false;
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = err.message || 'Impossible de créer cet utilisateur.';
    }
  }
}

/** Traite la soumission de modification d'utilisateur. */
async function handleEditUserSubmit(form) {
  let userId = form.dataset.id;
  if (form.dataset.args) {
    try {
      userId = JSON.parse(form.dataset.args);
    } catch (_) {
      userId = form.dataset.args;
    }
  }
  const errorBox = document.getElementById('modalUserErrorBox');
  const errorText = document.getElementById('modalUserErrorText');
  const submitBtn = document.getElementById('modalUserSubmitBtn');

  const display_name = form.display_name.value.trim();
  const role = form.role ? form.role.value : undefined;
  const site_id = form.site_id ? (form.site_id.value || null) : undefined;
  const active = form.active ? form.active.value === 'true' : undefined;

  submitBtn.disabled = true;
  if (errorBox) errorBox.hidden = true;

  const payload = { display_name };
  if (role !== undefined) payload.role = role;
  if (site_id !== undefined) payload.site_id = site_id;
  if (active !== undefined) payload.active = active;

  try {
    await api(`/api/users/${encodeURIComponent(userId)}`, {
      method: 'PATCH',
      body: payload,
    });
    closeModal();
    notify('Compte utilisateur mis à jour avec succès.', 'success');
    await loadUsersListView();
  } catch (err) {
    submitBtn.disabled = false;
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = err.message || 'Impossible de mettre à jour cet utilisateur.';
    }
  }
}

/** Ouvre la modale de réinitialisation du mot de passe d'un utilisateur. */
function openResetUserPwdModal(userId) {
  const user = state.usersList.find(u => u.id === userId);
  if (!user) return;

  const bodyHtml = `
    <form class="form-grid" id="modalResetPwdForm" data-action="reset-user-pwd-submit" data-args="${jsArg(user.id)}" data-id="${esc(user.id)}">
      <div id="modalResetErrorBox" class="form-error" hidden>
        ${icon('i-alert')}
        <span id="modalResetErrorText"></span>
      </div>
      <p>Vous réinitialisez le mot de passe pour le compte <b>${esc(user.display_name || user.username)}</b> (${esc(user.username)}).</p>
      <div class="form-group">
        <label for="resetNewPwd">Nouveau mot de passe temporaire</label>
        <input type="password" id="resetNewPwd" name="new_password" required minlength="8" autocomplete="new-password" autofocus />
        <span class="form-help">Minimum 8 caractères. L'utilisateur devra obligatoirement le changer lors de sa prochaine connexion.</span>
      </div>
    </form>
  `;

  const footerHtml = `
    <button type="button" class="btn" data-action="close-modal">Annuler</button>
    <button type="submit" form="modalResetPwdForm" class="btn primary" id="modalResetSubmitBtn">Réinitialiser</button>
  `;

  openModal('Réinitialiser le mot de passe', bodyHtml, footerHtml);
}

/** Traite la réinitialisation du mot de passe d'un utilisateur. */
async function handleResetUserPwdSubmit(form) {
  let userId = form.dataset.id;
  if (form.dataset.args) {
    try {
      userId = JSON.parse(form.dataset.args);
    } catch (_) {
      userId = form.dataset.args;
    }
  }
  const new_password = form.new_password.value;
  const errorBox = document.getElementById('modalResetErrorBox');
  const errorText = document.getElementById('modalResetErrorText');
  const submitBtn = document.getElementById('modalResetSubmitBtn');

  if (new_password.length < 8) {
    if (errorBox) { errorBox.hidden = false; errorText.textContent = 'Le mot de passe doit comporter au moins 8 caractères.'; }
    return;
  }

  submitBtn.disabled = true;
  if (errorBox) errorBox.hidden = true;

  try {
    await api(`/api/users/${encodeURIComponent(userId)}/password`, {
      method: 'POST',
      body: { new_password },
    });
    closeModal();
    notify('Mot de passe réinitialisé. L\'utilisateur devra le changer à sa prochaine connexion.', 'success');
  } catch (err) {
    submitBtn.disabled = false;
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = err.message || 'Impossible de réinitialiser le mot de passe.';
    }
  }
}

/** Active ou désactive un utilisateur. */
async function toggleUserActive(userId) {
  const user = state.usersList.find(u => u.id === userId);
  if (!user) return;

  const nextState = !user.active;
  const actionText = nextState ? 'Activer' : 'Désactiver';

  const ok = await confirmDialog(
    `${actionText} l'utilisateur ?`,
    `Voulez-vous vraiment ${actionText.toLowerCase()} le compte de « ${user.display_name || user.username} » ?`,
    actionText,
    !nextState
  );
  if (!ok) return;

  try {
    await api(`/api/users/${encodeURIComponent(userId)}`, {
      method: 'PATCH',
      body: { active: nextState },
    });
    notify(`Compte « ${user.username} » ${nextState ? 'activé' : 'désactivé'}.`, 'success');
    await loadUsersListView();
  } catch (err) {
    notify(`Erreur : ${err.message}`, 'error');
  }
}

/** Supprime un utilisateur (suppression logique). */
async function deleteUser(userId) {
  const user = state.usersList.find(u => u.id === userId);
  if (!user) return;

  const ok = await confirmDialog(
    'Supprimer l\'utilisateur ?',
    `Voulez-vous vraiment supprimer définitivement le compte de « ${user.display_name || user.username} » (${user.username}) ? Cette opération révoquera toutes ses sessions.`,
    'Supprimer',
    true
  );
  if (!ok) return;

  try {
    await api(`/api/users/${encodeURIComponent(userId)}`, {
      method: 'DELETE',
    });
    notify(`Compte « ${user.username} » supprimé.`, 'success');
    await loadUsersListView();
  } catch (err) {
    notify(`Erreur : ${err.message}`, 'error');
  }
}

// ============================================================================
// 13. MATRICE DES PERMISSIONS RBAC
// ============================================================================

/** Charge et affiche l'onglet de la matrice des permissions (admin seulement). */
async function loadPermissionsMatrixView() {
  const container = document.getElementById('usersTabContent');
  if (!container) return;

  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement de la matrice des permissions…</p>`;

  try {
    const data = await api('/api/permissions');
    state.permissionsMatrix = data;
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">Erreur lors de la récupération des permissions : ${esc(err.message)}</p>`;
    return;
  }

  renderPermissionsMatrixTable();
}

/** Extrait la matrice sous forme normalisée d'un objet { responsable: Set, technicien: Set, production: Set }. */
function extractRolePermissionsMap(data) {
  const map = {
    admin: new Set(),
    responsable: new Set(),
    technicien: new Set(),
    production: new Set(),
  };

  // L'admin possède toujours l'ensemble de toutes les permissions
  PERMISSION_DOMAINS.forEach(d => d.permissions.forEach(p => map.admin.add(p.key)));

  if (!data || !data.matrix) return map;

  ['responsable', 'technicien', 'production'].forEach(role => {
    const list = data.matrix[role];
    if (Array.isArray(list)) {
      list.forEach(p => map[role].add(p));
    }
  });

  return map;
}

/** Rendu de la table matrice de permissions à cases à cocher. */
function renderPermissionsMatrixTable() {
  const container = document.getElementById('usersTabContent');
  if (!container) return;

  const roleMap = extractRolePermissionsMap(state.permissionsMatrix);

  let rowsHtml = '';
  PERMISSION_DOMAINS.forEach(domain => {
    rowsHtml += `
      <tr class="perm-domain-row">
        <td colspan="5">${esc(domain.name)}</td>
      </tr>
    `;
    domain.permissions.forEach(p => {
      rowsHtml += `
        <tr>
          <td class="perm-label-cell">
            <div class="perm-label-title">${esc(p.label)}</div>
            <div class="perm-label-key mono">${esc(p.key)}</div>
          </td>
          <td class="perm-check-cell">
            <input type="checkbox" checked disabled title="L'administrateur possède toujours toutes les permissions" />
          </td>
          <td class="perm-check-cell">
            <input type="checkbox" data-role="responsable" data-perm="${esc(p.key)}" ${roleMap.responsable.has(p.key) ? 'checked' : ''} aria-label="${esc(p.key)} pour Responsable" />
          </td>
          <td class="perm-check-cell">
            <input type="checkbox" data-role="technicien" data-perm="${esc(p.key)}" ${roleMap.technicien.has(p.key) ? 'checked' : ''} aria-label="${esc(p.key)} pour Technicien" />
          </td>
          <td class="perm-check-cell">
            <input type="checkbox" data-role="production" data-perm="${esc(p.key)}" ${roleMap.production.has(p.key) ? 'checked' : ''} aria-label="${esc(p.key)} pour Production" />
          </td>
        </tr>
      `;
    });
  });

  container.innerHTML = `
    <div class="table-container perm-table-container">
      <table class="data-table perm-table">
        <thead>
          <tr>
            <th>Permissions par domaine</th>
            <th class="th-role"><span class="${roleBadgeClass('admin')}">Admin</span></th>
            <th class="th-role"><span class="${roleBadgeClass('responsable')}">Responsable</span></th>
            <th class="th-role"><span class="${roleBadgeClass('technicien')}">Technicien</span></th>
            <th class="th-role"><span class="${roleBadgeClass('production')}">Production</span></th>
          </tr>
        </thead>
        <tbody>
          ${rowsHtml}
        </tbody>
      </table>
    </div>
    <div class="perm-footer">
      <button type="button" class="btn primary" id="savePermsBtn" data-action="save-permissions-matrix">
        ${icon('i-check')} <span>Enregistrer les permissions</span>
      </button>
    </div>
  `;
}

/** Enregistre les modifications de la matrice via PUT /api/permissions/{role}. */
async function savePermissionsMatrix() {
  const btn = document.getElementById('savePermsBtn');
  if (btn) {
    btn.disabled = true;
    btn.innerHTML = `${icon('i-clock')} <span>Enregistrement…</span>`;
  }

  const roles = ['responsable', 'technicien', 'production'];
  const permsByRole = {
    responsable: [],
    technicien: [],
    production: [],
  };

  document.querySelectorAll('input[type="checkbox"][data-role]:checked').forEach(cb => {
    const r = cb.dataset.role;
    const p = cb.dataset.perm;
    if (permsByRole[r]) permsByRole[r].push(p);
  });

  try {
    for (const role of roles) {
      const permsList = permsByRole[role];
      await api(`/api/permissions/${encodeURIComponent(role)}`, {
        method: 'PUT',
        body: { permissions: permsList },
      });
    }
    notify('Matrice des permissions enregistrée avec succès.', 'success');
    await loadPermissionsMatrixView();
  } catch (err) {
    notify(`Erreur lors de l'enregistrement : ${err.message}`, 'error');
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.innerHTML = `${icon('i-check')} <span>Enregistrer les permissions</span>`;
    }
  }
}

// ============================================================================
// 14. PAGE GESTION DES SITES (ADMIN SEULEMENT)
// ============================================================================

function sitesPage() {
  if (!isAdmin()) return accessDeniedPage();

  return `
    <div class="page-head">${icon('i-site')}<h1>Gestion des sites</h1></div>
    <div class="toolbar">
      <div class="toolbar-group">
        <p class="hint">Administration des sites géographiques et industriels de la GMAO.</p>
      </div>
      <button type="button" class="btn primary" data-action="open-create-site">
        ${icon('i-plus')} <span>Nouveau site</span>
      </button>
    </div>
    <div id="sitesTableContainer">
      <p class="hint">${icon('i-clock')} Chargement des sites…</p>
    </div>
  `;
}

async function initSitesPageView() {
  await loadSites();
  renderSitesTable();
}

/** Rendu du tableau des sites. */
function renderSitesTable() {
  const container = document.getElementById('sitesTableContainer');
  if (!container) return;

  const sites = state.sites;
  if (!sites.length) {
    container.innerHTML = `
      <div class="table-container">
        <table class="data-table">
          <tbody>
            <tr><td class="empty-cell">Aucun site enregistré.</td></tr>
          </tbody>
        </table>
      </div>
    `;
    return;
  }

  const rows = sites.map(s => `
    <tr>
      <td><b>${esc(s.name)}</b></td>
      <td class="mono hint">${esc(s.id)}</td>
      <td class="mono">${s.created_at ? esc(formatDate(s.created_at)) : '—'}</td>
      <td>
        <div class="cell-actions">
          <button type="button" class="btn sm" title="Renommer le site" data-action="open-edit-site" data-args="${jsArg(s.id)}">
            ${icon('i-edit')} <span>Renommer</span>
          </button>
          <button type="button" class="btn sm danger" title="Supprimer le site" data-action="delete-site" data-args="${jsArg(s.id)}">
            ${icon('i-trash')} <span>Supprimer</span>
          </button>
        </div>
      </td>
    </tr>
  `).join('');

  container.innerHTML = `
    <div class="table-container">
      <table class="data-table">
        <thead>
          <tr>
            <th>Nom du site</th>
            <th>Identifiant</th>
            <th>Date de création</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          ${rows}
        </tbody>
      </table>
    </div>
  `;
}

/** Ouvre la modale de création d'un nouveau site. */
function openCreateSiteModal() {
  const bodyHtml = `
    <form class="form-grid" id="modalSiteForm" data-action="create-site-submit">
      <div id="modalSiteErrorBox" class="form-error" hidden>
        ${icon('i-alert')}
        <span id="modalSiteErrorText"></span>
      </div>
      <div class="form-group">
        <label for="siteName">Nom du site</label>
        <input type="text" id="siteName" name="name" required autocomplete="off" autofocus placeholder="ex. Usine Nord, Ligne Sud..." />
      </div>
    </form>
  `;
  const footerHtml = `
    <button type="button" class="btn" data-action="close-modal">Annuler</button>
    <button type="submit" form="modalSiteForm" class="btn primary" id="modalSiteSubmitBtn">Créer le site</button>
  `;
  openModal('Nouveau site', bodyHtml, footerHtml);
}

/** Ouvre la modale de renommage d'un site. */
function openEditSiteModal(siteId) {
  const site = state.sites.find(s => s.id === siteId);
  if (!site) return;

  const bodyHtml = `
    <form class="form-grid" id="modalSiteForm" data-action="edit-site-submit" data-args="${jsArg(site.id)}" data-id="${esc(site.id)}">
      <div id="modalSiteErrorBox" class="form-error" hidden>
        ${icon('i-alert')}
        <span id="modalSiteErrorText"></span>
      </div>
      <div class="form-group">
        <label for="siteName">Nom du site</label>
        <input type="text" id="siteName" name="name" value="${esc(site.name)}" required autocomplete="off" autofocus />
      </div>
    </form>
  `;
  const footerHtml = `
    <button type="button" class="btn" data-action="close-modal">Annuler</button>
    <button type="submit" form="modalSiteForm" class="btn primary" id="modalSiteSubmitBtn">Enregistrer</button>
  `;
  openModal(`Renommer le site « ${site.name} »`, bodyHtml, footerHtml);
}

/** Traite la création d'un site. */
async function handleCreateSiteSubmit(form) {
  const name = form.name.value.trim();
  const errorBox = document.getElementById('modalSiteErrorBox');
  const errorText = document.getElementById('modalSiteErrorText');
  const submitBtn = document.getElementById('modalSiteSubmitBtn');

  if (!name) return;

  submitBtn.disabled = true;
  if (errorBox) errorBox.hidden = true;

  try {
    await api('/api/sites', {
      method: 'POST',
      body: { name },
    });
    closeModal();
    notify(`Site « ${name} » créé avec succès.`, 'success');
    await loadSites();
    renderSitesTable();
  } catch (err) {
    submitBtn.disabled = false;
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = err.message || 'Impossible de créer ce site.';
    }
  }
}

/** Traite le renommage d'un site. */
async function handleEditSiteSubmit(form) {
  let siteId = form.dataset.id;
  if (form.dataset.args) {
    try {
      siteId = JSON.parse(form.dataset.args);
    } catch (_) {
      siteId = form.dataset.args;
    }
  }
  const name = form.name.value.trim();
  const errorBox = document.getElementById('modalSiteErrorBox');
  const errorText = document.getElementById('modalSiteErrorText');
  const submitBtn = document.getElementById('modalSiteSubmitBtn');

  if (!name) return;

  submitBtn.disabled = true;
  if (errorBox) errorBox.hidden = true;

  try {
    await api(`/api/sites/${encodeURIComponent(siteId)}`, {
      method: 'PATCH',
      body: { name },
    });
    closeModal();
    notify('Site renommé avec succès.', 'success');
    await loadSites();
    renderSitesTable();
  } catch (err) {
    submitBtn.disabled = false;
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = err.message || 'Impossible de renommer ce site.';
    }
  }
}

/** Supprime un site après confirmation. */
async function deleteSite(siteId) {
  const site = state.sites.find(s => s.id === siteId);
  if (!site) return;

  const ok = await confirmDialog(
    'Supprimer le site ?',
    `Voulez-vous vraiment supprimer le site « ${site.name} » ? Tout site ayant des utilisateurs actifs ne pourra pas être supprimé.`,
    'Supprimer',
    true
  );
  if (!ok) return;

  try {
    await api(`/api/sites/${encodeURIComponent(siteId)}`, {
      method: 'DELETE',
    });
    notify(`Site « ${site.name} » supprimé.`, 'success');
    await loadSites();
    renderSitesTable();
  } catch (err) {
    notify(`Impossible de supprimer le site : ${err.message}`, 'error');
  }
}

// ============================================================================
// 14B. PAGE LIGNES DE PRODUCTION (PHASE 2)
// ============================================================================

function productionLinesPage() {
  const canCreate = hasPermission('production_lines.create');
  return `
    <div class="page-head">${icon('i-line')}<h1>Lignes de production</h1></div>
    ${renderToolbar({
      searchAction: 'filter-production-lines-search',
      searchValue: state.productionLinesSearchQuery,
      searchPlaceholder: 'Rechercher par code ou nom…',
      primaryAction: 'open-create-production-line',
      primaryLabel: 'Nouvelle ligne',
      primaryIcon: 'i-plus',
      canCreate,
    })}
    <div id="productionLinesContainer">
      <p class="hint">${icon('i-clock')} Chargement des lignes de production…</p>
    </div>
  `;
}

async function initProductionLinesPageView() {
  await loadProductionLines(true);
}

async function loadProductionLines(reset = true) {
  const container = document.getElementById('productionLinesContainer');
  if (!container && reset) return;

  if (reset) {
    state.productionLines = [];
    state.productionLinesNextCursor = null;
  }

  state.productionLinesLoading = true;
  if (!reset) renderProductionLinesTable();

  try {
    const params = new URLSearchParams();
    params.set('limit', '50');
    if (state.productionLinesSearchQuery) params.set('q', state.productionLinesSearchQuery.trim());
    if (state.selectedSiteId) params.set('site_id', state.selectedSiteId);
    if (!reset && state.productionLinesNextCursor) params.set('cursor', state.productionLinesNextCursor);

    const res = await api(`/api/production-lines?${params.toString()}`);
    const items = res && Array.isArray(res.items) ? res.items : [];
    const nextCursor = res ? res.next_cursor : null;

    if (reset) {
      state.productionLines = items;
    } else {
      state.productionLines = state.productionLines.concat(items);
    }
    state.productionLinesNextCursor = nextCursor;
  } catch (err) {
    if (container) {
      container.innerHTML = `<p class="hint state-ko">Erreur lors du chargement des lignes : ${esc(err.message)}</p>`;
    }
    state.productionLinesLoading = false;
    return;
  }

  state.productionLinesLoading = false;
  renderProductionLinesTable();
}

function renderProductionLinesTable() {
  const container = document.getElementById('productionLinesContainer');
  if (!container) return;

  const lines = state.productionLines;
  const canEdit = hasPermission('production_lines.edit');
  const canDel = hasPermission('production_lines.delete');
  const isAdm = isAdmin();
  const showSiteCol = isAdm && !state.selectedSiteId;
  const sitesMap = new Map(state.sites.map(s => [s.id, s.name]));

  if (!lines.length && !state.productionLinesLoading) {
    container.innerHTML = `
      <div class="table-container">
        <table class="data-table">
          <tbody>
            <tr><td class="empty-cell">Aucune ligne de production trouvée.</td></tr>
          </tbody>
        </table>
      </div>
    `;
    return;
  }

  const rows = lines.map(line => {
    const siteName = line.site_id ? (sitesMap.get(line.site_id) || 'Site inconnu') : '—';
    const actionButtons = [];
    if (canEdit) {
      actionButtons.push(`
        <button type="button" class="btn sm" title="Modifier la ligne" data-action="open-edit-production-line" data-args="${jsArg(line.id)}">
          ${icon('i-edit')} <span>Modifier</span>
        </button>
      `);
    }
    if (canDel) {
      actionButtons.push(`
        <button type="button" class="btn sm danger" title="Supprimer la ligne" data-action="delete-production-line" data-args="${jsArg(line.id)}">
          ${icon('i-trash')} <span>Supprimer</span>
        </button>
      `);
    }

    return `
      <tr>
        <td><b class="mono">${esc(line.code)}</b></td>
        <td><b>${esc(line.name)}</b></td>
        <td>${esc(line.description || '—')}</td>
        ${showSiteCol ? `<td>${esc(siteName)}</td>` : ''}
        <td class="mono">${esc(line.equipment_count !== undefined ? line.equipment_count : 0)}</td>
        <td><div class="cell-actions">${actionButtons.join('') || '<span class="hint">—</span>'}</div></td>
      </tr>
    `;
  }).join('');

  container.innerHTML = `
    <div class="table-container">
      <table class="data-table">
        <thead>
          <tr>
            <th>Code</th>
            <th>Nom de la ligne</th>
            <th>Description</th>
            ${showSiteCol ? '<th>Site</th>' : ''}
            <th>Équipements</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          ${rows}
        </tbody>
      </table>
    </div>
    ${renderLoadMore(state.productionLinesNextCursor, 'load-more-production-lines', state.productionLinesLoading)}
  `;
}

function openCreateProductionLineModal() {
  const fieldsHtml = `
    ${renderSiteFormField(state.selectedSiteId)}
    <div class="form-row">
      <div class="form-group">
        <label for="plCode">Code *</label>
        <input type="text" id="plCode" name="code" required autocomplete="off" autofocus placeholder="ex. LIGNE-1" />
        <span class="form-help">Code unique par site.</span>
      </div>
      <div class="form-group">
        <label for="plName">Nom de la ligne *</label>
        <input type="text" id="plName" name="name" required autocomplete="off" placeholder="ex. Ligne de conditionnement A" />
      </div>
    </div>
    <div class="form-group">
      <label for="plDesc">Description</label>
      <textarea id="plDesc" name="description" rows="3" placeholder="Description facultative…"></textarea>
    </div>
  `;

  openFormModal({
    title: 'Nouvelle ligne de production',
    formId: 'modalProductionLineForm',
    submitAction: 'create-production-line-submit',
    submitLabel: 'Créer la ligne',
    fieldsHtml,
  });
}

async function handleCreateProductionLineSubmit(form) {
  const btn = document.getElementById('modalProductionLineFormSubmitBtn');
  const errBox = document.getElementById('modalProductionLineFormErrorBox');
  const errText = document.getElementById('modalProductionLineFormErrorBoxText');

  const site_id = form.site_id ? form.site_id.value : (state.site ? state.site.id : null);
  const code = (form.code.value || '').trim();
  const name = (form.name.value || '').trim();
  const description = (form.description.value || '').trim() || null;

  btn.disabled = true;
  errBox.hidden = true;

  try {
    const payload = { code, name, description };
    if (isAdmin()) {
      if (!site_id) {
        throw new Error('Veuillez sélectionner un site de rattachement.');
      }
      payload.site_id = site_id;
    }

    await api('/api/production-lines', {
      method: 'POST',
      body: payload,
    });

    notify(`Ligne « ${code} » créée avec succès.`, 'success');
    closeModal();
    await loadProductionLines(true);
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || 'Impossible de créer la ligne de production.';
  }
}

function openEditProductionLineModal(lineId) {
  const line = state.productionLines.find(l => l.id === lineId);
  if (!line) return;

  const fieldsHtml = `
    <div class="form-row">
      <div class="form-group">
        <label for="plCode">Code *</label>
        <input type="text" id="plCode" name="code" value="${esc(line.code)}" required autocomplete="off" autofocus />
      </div>
      <div class="form-group">
        <label for="plName">Nom de la ligne *</label>
        <input type="text" id="plName" name="name" value="${esc(line.name)}" required autocomplete="off" />
      </div>
    </div>
    <div class="form-group">
      <label for="plDesc">Description</label>
      <textarea id="plDesc" name="description" rows="3">${esc(line.description || '')}</textarea>
    </div>
  `;

  openFormModal({
    title: `Modifier la ligne ${line.code}`,
    formId: 'modalProductionLineForm',
    submitAction: 'edit-production-line-submit',
    submitArgs: line.id,
    submitLabel: 'Enregistrer les modifications',
    fieldsHtml,
  });
}

async function handleEditProductionLineSubmit(form) {
  const lineId = form.dataset.args ? JSON.parse(form.dataset.args) : null;
  if (!lineId) return;

  const btn = document.getElementById('modalProductionLineFormSubmitBtn');
  const errBox = document.getElementById('modalProductionLineFormErrorBox');
  const errText = document.getElementById('modalProductionLineFormErrorBoxText');

  const code = (form.code.value || '').trim();
  const name = (form.name.value || '').trim();
  const description = (form.description.value || '').trim() || null;

  btn.disabled = true;
  errBox.hidden = true;

  try {
    await api(`/api/production-lines/${encodeURIComponent(lineId)}`, {
      method: 'PATCH',
      body: { code, name, description },
    });

    notify('Ligne modifiée avec succès.', 'success');
    closeModal();
    await loadProductionLines(true);
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || 'Impossible de modifier la ligne de production.';
  }
}

async function deleteProductionLine(lineId) {
  const line = state.productionLines.find(l => l.id === lineId);
  const name = line ? line.name : 'cette ligne';

  const ok = await confirmDialog(
    'Supprimer la ligne de production',
    `Voulez-vous vraiment supprimer « ${name} » ?\nCette opération sera refusée si des équipements y sont rattachés.`,
    'Supprimer',
    true
  );
  if (!ok) return;

  try {
    await api(`/api/production-lines/${encodeURIComponent(lineId)}`, {
      method: 'DELETE',
    });
    notify('Ligne de production supprimée.', 'success');
    await loadProductionLines(true);
  } catch (err) {
    notify(err.message || 'Suppression refusée.', 'error');
  }
}

// ============================================================================
// 14C. PAGE ÉQUIPEMENTS (PHASE 2)
// ============================================================================

function equipmentsPage() {
  const canCreate = hasPermission('equipments.create');
  const isTree = state.equipmentsViewMode === 'tree';

  const extraFilters = `
    <select data-action="filter-equipments-status" aria-label="Filtrer par statut">
      <option value="">Tous les statuts</option>
      <option value="operational" ${state.equipmentsStatusFilter === 'operational' ? 'selected' : ''}>En service</option>
      <option value="degraded" ${state.equipmentsStatusFilter === 'degraded' ? 'selected' : ''}>Dégradé</option>
      <option value="out_of_service" ${state.equipmentsStatusFilter === 'out_of_service' ? 'selected' : ''}>Hors service</option>
    </select>
    <select id="equipmentsLineFilterSelect" data-action="filter-equipments-line" aria-label="Filtrer par ligne">
      <option value="">Toutes les lignes</option>
      ${state.productionLines.map(l => `<option value="${esc(l.id)}" ${state.equipmentsLineFilter === l.id ? 'selected' : ''}>${esc(l.name)}</option>`).join('')}
    </select>
  `;

  const viewSwitcherHtml = `
    <div class="view-switch" role="group" aria-label="Mode d'affichage">
      <button type="button" class="view-switch-btn ${!isTree ? 'active' : ''}" data-action="switch-equipments-view" data-args="${jsArg('list')}" title="Affichage en liste">
        ${icon('i-dash')} <span>Liste</span>
      </button>
      <button type="button" class="view-switch-btn ${isTree ? 'active' : ''}" data-action="switch-equipments-view" data-args="${jsArg('tree')}" title="Affichage arborescent">
        ${icon('i-gear')} <span>Arborescence</span>
      </button>
    </div>
  `;

  return `
    <div class="page-head">${icon('i-gear')}<h1>Équipements</h1></div>
    ${renderToolbar({
      searchAction: isTree ? null : 'filter-equipments-search',
      searchValue: state.equipmentsSearchQuery,
      searchPlaceholder: 'Rechercher par code ou nom…',
      extraFiltersHtml: isTree ? '' : extraFilters,
      primaryAction: 'open-create-equipment',
      primaryLabel: 'Nouvel équipement',
      primaryIcon: 'i-plus',
      canCreate,
      viewSwitcherHtml,
    })}
    <div id="equipmentsContainer">
      <p class="hint">${icon('i-clock')} Chargement des équipements…</p>
    </div>
  `;
}

async function initEquipmentsPageView() {
  if (!state.productionLines.length) {
    try {
      const siteParam = state.selectedSiteId ? `?site_id=${encodeURIComponent(state.selectedSiteId)}&limit=100` : '?limit=100';
      const pl = await api(`/api/production-lines${siteParam}`);
      if (pl && Array.isArray(pl.items)) state.productionLines = pl.items;
      const sel = document.getElementById('equipmentsLineFilterSelect');
      if (sel) {
        sel.innerHTML = `<option value="">Toutes les lignes</option>` +
          state.productionLines.map(l => `<option value="${esc(l.id)}" ${state.equipmentsLineFilter === l.id ? 'selected' : ''}>${esc(l.name)}</option>`).join('');
      }
    } catch (_) {}
  }

  if (state.equipmentsViewMode === 'tree') {
    await loadEquipmentsTree();
  } else {
    await loadEquipmentsList(true);
  }
}

async function loadEquipmentsList(reset = true) {
  const container = document.getElementById('equipmentsContainer');
  if (!container && reset) return;

  if (reset) {
    state.equipments = [];
    state.equipmentsNextCursor = null;
  }

  state.equipmentsLoading = true;
  if (!reset) renderEquipmentsListView();

  try {
    const params = new URLSearchParams();
    params.set('limit', '50');
    if (state.equipmentsSearchQuery) params.set('q', state.equipmentsSearchQuery.trim());
    if (state.selectedSiteId) params.set('site_id', state.selectedSiteId);
    if (state.equipmentsStatusFilter) params.set('status', state.equipmentsStatusFilter);
    if (state.equipmentsLineFilter) params.set('production_line_id', state.equipmentsLineFilter);
    if (!reset && state.equipmentsNextCursor) params.set('cursor', state.equipmentsNextCursor);

    const res = await api(`/api/equipments?${params.toString()}`);
    const items = res && Array.isArray(res.items) ? res.items : [];
    const nextCursor = res ? res.next_cursor : null;

    if (reset) {
      state.equipments = items;
    } else {
      state.equipments = state.equipments.concat(items);
    }
    state.equipmentsNextCursor = nextCursor;
  } catch (err) {
    if (container) {
      container.innerHTML = `<p class="hint state-ko">Erreur lors du chargement des équipements : ${esc(err.message)}</p>`;
    }
    state.equipmentsLoading = false;
    return;
  }

  state.equipmentsLoading = false;
  renderEquipmentsListView();
}

function renderEquipmentsListView() {
  const container = document.getElementById('equipmentsContainer');
  if (!container) return;

  const items = state.equipments;
  const canEdit = hasPermission('equipments.edit');
  const canDel = hasPermission('equipments.delete');

  if (!items.length && !state.equipmentsLoading) {
    container.innerHTML = `
      <div class="table-container">
        <table class="data-table">
          <tbody>
            <tr><td class="empty-cell">Aucun équipement correspondant aux critères.</td></tr>
          </tbody>
        </table>
      </div>
    `;
    return;
  }

  const rows = items.map(eq => {
    const lineLabel = eq.production_line ? eq.production_line.name : '—';
    const parentLabel = eq.parent ? eq.parent.name : '—';

    const actionButtons = [
      `<button type="button" class="btn sm" title="Fiche détaillée" data-action="open-detail-equipment" data-args="${jsArg(eq.id)}">
        ${icon('i-info')} <span>Détail</span>
      </button>`
    ];

    if (canEdit) {
      actionButtons.push(`
        <button type="button" class="btn sm" title="Modifier l'équipement" data-action="open-edit-equipment" data-args="${jsArg(eq.id)}">
          ${icon('i-edit')} <span>Modifier</span>
        </button>
      `);
    }

    if (canDel) {
      actionButtons.push(`
        <button type="button" class="btn sm danger" title="Supprimer l'équipement" data-action="delete-equipment" data-args="${jsArg(eq.id)}">
          ${icon('i-trash')}
        </button>
      `);
    }

    return `
      <tr>
        <td>
          <button type="button" class="link-action mono" data-action="open-detail-equipment" data-args="${jsArg(eq.id)}">
            ${esc(eq.code)}
          </button>
        </td>
        <td>
          <button type="button" class="link-action" data-action="open-detail-equipment" data-args="${jsArg(eq.id)}">
            <b>${esc(eq.name)}</b>
          </button>
        </td>
        <td>${statusBadge(eq.status)}</td>
        <td>${esc(lineLabel)}</td>
        <td>${esc(parentLabel)}</td>
        <td class="mono">${esc(eq.children_count || 0)}</td>
        <td><div class="cell-actions">${actionButtons.join('')}</div></td>
      </tr>
    `;
  }).join('');

  container.innerHTML = `
    <div class="table-container">
      <table class="data-table">
        <thead>
          <tr>
            <th>Code</th>
            <th>Nom de l'équipement</th>
            <th>Statut</th>
            <th>Ligne</th>
            <th>Équipement parent</th>
            <th>Sous-équipements</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          ${rows}
        </tbody>
      </table>
    </div>
    ${renderLoadMore(state.equipmentsNextCursor, 'load-more-equipments', state.equipmentsLoading)}
  `;
}

async function loadEquipmentsTree() {
  const container = document.getElementById('equipmentsContainer');
  if (!container) return;

  state.equipmentsTreeLoading = true;
  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement de l'arborescence des équipements…</p>`;

  try {
    const siteParam = state.selectedSiteId ? `?site_id=${encodeURIComponent(state.selectedSiteId)}` : '';
    const tree = await api(`/api/equipments/tree${siteParam}`);
    state.equipmentsTree = Array.isArray(tree) ? tree : [];

    if (state.equipmentsTreeExpanded.size === 0 && state.equipmentsTree.length > 0) {
      for (const node of state.equipmentsTree) {
        state.equipmentsTreeExpanded.add(node.id);
      }
    }
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">Erreur lors du chargement de l'arborescence : ${esc(err.message)}</p>`;
    state.equipmentsTreeLoading = false;
    return;
  }

  state.equipmentsTreeLoading = false;
  renderEquipmentsTreeView();
}

function renderEquipmentsTreeView() {
  const container = document.getElementById('equipmentsContainer');
  if (!container) return;

  if (!state.equipmentsTree.length) {
    container.innerHTML = `
      <div class="tree-container">
        <p class="hint">Aucun équipement enregistré dans l'arborescence.</p>
      </div>
    `;
    return;
  }

  container.innerHTML = `
    <div class="tree-container">
      <ul class="tree-list">
        ${state.equipmentsTree.map(renderEquipmentTreeNode).join('')}
      </ul>
    </div>
  `;
}

function renderEquipmentTreeNode(node) {
  const hasChildren = node.children && node.children.length > 0;
  const isExpanded = state.equipmentsTreeExpanded.has(node.id);
  const canEdit = hasPermission('equipments.edit');
  const canCreate = hasPermission('equipments.create');
  const canDel = hasPermission('equipments.delete');

  const toggleBtn = hasChildren ? `
    <button type="button" class="tree-toggle" data-action="toggle-equipment-node" data-args="${jsArg(node.id)}" aria-label="${isExpanded ? 'Replier' : 'Déplier'}">
      ${icon(isExpanded ? 'i-chevron-down' : 'i-chevron-right')}
    </button>
  ` : `<span class="tree-toggle-spacer"></span>`;

  const actions = [
    `<button type="button" class="btn sm" title="Fiche détaillée" data-action="open-detail-equipment" data-args="${jsArg(node.id)}">
      ${icon('i-info')}
    </button>`
  ];

  if (canCreate) {
    actions.push(`
      <button type="button" class="btn sm" title="Ajouter un sous-équipement" data-action="open-create-equipment" data-args="${jsArg({ parentId: node.id })}">
        ${icon('i-plus')}
      </button>
    `);
  }

  if (canEdit) {
    actions.push(`
      <button type="button" class="btn sm" title="Modifier" data-action="open-edit-equipment" data-args="${jsArg(node.id)}">
        ${icon('i-edit')}
      </button>
    `);
  }

  if (canDel) {
    actions.push(`
      <button type="button" class="btn sm danger" title="Supprimer" data-action="delete-equipment" data-args="${jsArg(node.id)}">
        ${icon('i-trash')}
      </button>
    `);
  }

  return `
    <li class="tree-node">
      <div class="tree-row">
        ${toggleBtn}
        <span class="tree-icon">${icon('i-gear')}</span>
        <div class="tree-label">
          <button type="button" class="link-action mono" data-action="open-detail-equipment" data-args="${jsArg(node.id)}">${esc(node.code)}</button>
          <span><b>${esc(node.name)}</b></span>
          ${statusBadge(node.status)}
        </div>
        <div class="tree-actions">${actions.join('')}</div>
      </div>
      ${hasChildren ? `
        <ul class="tree-children" ${isExpanded ? '' : 'hidden'}>
          ${node.children.map(renderEquipmentTreeNode).join('')}
        </ul>
      ` : ''}
    </li>
  `;
}

function toggleEquipmentTreeNode(id) {
  if (state.equipmentsTreeExpanded.has(id)) {
    state.equipmentsTreeExpanded.delete(id);
  } else {
    state.equipmentsTreeExpanded.add(id);
  }
  renderEquipmentsTreeView();
}

async function openEquipmentDetailModal(equipmentId) {
  openModal('Fiche équipement', `<p class="hint">${icon('i-clock')} Chargement du détail…</p>`);

  try {
    const eq = await api(`/api/equipments/${encodeURIComponent(equipmentId)}`);
    if (!eq) throw new Error('Équipement introuvable.');

    let subItems = [];
    try {
      const subRes = await api(`/api/equipments?parent_id=${encodeURIComponent(equipmentId)}&limit=100`);
      subItems = subRes && Array.isArray(subRes.items) ? subRes.items : [];
    } catch (_) {}

    const canEdit = hasPermission('equipments.edit');
    const lineLabel = eq.production_line ? `${eq.production_line.code} — ${eq.production_line.name}` : 'Aucune ligne rattachée';
    const parentLabel = eq.parent ? `${eq.parent.code} — ${eq.parent.name}` : 'Équipement racine (aucun parent)';

    const bodyHtml = `
      <div class="detail-grid">
        <div class="detail-card">
          <h3>${icon('i-gear')} Informations générales</h3>
          <dl class="detail-dl">
            <dt>Code</dt><dd class="mono"><b>${esc(eq.code)}</b></dd>
            <dt>Nom</dt><dd><b>${esc(eq.name)}</b></dd>
            <dt>Statut</dt><dd>${statusBadge(eq.status)}</dd>
            <dt>Ligne</dt><dd>${esc(lineLabel)}</dd>
            <dt>Parent</dt><dd>${esc(parentLabel)}</dd>
            <dt>Description</dt><dd>${esc(eq.description || '—')}</dd>
          </dl>
        </div>
        <div class="detail-card">
          <h3>${icon('i-wrench')} Données constructeur</h3>
          <dl class="detail-dl">
            <dt>Fabricant</dt><dd>${esc(eq.manufacturer || '—')}</dd>
            <dt>Modèle</dt><dd>${esc(eq.model || '—')}</dd>
            <dt>N° de série</dt><dd class="mono">${esc(eq.serial_number || '—')}</dd>
            <dt>Mise en service</dt><dd>${eq.commissioned_on ? esc(eq.commissioned_on) : '—'}</dd>
            <dt>Date création</dt><dd>${eq.created_at ? esc(formatDate(eq.created_at)) : '—'}</dd>
            <dt>Mise à jour</dt><dd>${eq.updated_at ? esc(formatDate(eq.updated_at)) : '—'}</dd>
          </dl>
        </div>
      </div>
      <div class="detail-card detail-card-spaced">
        <h3>${icon('i-gear')} Sous-équipements (${subItems.length})</h3>
        ${subItems.length ? `
          <div class="sub-equipments-list">
            ${subItems.map(sub => `
              <div class="sub-equipment-item" data-action="open-detail-equipment" data-args="${jsArg(sub.id)}">
                <span class="mono"><b>${esc(sub.code)}</b> — ${esc(sub.name)}</span>
                ${statusBadge(sub.status)}
              </div>
            `).join('')}
          </div>
        ` : '<p class="hint">Cet équipement ne comporte aucun sous-équipement.</p>'}
      </div>
    `;

    const footerHtml = `
      <button type="button" class="btn" data-action="close-modal">Fermer</button>
      ${canEdit ? `
        <button type="button" class="btn primary" data-action="open-edit-equipment" data-args="${jsArg(eq.id)}">
          ${icon('i-edit')} <span>Modifier</span>
        </button>
      ` : ''}
    `;

    openModal(`Équipement « ${eq.code} »`, bodyHtml, footerHtml);
  } catch (err) {
    openModal('Erreur', `<p class="hint state-ko">${esc(err.message)}</p>`, `<button type="button" class="btn" data-action="close-modal">Fermer</button>`);
  }
}

async function openCreateEquipmentModal(preselected = {}) {
  const preParentId = preselected && preselected.parentId ? preselected.parentId : null;
  const currentSiteId = state.selectedSiteId || (state.site ? state.site.id : '');

  let lines = [];
  let candidateParents = [];
  try {
    const siteParam = currentSiteId ? `?site_id=${encodeURIComponent(currentSiteId)}&limit=100` : '?limit=100';
    const [lRes, eRes] = await Promise.all([
      api(`/api/production-lines${siteParam}`),
      api(`/api/equipments${siteParam}`)
    ]);
    lines = lRes && Array.isArray(lRes.items) ? lRes.items : [];
    candidateParents = eRes && Array.isArray(eRes.items) ? eRes.items : [];
  } catch (_) {}

  const fieldsHtml = `
    ${renderSiteFormField(currentSiteId)}
    <div class="form-row">
      <div class="form-group">
        <label for="eqCode">Code équipement *</label>
        <input type="text" id="eqCode" name="code" required autocomplete="off" autofocus placeholder="ex. EXT-101" />
        <span class="form-help">Code unique par site.</span>
      </div>
      <div class="form-group">
        <label for="eqName">Nom de l'équipement *</label>
        <input type="text" id="eqName" name="name" required autocomplete="off" placeholder="ex. Extrudeuse principale" />
      </div>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="eqStatus">Statut opérationnel *</label>
        <select id="eqStatus" name="status" required>
          <option value="operational" selected>En service</option>
          <option value="degraded">Dégradé</option>
          <option value="out_of_service">Hors service</option>
        </select>
      </div>
      <div class="form-group">
        <label for="eqLine">Ligne de production</label>
        <select id="eqLine" name="production_line_id">
          <option value="">-- Aucune ligne --</option>
          ${lines.map(l => `<option value="${esc(l.id)}">${esc(l.code)} — ${esc(l.name)}</option>`).join('')}
        </select>
      </div>
    </div>
    <div class="form-group">
      <label for="eqParent">Équipement parent (hiérarchie)</label>
      <select id="eqParent" name="parent_id">
        <option value="">-- Racine (aucun parent) --</option>
        ${candidateParents.map(p => `<option value="${esc(p.id)}" ${p.id === preParentId ? 'selected' : ''}>${esc(p.code)} — ${esc(p.name)}</option>`).join('')}
      </select>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="eqMfr">Fabricant</label>
        <input type="text" id="eqMfr" name="manufacturer" autocomplete="off" placeholder="ex. Siemens, Schneider…" />
      </div>
      <div class="form-group">
        <label for="eqModel">Modèle</label>
        <input type="text" id="eqModel" name="model" autocomplete="off" />
      </div>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="eqSn">Numéro de série</label>
        <input type="text" id="eqSn" name="serial_number" autocomplete="off" />
      </div>
      <div class="form-group">
        <label for="eqComm">Mise en service</label>
        <input type="date" id="eqComm" name="commissioned_on" />
      </div>
    </div>
    <div class="form-group">
      <label for="eqDesc">Description</label>
      <textarea id="eqDesc" name="description" rows="2" placeholder="Description technique ou notes…"></textarea>
    </div>
  `;

  openFormModal({
    title: 'Nouvel équipement',
    formId: 'modalEquipmentForm',
    submitAction: 'create-equipment-submit',
    submitLabel: "Créer l'équipement",
    fieldsHtml,
  });
}

async function handleCreateEquipmentSubmit(form) {
  const btn = document.getElementById('modalEquipmentFormSubmitBtn');
  const errBox = document.getElementById('modalEquipmentFormErrorBox');
  const errText = document.getElementById('modalEquipmentFormErrorBoxText');

  const site_id = form.site_id ? form.site_id.value : (state.site ? state.site.id : null);
  const code = (form.code.value || '').trim();
  const name = (form.name.value || '').trim();
  const status = form.status.value;
  const production_line_id = form.production_line_id.value || null;
  const parent_id = form.parent_id.value || null;
  const manufacturer = (form.manufacturer.value || '').trim() || null;
  const model = (form.model.value || '').trim() || null;
  const serial_number = (form.serial_number.value || '').trim() || null;
  const commissioned_on = form.commissioned_on.value || null;
  const description = (form.description.value || '').trim() || null;

  btn.disabled = true;
  errBox.hidden = true;

  try {
    const payload = {
      code,
      name,
      status,
      production_line_id,
      parent_id,
      manufacturer,
      model,
      serial_number,
      commissioned_on,
      description,
    };

    if (isAdmin()) {
      if (!site_id) throw new Error('Veuillez sélectionner un site.');
      payload.site_id = site_id;
    }

    await api('/api/equipments', {
      method: 'POST',
      body: payload,
    });

    notify(`Équipement « ${code} » créé avec succès.`, 'success');
    closeModal();
    if (state.equipmentsViewMode === 'tree') {
      await loadEquipmentsTree();
    } else {
      await loadEquipmentsList(true);
    }
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || "Impossible de créer l'équipement.";
  }
}

async function openEditEquipmentModal(equipmentId) {
  openModal("Modifier l'équipement", `<p class="hint">${icon('i-clock')} Chargement…</p>`);

  try {
    const eq = await api(`/api/equipments/${encodeURIComponent(equipmentId)}`);
    if (!eq) throw new Error('Équipement introuvable.');

    const currentSiteId = eq.site_id;
    const siteParam = currentSiteId ? `?site_id=${encodeURIComponent(currentSiteId)}&limit=100` : '?limit=100';
    const [lRes, eRes] = await Promise.all([
      api(`/api/production-lines${siteParam}`),
      api(`/api/equipments${siteParam}`)
    ]);
    const lines = lRes && Array.isArray(lRes.items) ? lRes.items : [];
    const candidateParents = (eRes && Array.isArray(eRes.items) ? eRes.items : []).filter(e => e.id !== eq.id);

    const fieldsHtml = `
      <div class="form-row">
        <div class="form-group">
          <label for="eqCode">Code équipement *</label>
          <input type="text" id="eqCode" name="code" value="${esc(eq.code)}" required autocomplete="off" autofocus />
        </div>
        <div class="form-group">
          <label for="eqName">Nom de l'équipement *</label>
          <input type="text" id="eqName" name="name" value="${esc(eq.name)}" required autocomplete="off" />
        </div>
      </div>
      <div class="form-row">
        <div class="form-group">
          <label for="eqStatus">Statut opérationnel *</label>
          <select id="eqStatus" name="status" required>
            <option value="operational" ${eq.status === 'operational' ? 'selected' : ''}>En service</option>
            <option value="degraded" ${eq.status === 'degraded' ? 'selected' : ''}>Dégradé</option>
            <option value="out_of_service" ${eq.status === 'out_of_service' ? 'selected' : ''}>Hors service</option>
          </select>
        </div>
        <div class="form-group">
          <label for="eqLine">Ligne de production</label>
          <select id="eqLine" name="production_line_id">
            <option value="">-- Aucune ligne --</option>
            ${lines.map(l => `<option value="${esc(l.id)}" ${(eq.production_line && eq.production_line.id === l.id) ? 'selected' : ''}>${esc(l.code)} — ${esc(l.name)}</option>`).join('')}
          </select>
        </div>
      </div>
      <div class="form-group">
        <label for="eqParent">Équipement parent (hiérarchie)</label>
        <select id="eqParent" name="parent_id">
          <option value="">-- Racine (aucun parent) --</option>
          ${candidateParents.map(p => `<option value="${esc(p.id)}" ${(eq.parent && eq.parent.id === p.id) ? 'selected' : ''}>${esc(p.code)} — ${esc(p.name)}</option>`).join('')}
        </select>
      </div>
      <div class="form-row">
        <div class="form-group">
          <label for="eqMfr">Fabricant</label>
          <input type="text" id="eqMfr" name="manufacturer" value="${esc(eq.manufacturer || '')}" autocomplete="off" />
        </div>
        <div class="form-group">
          <label for="eqModel">Modèle</label>
          <input type="text" id="eqModel" name="model" value="${esc(eq.model || '')}" autocomplete="off" />
        </div>
      </div>
      <div class="form-row">
        <div class="form-group">
          <label for="eqSn">Numéro de série</label>
          <input type="text" id="eqSn" name="serial_number" value="${esc(eq.serial_number || '')}" autocomplete="off" />
        </div>
        <div class="form-group">
          <label for="eqComm">Mise en service</label>
          <input type="date" id="eqComm" name="commissioned_on" value="${esc(eq.commissioned_on || '')}" />
        </div>
      </div>
      <div class="form-group">
        <label for="eqDesc">Description</label>
        <textarea id="eqDesc" name="description" rows="2">${esc(eq.description || '')}</textarea>
      </div>
    `;

    openFormModal({
      title: `Modifier l'équipement ${eq.code}`,
      formId: 'modalEquipmentForm',
      submitAction: 'edit-equipment-submit',
      submitArgs: eq.id,
      submitLabel: 'Enregistrer les modifications',
      fieldsHtml,
    });
  } catch (err) {
    openModal('Erreur', `<p class="hint state-ko">${esc(err.message)}</p>`, `<button type="button" class="btn" data-action="close-modal">Fermer</button>`);
  }
}

async function handleEditEquipmentSubmit(form) {
  const equipmentId = form.dataset.args ? JSON.parse(form.dataset.args) : null;
  if (!equipmentId) return;

  const btn = document.getElementById('modalEquipmentFormSubmitBtn');
  const errBox = document.getElementById('modalEquipmentFormErrorBox');
  const errText = document.getElementById('modalEquipmentFormErrorBoxText');

  const code = (form.code.value || '').trim();
  const name = (form.name.value || '').trim();
  const status = form.status.value;
  const production_line_id = form.production_line_id.value || null;
  const parent_id = form.parent_id.value || null;
  const manufacturer = (form.manufacturer.value || '').trim() || null;
  const model = (form.model.value || '').trim() || null;
  const serial_number = (form.serial_number.value || '').trim() || null;
  const commissioned_on = form.commissioned_on.value || null;
  const description = (form.description.value || '').trim() || null;

  btn.disabled = true;
  errBox.hidden = true;

  try {
    await api(`/api/equipments/${encodeURIComponent(equipmentId)}`, {
      method: 'PATCH',
      body: {
        code,
        name,
        status,
        production_line_id,
        parent_id,
        manufacturer,
        model,
        serial_number,
        commissioned_on,
        description,
      },
    });

    notify('Équipement mis à jour avec succès.', 'success');
    closeModal();
    if (state.equipmentsViewMode === 'tree') {
      await loadEquipmentsTree();
    } else {
      await loadEquipmentsList(true);
    }
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || "Impossible de modifier l'équipement.";
  }
}

async function deleteEquipment(equipmentId) {
  const eq = state.equipments.find(e => e.id === equipmentId);
  const label = eq ? `« ${eq.name} »` : 'cet équipement';

  const ok = await confirmDialog(
    "Supprimer l'équipement",
    `Voulez-vous vraiment supprimer ${label} ?\nCette opération sera refusée si des sous-équipements y sont rattachés.`,
    'Supprimer',
    true
  );
  if (!ok) return;

  try {
    await api(`/api/equipments/${encodeURIComponent(equipmentId)}`, {
      method: 'DELETE',
    });
    notify('Équipement supprimé.', 'success');
    if (state.equipmentsViewMode === 'tree') {
      await loadEquipmentsTree();
    } else {
      await loadEquipmentsList(true);
    }
  } catch (err) {
    notify(err.message || 'Suppression refusée.', 'error');
  }
}

// ============================================================================
// 14D. PAGE EMPLACEMENTS (PHASE 2)
// ============================================================================

function locationsPage() {
  const canManage = hasPermission('locations.manage');

  const extraFilters = `
    <button type="button" class="btn sm" data-action="expand-all-locations">
      ${icon('i-chevron-down')} <span>Tout déplier</span>
    </button>
    <button type="button" class="btn sm" data-action="collapse-all-locations">
      ${icon('i-chevron-right')} <span>Tout replier</span>
    </button>
  `;

  return `
    <div class="page-head">${icon('i-pin')}<h1>Emplacements</h1></div>
    ${renderToolbar({
      primaryAction: 'open-create-location',
      primaryLabel: 'Nouveau magasin',
      primaryIcon: 'i-plus',
      canCreate: canManage,
      extraFiltersHtml: extraFilters,
    })}
    <div id="locationsContainer">
      <p class="hint">${icon('i-clock')} Chargement des emplacements…</p>
    </div>
  `;
}

async function initLocationsPageView() {
  await loadLocationsTree();
}

async function loadLocationsTree() {
  const container = document.getElementById('locationsContainer');
  if (!container) return;

  state.locationsTreeLoading = true;
  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement de l'arborescence des magasins…</p>`;

  try {
    const siteParam = state.selectedSiteId ? `?site_id=${encodeURIComponent(state.selectedSiteId)}` : '';
    const tree = await api(`/api/locations/tree${siteParam}`);
    state.locationsTree = Array.isArray(tree) ? tree : [];

    if (state.locationsTreeExpanded.size === 0 && state.locationsTree.length > 0) {
      for (const n of state.locationsTree) {
        state.locationsTreeExpanded.add(n.id);
      }
    }
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">Erreur lors du chargement des emplacements : ${esc(err.message)}</p>`;
    state.locationsTreeLoading = false;
    return;
  }

  state.locationsTreeLoading = false;
  renderLocationsTreeView();
}

function renderLocationsTreeView() {
  const container = document.getElementById('locationsContainer');
  if (!container) return;

  if (!state.locationsTree.length) {
    container.innerHTML = `
      <div class="tree-container">
        <p class="hint">Aucun emplacement enregistré. Cliquez sur « Nouveau magasin » pour démarrer l'arborescence.</p>
      </div>
    `;
    return;
  }

  container.innerHTML = `
    <div class="tree-container">
      <ul class="tree-list">
        ${state.locationsTree.map(renderLocationTreeNode).join('')}
      </ul>
    </div>
  `;
}

function renderLocationTreeNode(node) {
  const hasChildren = node.children && node.children.length > 0;
  const isExpanded = state.locationsTreeExpanded.has(node.id);
  const canManage = hasPermission('locations.manage');
  const nextKind = nextLocationKind(node.kind);

  const toggleBtn = hasChildren ? `
    <button type="button" class="tree-toggle" data-action="toggle-location-node" data-args="${jsArg(node.id)}" aria-label="${isExpanded ? 'Replier' : 'Déplier'}">
      ${icon(isExpanded ? 'i-chevron-down' : 'i-chevron-right')}
    </button>
  ` : `<span class="tree-toggle-spacer"></span>`;

  const actions = [];
  if (canManage) {
    if (nextKind) {
      const childKindLabel = locationKindLabel(nextKind);
      actions.push(`
        <button type="button" class="btn sm" title="Ajouter un sous-emplacement (${childKindLabel})" data-action="open-create-location" data-args="${jsArg({ parentId: node.id, parentKind: node.kind, siteId: node.site_id, parentName: node.name })}">
          ${icon('i-plus')} <span>${esc(childKindLabel)}</span>
        </button>
      `);
    }
    actions.push(`
      <button type="button" class="btn sm" title="Renommer l'emplacement" data-action="open-edit-location" data-args="${jsArg(node.id)}">
        ${icon('i-edit')} <span>Renommer</span>
      </button>
    `);
    actions.push(`
      <button type="button" class="btn sm danger" title="Supprimer l'emplacement" data-action="delete-location" data-args="${jsArg(node.id)}">
        ${icon('i-trash')}
      </button>
    `);
  }

  return `
    <li class="tree-node">
      <div class="tree-row">
        ${toggleBtn}
        <span class="tree-icon">${icon('i-pin')}</span>
        <div class="tree-label">
          ${locationKindBadge(node.kind)}
          <span class="mono"><b>${esc(node.code)}</b></span>
          <span>${esc(node.name)}</span>
        </div>
        <div class="tree-actions">${actions.join('')}</div>
      </div>
      ${hasChildren ? `
        <ul class="tree-children" ${isExpanded ? '' : 'hidden'}>
          ${node.children.map(renderLocationTreeNode).join('')}
        </ul>
      ` : ''}
    </li>
  `;
}

function toggleLocationTreeNode(id) {
  if (state.locationsTreeExpanded.has(id)) {
    state.locationsTreeExpanded.delete(id);
  } else {
    state.locationsTreeExpanded.add(id);
  }
  renderLocationsTreeView();
}

function expandAllLocations(expand = true) {
  function collectIds(list) {
    for (const n of list) {
      if (expand) state.locationsTreeExpanded.add(n.id);
      else state.locationsTreeExpanded.delete(n.id);
      if (n.children && n.children.length) collectIds(n.children);
    }
  }
  collectIds(state.locationsTree);
  renderLocationsTreeView();
}

function findLocationInTree(list, id) {
  for (const n of list) {
    if (n.id === id) return n;
    if (n.children && n.children.length) {
      const found = findLocationInTree(n.children, id);
      if (found) return found;
    }
  }
  return null;
}

function openCreateLocationModal(opts = {}) {
  const parentId = opts && opts.parentId ? opts.parentId : null;
  const parentKind = opts && opts.parentKind ? opts.parentKind : null;
  const parentName = opts && opts.parentName ? opts.parentName : '';
  const siteId = opts && opts.siteId ? opts.siteId : (state.selectedSiteId || (state.site ? state.site.id : ''));
  const deducedKind = parentKind ? nextLocationKind(parentKind) : 'magasin';

  let title = 'Nouveau magasin';
  let contextInfoHtml = '';

  if (parentId) {
    title = `Nouveau ${locationKindLabel(deducedKind).toLowerCase()} sous « ${parentName} »`;
    contextInfoHtml = `
      <div class="hint hint-spaced">
        Type d'emplacement déduit : <b>${esc(locationKindLabel(deducedKind))}</b>
      </div>
    `;
  }

  const fieldsHtml = `
    ${!parentId ? renderSiteFormField(siteId) : `<input type="hidden" name="site_id" value="${esc(siteId)}" />`}
    <input type="hidden" name="parent_id" value="${esc(parentId || '')}" />
    <input type="hidden" name="kind" value="${esc(deducedKind)}" />
    ${contextInfoHtml}
    <div class="form-row">
      <div class="form-group">
        <label for="locCode">Code *</label>
        <input type="text" id="locCode" name="code" required autocomplete="off" autofocus placeholder="ex. ${deducedKind === 'magasin' ? 'MAG-CENTRAL' : deducedKind === 'allee' ? 'ALL-A' : deducedKind === 'rack' ? 'RCK-A1' : 'BAC-01'}" />
      </div>
      <div class="form-group">
        <label for="locName">Nom de l'emplacement *</label>
        <input type="text" id="locName" name="name" required autocomplete="off" placeholder="ex. ${locationKindLabel(deducedKind)} principal" />
      </div>
    </div>
  `;

  openFormModal({
    title,
    formId: 'modalLocationForm',
    submitAction: 'create-location-submit',
    submitLabel: "Créer l'emplacement",
    fieldsHtml,
  });
}

async function handleCreateLocationSubmit(form) {
  const btn = document.getElementById('modalLocationFormSubmitBtn');
  const errBox = document.getElementById('modalLocationFormErrorBox');
  const errText = document.getElementById('modalLocationFormErrorBoxText');

  const site_id = form.site_id ? form.site_id.value : (state.site ? state.site.id : null);
  const parent_id = form.parent_id.value || null;
  const kind = form.kind.value;
  const code = (form.code.value || '').trim();
  const name = (form.name.value || '').trim();

  btn.disabled = true;
  errBox.hidden = true;

  try {
    const payload = {
      code,
      name,
      kind,
      parent_id,
    };

    if (isAdmin()) {
      if (!site_id) throw new Error('Veuillez sélectionner un site.');
      payload.site_id = site_id;
    }

    await api('/api/locations', {
      method: 'POST',
      body: payload,
    });

    if (parent_id) state.locationsTreeExpanded.add(parent_id);
    notify(`Emplacement « ${code} » créé avec succès.`, 'success');
    closeModal();
    await loadLocationsTree();
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || "Impossible de créer l'emplacement.";
  }
}

function openEditLocationModal(locationId) {
  const loc = findLocationInTree(state.locationsTree, locationId);
  if (!loc) return;

  const fieldsHtml = `
    <div class="form-row">
      <div class="form-group">
        <label for="locCode">Code *</label>
        <input type="text" id="locCode" name="code" value="${esc(loc.code)}" required autocomplete="off" autofocus />
      </div>
      <div class="form-group">
        <label for="locName">Nom *</label>
        <input type="text" id="locName" name="name" value="${esc(loc.name)}" required autocomplete="off" />
      </div>
    </div>
  `;

  openFormModal({
    title: `Renommer l'emplacement ${loc.code}`,
    formId: 'modalLocationForm',
    submitAction: 'edit-location-submit',
    submitArgs: loc.id,
    submitLabel: 'Enregistrer',
    fieldsHtml,
  });
}

async function handleEditLocationSubmit(form) {
  const locationId = form.dataset.args ? JSON.parse(form.dataset.args) : null;
  if (!locationId) return;

  const btn = document.getElementById('modalLocationFormSubmitBtn');
  const errBox = document.getElementById('modalLocationFormErrorBox');
  const errText = document.getElementById('modalLocationFormErrorBoxText');

  const code = (form.code.value || '').trim();
  const name = (form.name.value || '').trim();

  btn.disabled = true;
  errBox.hidden = true;

  try {
    await api(`/api/locations/${encodeURIComponent(locationId)}`, {
      method: 'PATCH',
      body: { code, name },
    });

    notify('Emplacement mis à jour.', 'success');
    closeModal();
    await loadLocationsTree();
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || "Impossible de modifier l'emplacement.";
  }
}

async function deleteLocation(locationId) {
  const loc = findLocationInTree(state.locationsTree, locationId);
  const label = loc ? `« ${loc.name} »` : 'cet emplacement';

  const ok = await confirmDialog(
    "Supprimer l'emplacement",
    `Voulez-vous vraiment supprimer ${label} ?\nCette opération sera refusée si des sous-emplacements y sont rattachés.`,
    'Supprimer',
    true
  );
  if (!ok) return;

  try {
    await api(`/api/locations/${encodeURIComponent(locationId)}`, {
      method: 'DELETE',
    });
    notify('Emplacement supprimé.', 'success');
    await loadLocationsTree();
  } catch (err) {
    notify(err.message || 'Suppression refusée.', 'error');
  }
}

// ============================================================================
// 14E. PAGE FOURNISSEURS (PHASE 2)
// ============================================================================

function suppliersPage() {
  const canManage = hasPermission('suppliers.manage');

  return `
    <div class="page-head">${icon('i-truck')}<h1>Fournisseurs</h1></div>
    ${renderToolbar({
      searchAction: 'filter-suppliers-search',
      searchValue: state.suppliersSearchQuery,
      searchPlaceholder: 'Rechercher par nom…',
      primaryAction: 'open-create-supplier',
      primaryLabel: 'Nouveau fournisseur',
      primaryIcon: 'i-plus',
      canCreate: canManage,
    })}
    <div id="suppliersContainer">
      <p class="hint">${icon('i-clock')} Chargement des fournisseurs…</p>
    </div>
  `;
}

async function initSuppliersPageView() {
  await loadSuppliers(true);
}

async function loadSuppliers(reset = true) {
  const container = document.getElementById('suppliersContainer');
  if (!container && reset) return;

  if (reset) {
    state.suppliers = [];
    state.suppliersNextCursor = null;
  }

  state.suppliersLoading = true;
  if (!reset) renderSuppliersTable();

  try {
    const params = new URLSearchParams();
    params.set('limit', '50');
    if (state.suppliersSearchQuery) params.set('q', state.suppliersSearchQuery.trim());
    if (state.selectedSiteId) params.set('site_id', state.selectedSiteId);
    if (!reset && state.suppliersNextCursor) params.set('cursor', state.suppliersNextCursor);

    const res = await api(`/api/suppliers?${params.toString()}`);
    const items = res && Array.isArray(res.items) ? res.items : [];
    const nextCursor = res ? res.next_cursor : null;

    if (reset) {
      state.suppliers = items;
    } else {
      state.suppliers = state.suppliers.concat(items);
    }
    state.suppliersNextCursor = nextCursor;
  } catch (err) {
    if (container) {
      container.innerHTML = `<p class="hint state-ko">Erreur lors du chargement des fournisseurs : ${esc(err.message)}</p>`;
    }
    state.suppliersLoading = false;
    return;
  }

  state.suppliersLoading = false;
  renderSuppliersTable();
}

function renderSuppliersTable() {
  const container = document.getElementById('suppliersContainer');
  if (!container) return;

  const items = state.suppliers;
  const canManage = hasPermission('suppliers.manage');
  const isAdm = isAdmin();
  const showSiteCol = isAdm && !state.selectedSiteId;
  const sitesMap = new Map(state.sites.map(s => [s.id, s.name]));

  if (!items.length && !state.suppliersLoading) {
    container.innerHTML = `
      <div class="table-container">
        <table class="data-table">
          <tbody>
            <tr><td class="empty-cell">Aucun fournisseur trouvé.</td></tr>
          </tbody>
        </table>
      </div>
    `;
    return;
  }

  const rows = items.map(s => {
    const siteName = s.site_id ? (sitesMap.get(s.site_id) || 'Site inconnu') : '—';
    const emailLink = s.email ? `<a href="mailto:${esc(s.email)}" class="link-action">${esc(s.email)}</a>` : '—';
    const phoneLink = s.phone ? `<a href="tel:${esc(s.phone)}" class="link-action">${esc(s.phone)}</a>` : '—';

    const actionButtons = [];
    if (canManage) {
      actionButtons.push(`
        <button type="button" class="btn sm" title="Modifier le fournisseur" data-action="open-edit-supplier" data-args="${jsArg(s.id)}">
          ${icon('i-edit')} <span>Modifier</span>
        </button>
      `);
      actionButtons.push(`
        <button type="button" class="btn sm danger" title="Supprimer le fournisseur" data-action="delete-supplier" data-args="${jsArg(s.id)}">
          ${icon('i-trash')}
        </button>
      `);
    }

    return `
      <tr>
        <td><b>${esc(s.name)}</b></td>
        <td>${esc(s.contact_name || '—')}</td>
        <td>${emailLink}</td>
        <td>${phoneLink}</td>
        <td>${esc(s.address || '—')}</td>
        ${showSiteCol ? `<td>${esc(siteName)}</td>` : ''}
        <td><div class="cell-actions">${actionButtons.join('') || '<span class="hint">—</span>'}</div></td>
      </tr>
    `;
  }).join('');

  container.innerHTML = `
    <div class="table-container">
      <table class="data-table">
        <thead>
          <tr>
            <th>Nom du fournisseur</th>
            <th>Contact</th>
            <th>Email</th>
            <th>Téléphone</th>
            <th>Adresse</th>
            ${showSiteCol ? '<th>Site</th>' : ''}
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          ${rows}
        </tbody>
      </table>
    </div>
    ${renderLoadMore(state.suppliersNextCursor, 'load-more-suppliers', state.suppliersLoading)}
  `;
}

function openCreateSupplierModal() {
  const currentSiteId = state.selectedSiteId || (state.site ? state.site.id : '');

  const fieldsHtml = `
    ${renderSiteFormField(currentSiteId)}
    <div class="form-row">
      <div class="form-group">
        <label for="supName">Nom du fournisseur *</label>
        <input type="text" id="supName" name="name" required autocomplete="off" autofocus placeholder="ex. SKF France" />
      </div>
      <div class="form-group">
        <label for="supContact">Nom du contact</label>
        <input type="text" id="supContact" name="contact_name" autocomplete="off" placeholder="ex. Jean Dupont" />
      </div>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="supEmail">Email</label>
        <input type="email" id="supEmail" name="email" autocomplete="off" placeholder="ex. contact@fournisseur.fr" />
      </div>
      <div class="form-group">
        <label for="supPhone">Téléphone</label>
        <input type="tel" id="supPhone" name="phone" autocomplete="off" placeholder="ex. 01 23 45 67 89" />
      </div>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="supAddress">Adresse</label>
        <input type="text" id="supAddress" name="address" autocomplete="off" placeholder="ex. 12 rue de l'Industrie, 69000 Lyon" />
      </div>
      <div class="form-group">
        <label for="supWeb">Site internet</label>
        <input type="url" id="supWeb" name="website" autocomplete="off" placeholder="ex. https://www.skf.com" />
      </div>
    </div>
    <div class="form-group">
      <label for="supNotes">Notes</label>
      <textarea id="supNotes" name="notes" rows="2" placeholder="Conditions commerciales, délais habituels…"></textarea>
    </div>
  `;

  openFormModal({
    title: 'Nouveau fournisseur',
    formId: 'modalSupplierForm',
    submitAction: 'create-supplier-submit',
    submitLabel: 'Créer le fournisseur',
    fieldsHtml,
  });
}

async function handleCreateSupplierSubmit(form) {
  const btn = document.getElementById('modalSupplierFormSubmitBtn');
  const errBox = document.getElementById('modalSupplierFormErrorBox');
  const errText = document.getElementById('modalSupplierFormErrorBoxText');

  const site_id = form.site_id ? form.site_id.value : (state.site ? state.site.id : null);
  const name = (form.name.value || '').trim();
  const contact_name = (form.contact_name.value || '').trim() || null;
  const email = (form.email.value || '').trim() || null;
  const phone = (form.phone.value || '').trim() || null;
  const address = (form.address.value || '').trim() || null;
  const website = (form.website.value || '').trim() || null;
  const notes = (form.notes.value || '').trim() || null;

  btn.disabled = true;
  errBox.hidden = true;

  try {
    const payload = {
      name,
      contact_name,
      email,
      phone,
      address,
      website,
      notes,
    };

    if (isAdmin()) {
      if (!site_id) throw new Error('Veuillez sélectionner un site.');
      payload.site_id = site_id;
    }

    await api('/api/suppliers', {
      method: 'POST',
      body: payload,
    });

    notify(`Fournisseur « ${name} » créé avec succès.`, 'success');
    closeModal();
    await loadSuppliers(true);
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || 'Impossible de créer le fournisseur.';
  }
}

function openEditSupplierModal(supplierId) {
  const s = state.suppliers.find(x => x.id === supplierId);
  if (!s) return;

  const fieldsHtml = `
    <div class="form-row">
      <div class="form-group">
        <label for="supName">Nom du fournisseur *</label>
        <input type="text" id="supName" name="name" value="${esc(s.name)}" required autocomplete="off" autofocus />
      </div>
      <div class="form-group">
        <label for="supContact">Nom du contact</label>
        <input type="text" id="supContact" name="contact_name" value="${esc(s.contact_name || '')}" autocomplete="off" />
      </div>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="supEmail">Email</label>
        <input type="email" id="supEmail" name="email" value="${esc(s.email || '')}" autocomplete="off" />
      </div>
      <div class="form-group">
        <label for="supPhone">Téléphone</label>
        <input type="tel" id="supPhone" name="phone" value="${esc(s.phone || '')}" autocomplete="off" />
      </div>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="supAddress">Adresse</label>
        <input type="text" id="supAddress" name="address" value="${esc(s.address || '')}" autocomplete="off" />
      </div>
      <div class="form-group">
        <label for="supWeb">Site internet</label>
        <input type="url" id="supWeb" name="website" value="${esc(s.website || '')}" autocomplete="off" />
      </div>
    </div>
    <div class="form-group">
      <label for="supNotes">Notes</label>
      <textarea id="supNotes" name="notes" rows="2">${esc(s.notes || '')}</textarea>
    </div>
  `;

  openFormModal({
    title: `Modifier le fournisseur « ${s.name} »`,
    formId: 'modalSupplierForm',
    submitAction: 'edit-supplier-submit',
    submitArgs: s.id,
    submitLabel: 'Enregistrer',
    fieldsHtml,
  });
}

async function handleEditSupplierSubmit(form) {
  const supplierId = form.dataset.args ? JSON.parse(form.dataset.args) : null;
  if (!supplierId) return;

  const btn = document.getElementById('modalSupplierFormSubmitBtn');
  const errBox = document.getElementById('modalSupplierFormErrorBox');
  const errText = document.getElementById('modalSupplierFormErrorBoxText');

  const name = (form.name.value || '').trim();
  const contact_name = (form.contact_name.value || '').trim() || null;
  const email = (form.email.value || '').trim() || null;
  const phone = (form.phone.value || '').trim() || null;
  const address = (form.address.value || '').trim() || null;
  const website = (form.website.value || '').trim() || null;
  const notes = (form.notes.value || '').trim() || null;

  btn.disabled = true;
  errBox.hidden = true;

  try {
    await api(`/api/suppliers/${encodeURIComponent(supplierId)}`, {
      method: 'PATCH',
      body: {
        name,
        contact_name,
        email,
        phone,
        address,
        website,
        notes,
      },
    });

    notify('Fournisseur mis à jour avec succès.', 'success');
    closeModal();
    await loadSuppliers(true);
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || 'Impossible de modifier le fournisseur.';
  }
}

async function deleteSupplier(supplierId) {
  const s = state.suppliers.find(x => x.id === supplierId);
  const name = s ? `« ${s.name} »` : 'ce fournisseur';

  const ok = await confirmDialog(
    'Supprimer le fournisseur',
    `Voulez-vous vraiment supprimer ${name} ?`,
    'Supprimer',
    true
  );
  if (!ok) return;

  try {
    await api(`/api/suppliers/${encodeURIComponent(supplierId)}`, {
      method: 'DELETE',
    });
    notify('Fournisseur supprimé.', 'success');
    await loadSuppliers(true);
  } catch (err) {
    notify(err.message || 'Suppression refusée.', 'error');
  }
}

// ============================================================================
// 14E. MODULE INTERVENTIONS (PHASE 3)
// ============================================================================

function interventionsPage() {
  const canCreate = hasPermission('interventions.create');

  const quickTabsHtml = `
    <div class="filter-tabs" role="tablist" aria-label="Filtres rapides d'interventions">
      <button type="button" class="filter-tab ${state.interventionsQuickTab === 'all' ? 'active' : ''}" data-action="tab-interventions" data-args="${jsArg('all')}">
        Toutes
      </button>
      <button type="button" class="filter-tab ${state.interventionsQuickTab === 'active' ? 'active' : ''}" data-action="tab-interventions" data-args="${jsArg('active')}">
        ${icon('i-wrench')} <span>En cours / Ouvertes</span>
      </button>
      <button type="button" class="filter-tab ${state.interventionsQuickTab === 'mine' ? 'active' : ''}" data-action="tab-interventions" data-args="${jsArg('mine')}">
        ${icon('i-user')} <span>Mes interventions</span>
      </button>
      <button type="button" class="filter-tab filter-tab-arret ${state.interventionsQuickTab === 'arret' ? 'active' : ''}" data-action="tab-interventions" data-args="${jsArg('arret')}">
        ${icon('i-alert')} <span>Arrêt machine</span>
      </button>
      <button type="button" class="filter-tab filter-tab-crit ${state.interventionsQuickTab === 'critical' ? 'active' : ''}" data-action="tab-interventions" data-args="${jsArg('critical')}">
        ${icon('i-alert')} <span>Critiques</span>
      </button>
    </div>
  `;

  const extraFilters = `
    <select data-action="filter-interventions-type" aria-label="Filtrer par type">
      <option value="">Tous les types</option>
      <option value="corrective" ${state.interventionsTypeFilter === 'corrective' ? 'selected' : ''}>Corrective</option>
      <option value="preventive" ${state.interventionsTypeFilter === 'preventive' ? 'selected' : ''}>Préventive</option>
      <option value="a_posteriori" ${state.interventionsTypeFilter === 'a_posteriori' ? 'selected' : ''}>A posteriori</option>
      <option value="hors_equipement" ${state.interventionsTypeFilter === 'hors_equipement' ? 'selected' : ''}>Hors équipement</option>
    </select>
    <select data-action="filter-interventions-status" aria-label="Filtrer par statut">
      <option value="">Tous les statuts</option>
      <option value="open" ${state.interventionsStatusFilter === 'open' ? 'selected' : ''}>Ouverte</option>
      <option value="in_progress" ${state.interventionsStatusFilter === 'in_progress' ? 'selected' : ''}>En cours</option>
      <option value="completed" ${state.interventionsStatusFilter === 'completed' ? 'selected' : ''}>Clôturée</option>
      <option value="cancelled" ${state.interventionsStatusFilter === 'cancelled' ? 'selected' : ''}>Annulée</option>
    </select>
    <select data-action="filter-interventions-priority" aria-label="Filtrer par priorité">
      <option value="">Toutes les priorités</option>
      <option value="low" ${state.interventionsPriorityFilter === 'low' ? 'selected' : ''}>Basse</option>
      <option value="medium" ${state.interventionsPriorityFilter === 'medium' ? 'selected' : ''}>Moyenne</option>
      <option value="high" ${state.interventionsPriorityFilter === 'high' ? 'selected' : ''}>Haute</option>
      <option value="critical" ${state.interventionsPriorityFilter === 'critical' ? 'selected' : ''}>Critique</option>
    </select>
    <select id="interventionsEquipFilterSelect" data-action="filter-interventions-equipment" aria-label="Filtrer par équipement">
      <option value="">Tous les équipements</option>
      ${state.equipments.map(e => `<option value="${esc(e.id)}" ${state.interventionsEquipmentFilter === e.id ? 'selected' : ''}>${esc(e.code)} - ${esc(e.name)}</option>`).join('')}
    </select>
  `;

  return `
    <div class="page-head">
      ${icon('i-wrench')}
      <h1>Interventions de maintenance</h1>
    </div>
    ${quickTabsHtml}
    ${renderToolbar({
      searchAction: 'filter-interventions-search',
      searchPlaceholder: 'Rechercher par code, titre, description…',
      searchValue: state.interventionsSearchQuery,
      extraFiltersHtml: extraFilters,
      canCreate,
      primaryAction: 'open-create-intervention',
      primaryLabel: 'Nouvelle intervention',
      primaryIcon: 'i-plus',
    })}
    <div id="interventionsContainer">
      <p class="hint">${icon('i-clock')} Chargement des interventions…</p>
    </div>
  `;
}

async function initInterventionsPageView() {
  if (!state.equipments.length) {
    try {
      const siteParam = state.selectedSiteId ? `?site_id=${encodeURIComponent(state.selectedSiteId)}&limit=100` : '?limit=100';
      const eqRes = await api(`/api/equipments${siteParam}`);
      if (eqRes && Array.isArray(eqRes.items)) {
        state.equipments = eqRes.items;
        const sel = document.getElementById('interventionsEquipFilterSelect');
        if (sel) {
          sel.innerHTML = `<option value="">Tous les équipements</option>` +
            state.equipments.map(e => `<option value="${esc(e.id)}" ${state.interventionsEquipmentFilter === e.id ? 'selected' : ''}>${esc(e.code)} - ${esc(e.name)}</option>`).join('');
        }
      }
    } catch (_) {}
  }
  await loadInterventionsList(true);
}

async function loadInterventionsList(reset = true) {
  const container = document.getElementById('interventionsContainer');
  if (!container && reset) return;

  if (reset) {
    state.interventions = [];
    state.interventionsNextCursor = null;
  }

  state.interventionsLoading = true;
  if (!reset) renderInterventionsListView();

  try {
    const params = new URLSearchParams();
    params.set('limit', '50');
    if (state.interventionsSearchQuery) params.set('q', state.interventionsSearchQuery.trim());
    if (state.selectedSiteId) params.set('site_id', state.selectedSiteId);
    if (state.interventionsTypeFilter) params.set('type', state.interventionsTypeFilter);
    if (state.interventionsEquipmentFilter) params.set('equipment_id', state.interventionsEquipmentFilter);

    if (state.interventionsQuickTab === 'active') {
      params.set('status', 'active');
    } else if (state.interventionsStatusFilter) {
      params.set('status', state.interventionsStatusFilter);
    }

    if (state.interventionsQuickTab === 'critical') {
      params.set('priority', 'critical');
    } else if (state.interventionsPriorityFilter) {
      params.set('priority', state.interventionsPriorityFilter);
    }

    if (state.interventionsQuickTab === 'arret') {
      params.set('machine_state', 'ARRET');
    }

    if (state.interventionsQuickTab === 'mine' && state.currentUser) {
      params.set('assigned_to', state.currentUser.id);
    }

    if (!reset && state.interventionsNextCursor) {
      params.set('cursor', state.interventionsNextCursor);
    }

    const res = await api(`/api/interventions?${params.toString()}`);
    const items = res && Array.isArray(res.items) ? res.items : [];
    const nextCursor = res ? res.next_cursor : null;

    if (reset) {
      state.interventions = items;
    } else {
      state.interventions = state.interventions.concat(items);
    }
    state.interventionsNextCursor = nextCursor;
  } catch (err) {
    if (container) {
      container.innerHTML = `<p class="hint state-ko">Erreur lors du chargement des interventions : ${esc(err.message)}</p>`;
    }
    state.interventionsLoading = false;
    return;
  }

  state.interventionsLoading = false;
  renderInterventionsListView();
}

function renderInterventionsListView() {
  const container = document.getElementById('interventionsContainer');
  if (!container) return;

  const items = state.interventions;
  const canEdit = hasPermission('interventions.edit');

  if (!items.length && !state.interventionsLoading) {
    container.innerHTML = `
      <div class="table-container">
        <table class="data-table">
          <tbody>
            <tr><td class="empty-cell">Aucune intervention correspondant aux critères.</td></tr>
          </tbody>
        </table>
      </div>
    `;
    return;
  }

  const rows = items.map(i => {
    let equipHtml = '<span class="hint">Hors équipement</span>';
    if (i.equipment) {
      equipHtml = `
        <div>
          <button type="button" class="link-action" data-action="open-detail-equipment" data-args="${jsArg(i.equipment.id)}" title="Voir l'équipement">
            <b>${esc(i.equipment.code)}</b>
          </button>
          <span class="hint">— ${esc(i.equipment.name)}</span>
          ${i.equipment.status ? statusBadge(i.equipment.status) : ''}
        </div>
      `;
    }

    let techsHtml = '<span class="hint">—</span>';
    if (Array.isArray(i.technicians) && i.technicians.length > 0) {
      techsHtml = i.technicians.map(t => {
        const leadStar = t.is_lead ? ` <span class="badge-lead" title="Intervenant principal">Lead</span>` : '';
        return `<div><span class="mono">${esc(t.display_name)}</span>${leadStar}</div>`;
      }).join('');
    }

    const actions = [
      `<a href="#/interventions/${esc(i.id)}" class="btn sm" title="Ouvrir la fiche d'intervention">
        ${icon('i-info')} <span>Détail</span>
      </a>`
    ];

    if (canEdit && i.status === 'open') {
      actions.push(`
        <button type="button" class="btn sm primary" title="Prendre en charge cette intervention" data-action="take-intervention" data-args="${jsArg(i.id)}">
          ${icon('i-wrench')} <span>Prendre en charge</span>
        </button>
      `);
    }

    const subBadge = i.sub_interventions_count > 0
      ? `<span class="badge" title="${esc(i.sub_interventions_count)} sous-intervention(s) rattachée(s)">${esc(i.sub_interventions_count)} sous-tâche(s)</span>`
      : '';

    return `
      <tr>
        <td>
          <a href="#/interventions/${esc(i.id)}" class="link-action mono" title="Consulter la fiche">
            <b>${esc(i.code)}</b>
          </a>
        </td>
        <td>${formatDate(i.reported_at || i.created_at)}</td>
        <td>
          <a href="#/interventions/${esc(i.id)}" class="link-action" title="Consulter la fiche">
            <b>${esc(i.title)}</b>
          </a>
          <div class="hint-spaced">${equipHtml}</div>
          ${subBadge}
        </td>
        <td>${interventionTypeBadge(i.type)}</td>
        <td>${interventionPriorityBadge(i.priority)}</td>
        <td>${machineStateBadge(i.machine_state)}</td>
        <td>${techsHtml}</td>
        <td class="mono">${formatDurationMinutes(i.duration_minutes)}</td>
        <td>${interventionStatusBadge(i.status)}</td>
        <td><div class="cell-actions">${actions.join('')}</div></td>
      </tr>
    `;
  }).join('');

  container.innerHTML = `
    <div class="table-container">
      <table class="data-table">
        <thead>
          <tr>
            <th>Référence</th>
            <th>Date signalement</th>
            <th>Titre & Équipement</th>
            <th>Type</th>
            <th>Priorité</th>
            <th>État machine</th>
            <th>Intervenants</th>
            <th>Durée</th>
            <th>Statut</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          ${rows}
        </tbody>
      </table>
    </div>
    ${renderLoadMore(state.interventionsNextCursor, 'load-more-interventions', state.interventionsLoading)}
  `;
}

// --- FICHE DÉTAIL D'INTERVENTION ---

function interventionDetailPage(id) {
  return `
    <div class="intervention-detail">
      <div class="detail-nav-bar">
        <a href="#/interventions" class="btn" title="Retour à la liste des interventions">
          ${icon('i-arrow-left')} <span>Retour aux interventions</span>
        </a>
      </div>
      <div id="interventionDetailContainer">
        <p class="hint">${icon('i-clock')} Chargement des détails de l'intervention…</p>
      </div>
    </div>
  `;
}

async function initInterventionDetailPageView(id) {
  await loadInterventionDetail(id);
}

async function loadInterventionDetail(id) {
  const container = document.getElementById('interventionDetailContainer');
  if (!container) return;

  state.currentInterventionLoading = true;
  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement des détails de l'intervention…</p>`;

  try {
    const inter = await api(`/api/interventions/${encodeURIComponent(id)}`);
    state.currentInterventionDetail = inter;
    renderInterventionDetailView(inter);
  } catch (err) {
    container.innerHTML = `
      <div class="card">
        <p class="hint state-ko">${icon('i-alert')} Erreur lors du chargement de l'intervention : ${esc(err.message)}</p>
        <p><a href="#/interventions" class="btn">${icon('i-arrow-left')} <span>Retour à la liste</span></a></p>
      </div>
    `;
  } finally {
    state.currentInterventionLoading = false;
  }
}

function renderInterventionDetailView(inter) {
  const container = document.getElementById('interventionDetailContainer');
  if (!container) return;

  const canEdit = hasPermission('interventions.edit');
  const canDelete = hasPermission('interventions.delete');
  const currentUserId = state.currentUser ? state.currentUser.id : null;
  const isAssigned = Array.isArray(inter.technicians) && inter.technicians.some(t => t.user_id === currentUserId);
  const isOpen = inter.status === 'open';
  const isInProgress = inter.status === 'in_progress';
  const isCompleted = inter.status === 'completed';
  const isCancelled = inter.status === 'cancelled';
  const isMachineArret = inter.machine_state === 'ARRET' && (isOpen || isInProgress);

  let statusBannerHtml = '';
  if (isCompleted) {
    statusBannerHtml = `
      <div class="banner-status banner-completed">
        ${icon('i-check')}
        <div>
          <b>Intervention clôturée le ${formatDate(inter.completed_at)}.</b>
          <div>Durée totale passée : <b>${formatDurationMinutes(inter.duration_minutes)}</b>.</div>
        </div>
      </div>
    `;
  } else if (isCancelled) {
    statusBannerHtml = `
      <div class="banner-status banner-cancelled">
        ${icon('i-ban')}
        <div>
          <b>Intervention annulée le ${formatDate(inter.cancelled_at)}${inter.cancelled_by ? ` par ${esc(inter.cancelled_by.display_name)}` : ''}.</b>
          <div>Motif de l'annulation : <em>« ${esc(inter.cancel_reason || 'Aucun motif renseigné')} »</em></div>
        </div>
      </div>
    `;
  } else if (isMachineArret) {
    statusBannerHtml = `
      <div class="banner-status banner-arret">
        ${icon('i-alert')}
        <div>
          <b>Arrêt machine déclaré :</b> L'équipement associé est consigné hors service jusqu'à la fin complète des travaux.
        </div>
      </div>
    `;
  }

  const quickActions = [];

  if (canEdit && isOpen) {
    quickActions.push(`
      <button type="button" class="btn primary" data-action="take-intervention" data-args="${jsArg(inter.id)}">
        ${icon('i-wrench')} <span>Prendre en charge</span>
      </button>
    `);
  }

  if (canEdit && isInProgress) {
    quickActions.push(`
      <button type="button" class="btn primary" data-action="open-complete-intervention" data-args="${jsArg(inter.id)}">
        ${icon('i-check')} <span>Clôturer l'intervention</span>
      </button>
    `);

    if (!isAssigned) {
      quickActions.push(`
        <button type="button" class="btn" data-action="assign-myself" data-args="${jsArg(inter.id)}">
          ${icon('i-user')} <span>M'assigner</span>
        </button>
      `);
    }
  }

  if (canEdit && (isOpen || isInProgress)) {
    quickActions.push(`
      <button type="button" class="btn" data-action="open-cancel-intervention" data-args="${jsArg(inter.id)}">
        ${icon('i-ban')} <span>Annuler l'intervention</span>
      </button>
    `);
    quickActions.push(`
      <button type="button" class="btn" data-action="open-edit-intervention" data-args="${jsArg(inter.id)}">
        ${icon('i-edit')} <span>Modifier</span>
      </button>
    `);
  }

  if (canDelete) {
    quickActions.push(`
      <button type="button" class="btn danger" data-action="delete-intervention" data-args="${jsArg(inter.id)}">
        ${icon('i-trash')} <span>Supprimer</span>
      </button>
    `);
  }

  let equipHtml = '<span class="hint">Intervention hors équipement</span>';
  if (inter.equipment) {
    equipHtml = `
      <div>
        <button type="button" class="link-action" data-action="open-detail-equipment" data-args="${jsArg(inter.equipment.id)}">
          <b>${esc(inter.equipment.code)} — ${esc(inter.equipment.name)}</b>
        </button>
        <span class="detail-card-spaced">${statusBadge(inter.equipment.status)}</span>
      </div>
    `;
  }

  let parentBannerHtml = '';
  if (inter.parent) {
    parentBannerHtml = `
      <div class="hint-spaced">
        ${icon('i-info')} <b>Sous-tâche rattachée à :</b>
        <a href="#/interventions/${esc(inter.parent.id)}" class="link-action">
          <b>${esc(inter.parent.code)}</b> — ${esc(inter.parent.title)}
        </a>
      </div>
    `;
  }

  const techList = Array.isArray(inter.technicians) ? inter.technicians : [];
  const canManageTechs = canEdit && (isOpen || isInProgress);

  let techRows = techList.map(t => {
    const isMe = t.user_id === currentUserId;
    const canEditThisTime = canManageTechs && (isAdmin() || isResponsable() || isMe);

    const timeActions = [];
    if (canEditThisTime) {
      timeActions.push(`
        <button type="button" class="btn sm" data-action="open-technician-time" data-args="${jsArg({ interventionId: inter.id, userId: t.user_id, currentMinutes: t.duration_minutes, userName: t.display_name })}" title="Saisir ou modifier le temps passé">
          ${icon('i-clock')} <span>${t.duration_minutes !== null ? 'Modifier le temps' : (isMe ? 'Saisir mon temps' : 'Saisir le temps')}</span>
        </button>
      `);
    }

    if (canManageTechs) {
      timeActions.push(`
        <button type="button" class="btn sm danger" data-action="remove-technician" data-args="${jsArg({ interventionId: inter.id, userId: t.user_id })}" title="Retirer cet intervenant">
          ${icon('i-trash')}
        </button>
      `);
    }

    return `
      <tr>
        <td>
          <b>${esc(t.display_name)}</b>
          <div class="hint mono">${esc(t.username)} (${esc(formatRole(t.role))})</div>
        </td>
        <td>
          ${t.is_lead ? '<span class="badge-lead">Intervenant référent (Lead)</span>' : '<span class="badge">Intervenant</span>'}
        </td>
        <td>${formatDate(t.assigned_at)}</td>
        <td class="mono">
          <b>${formatDurationMinutes(t.duration_minutes)}</b>
        </td>
        <td><div class="cell-actions">${timeActions.join('')}</div></td>
      </tr>
    `;
  }).join('');

  if (!techList.length) {
    techRows = `<tr><td colspan="5" class="empty-cell">Aucun intervenant assigné pour le moment.</td></tr>`;
  }

  const subList = Array.isArray(inter.sub_interventions) ? inter.sub_interventions : [];
  let subRows = subList.map(sub => `
    <tr>
      <td>
        <a href="#/interventions/${esc(sub.id)}" class="link-action mono">
          <b>${esc(sub.code)}</b>
        </a>
      </td>
      <td>
        <a href="#/interventions/${esc(sub.id)}" class="link-action">
          <b>${esc(sub.title)}</b>
        </a>
      </td>
      <td>${interventionTypeBadge(sub.type)}</td>
      <td>${interventionPriorityBadge(sub.priority)}</td>
      <td>${machineStateBadge(sub.machine_state)}</td>
      <td>${interventionStatusBadge(sub.status)}</td>
      <td>
        <a href="#/interventions/${esc(sub.id)}" class="btn sm">
          ${icon('i-info')} <span>Consulter</span>
        </a>
      </td>
    </tr>
  `).join('');

  if (!subList.length) {
    subRows = `<tr><td colspan="7" class="empty-cell">Aucune sous-intervention rattachée.</td></tr>`;
  }

  container.innerHTML = `
    <!-- CARTE D'EN-TÊTE -->
    <section class="detail-header-card">
      <div class="detail-header-top">
        <div class="detail-title-block">
          ${parentBannerHtml}
          <div class="detail-code-row">
            <span class="detail-code-label mono">${esc(inter.code)}</span>
            <span class="detail-title-label">${esc(inter.title)}</span>
          </div>
          <div class="detail-badges-row">
            ${interventionStatusBadge(inter.status)}
            ${interventionPriorityBadge(inter.priority)}
            ${machineStateBadge(inter.machine_state)}
            ${interventionTypeBadge(inter.type)}
          </div>
        </div>
      </div>
      ${statusBannerHtml}
      ${quickActions.length ? `<div class="quick-actions-bar">${quickActions.join('')}</div>` : ''}
    </section>

    <!-- GRILLE DE DÉTAIL -->
    <div class="detail-grid">
      <!-- INFORMATIONS GÉNÉRALES -->
      <section class="card">
        <h2>${icon('i-info')} Informations générales</h2>
        <dl class="detail-dl">
          <dt>Équipement lié</dt><dd>${equipHtml}</dd>
          <dt>Demandeur</dt><dd>${esc(inter.reported_by ? inter.reported_by.display_name : '—')}</dd>
          <dt>Signalé le</dt><dd>${formatDate(inter.reported_at)}</dd>
          <dt>Début effectif</dt><dd>${formatDate(inter.started_at)}</dd>
          <dt>Clôture effective</dt><dd>${formatDate(inter.completed_at)}</dd>
          <dt>Durée totale</dt><dd><b class="mono">${formatDurationMinutes(inter.duration_minutes)}</b> (somme des temps intervenants)</dd>
          <dt>Description</dt><dd>${esc(inter.description || 'Aucune description complémentaire.')}</dd>
        </dl>
      </section>

      <!-- COMPTE-RENDU TECHNIQUE -->
      <section class="card">
        <h2>${icon('i-doc')} Compte-rendu d'intervention</h2>
        ${inter.solution_report ? `
          <div class="solution-report-box">${esc(inter.solution_report)}</div>
        ` : `
          <p class="hint">Le compte-rendu technique sera rédigé lors de la clôture des travaux.</p>
        `}
      </section>
    </div>

    <!-- TECHNICIENS ET TEMPS PASSÉ -->
    <section class="card">
      <div class="detail-nav-bar">
        <h2>${icon('i-users')} Intervenants et temps passé</h2>
        ${canManageTechs ? `
          <button type="button" class="btn sm" data-action="open-assign-technician" data-args="${jsArg(inter.id)}">
            ${icon('i-plus')} <span>Assigner un intervenant</span>
          </button>
        ` : ''}
      </div>
      <div class="table-container">
        <table class="data-table">
          <thead>
            <tr>
              <th>Intervenant</th>
              <th>Rôle</th>
              <th>Assigné le</th>
              <th>Temps passé</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            ${techRows}
          </tbody>
        </table>
      </div>
      <div class="tech-total-box">
        <span>Durée totale cumulée passée sur l'intervention :</span>
        <span class="tech-total-value mono">${formatDurationMinutes(inter.duration_minutes)}</span>
      </div>
    </section>

    <!-- SOUS-INTERVENTIONS -->
    ${!inter.parent_id ? `
      <section class="card">
        <div class="detail-nav-bar">
          <h2>${icon('i-wrench')} Sous-interventions (${esc(subList.length)})</h2>
          ${(canEdit && !isCompleted && !isCancelled) ? `
            <button type="button" class="btn sm" data-action="open-create-sub-intervention" data-args="${jsArg(inter.id)}">
              ${icon('i-plus')} <span>Créer une sous-tâche</span>
            </button>
          ` : ''}
        </div>
        <div class="table-container">
          <table class="data-table">
            <thead>
              <tr>
                <th>Référence</th>
                <th>Titre</th>
                <th>Type</th>
                <th>Priorité</th>
                <th>État machine</th>
                <th>Statut</th>
                <th>Action</th>
              </tr>
            </thead>
            <tbody>
              ${subRows}
            </tbody>
          </table>
        </div>
      </section>
    ` : ''}
  `;
}

// --- MODALES DU MODULE INTERVENTIONS ---

async function openCreateInterventionModal(prefill = {}) {
  // Charge la liste des techniciens et équipements si nécessaire
  let siteUsers = [];
  try {
    const siteParam = state.selectedSiteId ? `?site_id=${encodeURIComponent(state.selectedSiteId)}` : '';
    const uRes = await api(`/api/users${siteParam}`);
    const uList = Array.isArray(uRes) ? uRes : (uRes && Array.isArray(uRes.items) ? uRes.items : []);
    siteUsers = uList.filter(u => u.active && ['technicien', 'responsable', 'admin'].includes(u.role));
  } catch (_) {}

  if (!state.equipments.length) {
    try {
      const siteParam = state.selectedSiteId ? `?site_id=${encodeURIComponent(state.selectedSiteId)}&limit=100` : '?limit=100';
      const eqRes = await api(`/api/equipments${siteParam}`);
      if (eqRes && Array.isArray(eqRes.items)) state.equipments = eqRes.items;
    } catch (_) {}
  }

  const defaultType = prefill.type || 'corrective';
  const defaultMachineState = prefill.machine_state || 'FONCTIONNEL';

  const fieldsHtml = `
    ${renderSiteFormField()}
    <div class="form-row">
      <div class="form-group">
        <label for="intType">Type d'intervention *</label>
        <select id="intType" name="type" required data-action="on-intervention-type-change">
          <option value="corrective" ${defaultType === 'corrective' ? 'selected' : ''}>Corrective (dépannage)</option>
          <option value="preventive" ${defaultType === 'preventive' ? 'selected' : ''}>Préventive (planifiée)</option>
          <option value="a_posteriori" ${defaultType === 'a_posteriori' ? 'selected' : ''}>A posteriori (travaux déjà réalisés)</option>
          <option value="hors_equipement" ${defaultType === 'hors_equipement' ? 'selected' : ''}>Hors équipement (infrastructure, bâtiment)</option>
        </select>
      </div>
      <div class="form-group">
        <label for="intPriority">Priorité *</label>
        <select id="intPriority" name="priority" required>
          <option value="low">Basse</option>
          <option value="medium" selected>Moyenne</option>
          <option value="high">Haute</option>
          <option value="critical">Critique (urgente)</option>
        </select>
      </div>
    </div>

    <div class="form-group" id="intEquipGroup" ${defaultType === 'hors_equipement' ? 'hidden' : ''}>
      <label for="intEquipmentId">Équipement concerné *</label>
      <select id="intEquipmentId" name="equipment_id" ${defaultType === 'hors_equipement' ? '' : 'required'}>
        <option value="">Sélectionner un équipement…</option>
        ${state.equipments.map(e => `<option value="${esc(e.id)}" ${prefill.equipment_id === e.id ? 'selected' : ''}>${esc(e.code)} — ${esc(e.name)}</option>`).join('')}
      </select>
      <small class="hint">Obligatoire pour les interventions correctives, préventives et a posteriori.</small>
    </div>

    <div class="form-group">
      <label for="intMachineState">Impact sur l'état de la machine *</label>
      <select id="intMachineState" name="machine_state" required data-action="on-intervention-machine-state-change">
        <option value="FONCTIONNEL" ${defaultMachineState === 'FONCTIONNEL' ? 'selected' : ''}>FONCTIONNEL (machine en service normal)</option>
        <option value="DEGRADE" ${defaultMachineState === 'DEGRADE' ? 'selected' : ''}>DEGRADE (fonctionnement partiel / provisoire)</option>
        <option value="ARRET" ${defaultMachineState === 'ARRET' ? 'selected' : ''}>ARRET (machine arrêtée / hors service)</option>
      </select>
      <div id="intMachineStateAlert" class="machine-state-alert" ${defaultMachineState === 'ARRET' ? '' : 'hidden'}>
        ${icon('i-alert')}
        <span><b>Mise en garde :</b> La sélection « ARRET » basculera automatiquement l'équipement en statut <em>Hors service</em>.</span>
      </div>
    </div>

    <div class="form-group">
      <label for="intTitle">Titre de l'intervention *</label>
      <input type="text" id="intTitle" name="title" required placeholder="Ex. Remplacement roulement convoyeur" value="${esc(prefill.title || '')}" />
    </div>

    <div class="form-group">
      <label for="intDesc">Description du constat ou de la demande</label>
      <textarea id="intDesc" name="description" rows="3" placeholder="Description des symptômes, circonstances de la panne…">${esc(prefill.description || '')}</textarea>
    </div>

    ${siteUsers.length ? `
      <div class="form-group">
        <label>Technicien(s) à assigner</label>
        <div class="checkbox-group">
          ${siteUsers.map(u => `
            <label class="checkbox-label">
              <input type="checkbox" name="technician_ids" value="${esc(u.id)}" ${prefill.technician_ids && prefill.technician_ids.includes(u.id) ? 'checked' : ''} />
              <span>${esc(u.display_name)} (${esc(formatRole(u.role))})</span>
            </label>
          `).join('')}
        </div>
      </div>
    ` : ''}

    <div id="intAposterioriFields" ${defaultType === 'a_posteriori' ? '' : 'hidden'}>
      <div class="form-group">
        <label for="intSolutionReport">Compte-rendu technique des travaux réalisés *</label>
        <textarea id="intSolutionReport" name="solution_report" rows="3" placeholder="Diagnostic, action corrective réalisée, cause racine…"></textarea>
      </div>
      <div class="form-group">
        <label for="intDurationMinutes">Durée totale passée (minutes) *</label>
        <input type="number" id="intDurationMinutes" name="duration_minutes" min="1" placeholder="Ex. 60" />
      </div>
    </div>
  `;

  openFormModal({
    title: prefill.parentId ? 'Créer une sous-intervention' : 'Déclarer une intervention',
    formId: 'modalInterventionForm',
    submitAction: 'create-intervention-submit',
    submitArgs: prefill.parentId || null,
    submitLabel: 'Créer l\'intervention',
    fieldsHtml,
  });
}

async function handleCreateInterventionSubmit(form) {
  const parentId = form.dataset.args ? JSON.parse(form.dataset.args) : null;
  const btn = document.getElementById('modalInterventionFormSubmitBtn');
  const errBox = document.getElementById('modalInterventionFormErrorBox');
  const errText = document.getElementById('modalInterventionFormErrorBoxText');

  const type = form.type.value;
  const title = (form.title.value || '').trim();
  const description = (form.description.value || '').trim() || null;
  const priority = form.priority.value;
  const machine_state = form.machine_state.value;
  const equipment_id = (type === 'hors_equipement') ? null : (form.equipment_id.value || null);

  const techCheckboxes = form.querySelectorAll('input[name="technician_ids"]:checked');
  const technician_ids = Array.from(techCheckboxes).map(cb => cb.value);

  const payload = {
    title,
    description,
    type,
    priority,
    machine_state,
    equipment_id,
    technician_ids,
  };

  if (isAdmin() && form.site_id) {
    payload.site_id = form.site_id.value;
  }

  if (type === 'a_posteriori') {
    payload.solution_report = (form.solution_report.value || '').trim();
    if (form.duration_minutes.value) {
      payload.duration_minutes = parseInt(form.duration_minutes.value, 10);
    }
  }

  btn.disabled = true;
  errBox.hidden = true;

  try {
    let created;
    if (parentId) {
      created = await api(`/api/interventions/${encodeURIComponent(parentId)}/sub-interventions`, {
        method: 'POST',
        body: payload,
      });
    } else {
      created = await api('/api/interventions', {
        method: 'POST',
        body: payload,
      });
    }

    closeModal();
    notify('Intervention enregistrée avec succès.', 'success');

    if (created && created.id) {
      location.hash = `#/interventions/${created.id}`;
    } else {
      await loadInterventionsList(true);
    }
  } catch (err) {
    errText.textContent = err.message || 'Erreur lors de la création.';
    errBox.hidden = false;
    btn.disabled = false;
  }
}

async function openEditInterventionModal(id) {
  let inter = state.currentInterventionDetail;
  if (!inter || inter.id !== id) {
    try {
      inter = await api(`/api/interventions/${encodeURIComponent(id)}`);
    } catch (err) {
      notify(err.message, 'error');
      return;
    }
  }

  const fieldsHtml = `
    <div class="form-row">
      <div class="form-group">
        <label for="editIntPriority">Priorité *</label>
        <select id="editIntPriority" name="priority" required>
          <option value="low" ${inter.priority === 'low' ? 'selected' : ''}>Basse</option>
          <option value="medium" ${inter.priority === 'medium' ? 'selected' : ''}>Moyenne</option>
          <option value="high" ${inter.priority === 'high' ? 'selected' : ''}>Haute</option>
          <option value="critical" ${inter.priority === 'critical' ? 'selected' : ''}>Critique</option>
        </select>
      </div>
      <div class="form-group">
        <label for="editIntMachineState">État machine *</label>
        <select id="editIntMachineState" name="machine_state" required data-action="on-intervention-machine-state-change">
          <option value="FONCTIONNEL" ${inter.machine_state === 'FONCTIONNEL' ? 'selected' : ''}>FONCTIONNEL</option>
          <option value="DEGRADE" ${inter.machine_state === 'DEGRADE' ? 'selected' : ''}>DEGRADE</option>
          <option value="ARRET" ${inter.machine_state === 'ARRET' ? 'selected' : ''}>ARRET (machine arrêtée)</option>
        </select>
        <div id="editIntMachineStateAlert" class="machine-state-alert" ${inter.machine_state === 'ARRET' ? '' : 'hidden'}>
          ${icon('i-alert')}
          <span><b>Attention :</b> L'équipement passera immédiatement en Hors service.</span>
        </div>
      </div>
    </div>
    <div class="form-group">
      <label for="editIntTitle">Titre de l'intervention *</label>
      <input type="text" id="editIntTitle" name="title" value="${esc(inter.title)}" required />
    </div>
    <div class="form-group">
      <label for="editIntDesc">Description</label>
      <textarea id="editIntDesc" name="description" rows="3">${esc(inter.description || '')}</textarea>
    </div>
  `;

  openFormModal({
    title: `Modifier l'intervention « ${inter.code} »`,
    formId: 'modalEditInterventionForm',
    submitAction: 'edit-intervention-submit',
    submitArgs: inter.id,
    submitLabel: 'Enregistrer les modifications',
    fieldsHtml,
  });
}

async function handleEditInterventionSubmit(form) {
  const id = form.dataset.args ? JSON.parse(form.dataset.args) : null;
  if (!id) return;

  const btn = document.getElementById('modalEditInterventionFormSubmitBtn');
  const errBox = document.getElementById('modalEditInterventionFormErrorBox');
  const errText = document.getElementById('modalEditInterventionFormErrorBoxText');

  const title = (form.title.value || '').trim();
  const description = (form.description.value || '').trim() || null;
  const priority = form.priority.value;
  const machine_state = form.machine_state.value;

  btn.disabled = true;
  errBox.hidden = true;

  try {
    await api(`/api/interventions/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body: { title, description, priority, machine_state },
    });
    closeModal();
    notify('Intervention mise à jour.', 'success');
    await loadInterventionDetail(id);
  } catch (err) {
    errText.textContent = err.message || 'Erreur lors de la modification.';
    errBox.hidden = false;
    btn.disabled = false;
  }
}

async function takeIntervention(id) {
  try {
    const res = await api(`/api/interventions/${encodeURIComponent(id)}/take`, {
      method: 'POST',
      body: {},
    });
    notify('Intervention prise en charge avec succès.', 'success');
    if (state.currentInterventionDetail && state.currentInterventionDetail.id === id) {
      await loadInterventionDetail(id);
    } else {
      await loadInterventionsList(false);
    }
  } catch (err) {
    notify(err.message || 'Prise en charge refusée.', 'error');
  }
}

async function openCompleteInterventionModal(id) {
  let inter = state.currentInterventionDetail;
  if (!inter || inter.id !== id) {
    try {
      inter = await api(`/api/interventions/${encodeURIComponent(id)}`);
    } catch (err) {
      notify(err.message, 'error');
      return;
    }
  }

  // La clôture exige au moins un temps saisi
  const assignedTechs = Array.isArray(inter.technicians) ? inter.technicians : [];
  const hasReportedTime = assignedTechs.some(t => typeof t.duration_minutes === 'number' && t.duration_minutes >= 0);
  const currentUserId = state.currentUser ? state.currentUser.id : null;

  const timeWarningHtml = hasReportedTime ? `
    <div class="hint-spaced">
      ${icon('i-check')} <b>Temps cumulé comptabilisé :</b> <span class="mono">${formatDurationMinutes(inter.duration_minutes)}</span>.
    </div>
  ` : `
    <div class="machine-state-alert">
      ${icon('i-alert')}
      <span><b>Temps requis :</b> La clôture exige qu'au moins un temps soit saisi. Indiquez votre temps ci-dessous :</span>
    </div>
    <div class="form-group detail-card-spaced">
      <label for="closeMyTime">Votre temps passé (en minutes) *</label>
      <input type="number" id="closeMyTime" name="my_duration_minutes" min="1" required placeholder="Ex. 45" />
    </div>
  `;

  const fieldsHtml = `
    ${timeWarningHtml}
    <div class="form-group">
      <label for="closeReport">Compte-rendu technique de solution *</label>
      <textarea id="closeReport" name="solution_report" rows="4" required placeholder="Description détaillée du diagnostic, des réparations effectuées et de l'état final…"></textarea>
    </div>

    ${inter.equipment_id ? `
      <div class="form-group">
        <label for="closeFinalEquipStatus">Restitution de l'équipement (facultatif)</label>
        <select id="closeFinalEquipStatus" name="final_equipment_status">
          <option value="">Recalcul automatique de criticité résiduelle (recommandé)</option>
          <option value="operational">En service normal (opérationnel)</option>
          <option value="degraded">Fonctionnement dégradé (réparation provisoire)</option>
          <option value="out_of_service">Maintenir hors service</option>
        </select>
        <small class="hint">Si non précisé, le serveur recalcule le statut selon les éventuelles autres interventions actives sur cet équipement.</small>
      </div>
    ` : ''}
  `;

  openFormModal({
    title: `Clôturer l'intervention « ${inter.code} »`,
    formId: 'modalCompleteForm',
    submitAction: 'complete-intervention-submit',
    submitArgs: inter.id,
    submitLabel: 'Valider la clôture',
    fieldsHtml,
  });
}

async function handleCompleteInterventionSubmit(form) {
  const id = form.dataset.args ? JSON.parse(form.dataset.args) : null;
  if (!id) return;

  const btn = document.getElementById('modalCompleteFormSubmitBtn');
  const errBox = document.getElementById('modalCompleteFormErrorBox');
  const errText = document.getElementById('modalCompleteFormErrorBoxText');

  btn.disabled = true;
  errBox.hidden = true;

  try {
    // Si un temps pour l'utilisateur courant a été saisi dans le formulaire
    if (form.my_duration_minutes && form.my_duration_minutes.value) {
      const minutes = parseInt(form.my_duration_minutes.value, 10);
      if (isNaN(minutes) || minutes < 0) {
        throw new Error('La durée passée doit être un entier positif.');
      }
      // S'assurer qu'il est assigné et enregistrer le temps
      await api(`/api/interventions/${encodeURIComponent(id)}/technicians/${encodeURIComponent(state.currentUser.id)}/time`, {
        method: 'PUT',
        body: { duration_minutes: minutes },
      });
    }

    const solution_report = (form.solution_report.value || '').trim();
    const final_equipment_status = form.final_equipment_status ? form.final_equipment_status.value || null : null;

    const payload = { solution_report };
    if (final_equipment_status) payload.final_equipment_status = final_equipment_status;

    await api(`/api/interventions/${encodeURIComponent(id)}/complete`, {
      method: 'POST',
      body: payload,
    });

    closeModal();
    notify('Intervention clôturée avec succès.', 'success');
    await loadInterventionDetail(id);
  } catch (err) {
    errText.textContent = err.message || 'Erreur lors de la clôture.';
    errBox.hidden = false;
    btn.disabled = false;
  }
}

async function openCancelInterventionModal(id) {
  let inter = state.currentInterventionDetail;
  if (!inter || inter.id !== id) {
    try {
      inter = await api(`/api/interventions/${encodeURIComponent(id)}`);
    } catch (err) {
      notify(err.message, 'error');
      return;
    }
  }

  const fieldsHtml = `
    <div class="machine-state-alert">
      ${icon('i-alert')}
      <span><b>Action irréversible :</b> l'annulation est définitive et libère l'équipement si aucune autre intervention n'est active.</span>
    </div>
    <div class="form-group detail-card-spaced">
      <label for="cancelReason">Motif obligatoire de l'annulation *</label>
      <textarea id="cancelReason" name="reason" rows="3" required placeholder="Précisez la raison de l'annulation (ex. fausse alerte, intervention différée, doublon…)"></textarea>
    </div>
  `;

  openFormModal({
    title: `Annuler l'intervention « ${inter.code} »`,
    formId: 'modalCancelForm',
    submitAction: 'cancel-intervention-submit',
    submitArgs: inter.id,
    submitLabel: 'Confirmer l\'annulation',
    fieldsHtml,
  });
}

async function handleCancelInterventionSubmit(form) {
  const id = form.dataset.args ? JSON.parse(form.dataset.args) : null;
  if (!id) return;

  const btn = document.getElementById('modalCancelFormSubmitBtn');
  const errBox = document.getElementById('modalCancelFormErrorBox');
  const errText = document.getElementById('modalCancelFormErrorBoxText');

  const reason = (form.reason.value || '').trim();
  if (!reason) {
    errText.textContent = 'Le motif d\'annulation est obligatoire.';
    errBox.hidden = false;
    return;
  }

  btn.disabled = true;
  errBox.hidden = true;

  try {
    await api(`/api/interventions/${encodeURIComponent(id)}/cancel`, {
      method: 'POST',
      body: { reason },
    });
    closeModal();
    notify('Intervention annulée.', 'success');
    await loadInterventionDetail(id);
  } catch (err) {
    errText.textContent = err.message || 'Erreur lors de l\'annulation.';
    errBox.hidden = false;
    btn.disabled = false;
  }
}

function openTechnicianTimeModal(args) {
  const fieldsHtml = `
    <div class="form-group">
      <label for="techDurationInput">Temps passé en minutes *</label>
      <input type="number" id="techDurationInput" name="duration_minutes" min="0" required value="${args.currentMinutes !== null && args.currentMinutes !== undefined ? esc(args.currentMinutes) : ''}" placeholder="Ex. 60" />
      <small class="hint">Indiquez la durée en minutes (ex. 90 = 1 h 30 min).</small>
    </div>
  `;

  openFormModal({
    title: `Saisir le temps passé — ${esc(args.userName)}`,
    formId: 'modalTechTimeForm',
    submitAction: 'technician-time-submit',
    submitArgs: args,
    submitLabel: 'Enregistrer le temps',
    fieldsHtml,
  });
}

async function handleTechnicianTimeSubmit(form) {
  const args = form.dataset.args ? JSON.parse(form.dataset.args) : null;
  if (!args) return;

  const btn = document.getElementById('modalTechTimeFormSubmitBtn');
  const errBox = document.getElementById('modalTechTimeFormErrorBox');
  const errText = document.getElementById('modalTechTimeFormErrorBoxText');

  const minutesVal = (form.duration_minutes.value || '').trim();
  const minutes = minutesVal === '' ? null : parseInt(minutesVal, 10);
  if (minutes !== null && (isNaN(minutes) || minutes < 0)) {
    errText.textContent = 'La durée doit être un entier positif ou nul.';
    errBox.hidden = false;
    return;
  }

  btn.disabled = true;
  errBox.hidden = true;

  try {
    await api(`/api/interventions/${encodeURIComponent(args.interventionId)}/technicians/${encodeURIComponent(args.userId)}/time`, {
      method: 'PUT',
      body: { duration_minutes: minutes },
    });
    closeModal();
    notify('Temps passé enregistré.', 'success');
    await loadInterventionDetail(args.interventionId);
  } catch (err) {
    errText.textContent = err.message || 'Erreur lors de l\'enregistrement du temps.';
    errBox.hidden = false;
    btn.disabled = false;
  }
}

async function openAssignTechnicianModal(interventionId) {
  let siteUsers = [];
  try {
    const siteParam = state.selectedSiteId ? `?site_id=${encodeURIComponent(state.selectedSiteId)}` : '';
    const uRes = await api(`/api/users${siteParam}`);
    const uList = Array.isArray(uRes) ? uRes : (uRes && Array.isArray(uRes.items) ? uRes.items : []);
    siteUsers = uList.filter(u => u.active && ['technicien', 'responsable', 'admin'].includes(u.role));
  } catch (_) {}

  const fieldsHtml = `
    <div class="form-group">
      <label for="assignUserId">Intervenant à assigner *</label>
      <select id="assignUserId" name="user_id" required>
        <option value="">Sélectionner un intervenant…</option>
        ${siteUsers.map(u => `<option value="${esc(u.id)}">${esc(u.display_name)} (${esc(formatRole(u.role))})</option>`).join('')}
      </select>
    </div>
    <div class="form-group">
      <label class="checkbox-label">
        <input type="checkbox" name="is_lead" value="1" />
        <span>Définir comme intervenant principal (Lead)</span>
      </label>
    </div>
  `;

  openFormModal({
    title: 'Assigner un intervenant',
    formId: 'modalAssignTechForm',
    submitAction: 'assign-technician-submit',
    submitArgs: interventionId,
    submitLabel: 'Assigner',
    fieldsHtml,
  });
}

async function handleAssignTechnicianSubmit(form) {
  const interventionId = form.dataset.args ? JSON.parse(form.dataset.args) : null;
  if (!interventionId) return;

  const btn = document.getElementById('modalAssignTechFormSubmitBtn');
  const errBox = document.getElementById('modalAssignTechFormErrorBox');
  const errText = document.getElementById('modalAssignTechFormErrorBoxText');

  const user_id = form.user_id.value;
  const is_lead = form.is_lead.checked;

  btn.disabled = true;
  errBox.hidden = true;

  try {
    await api(`/api/interventions/${encodeURIComponent(interventionId)}/technicians`, {
      method: 'POST',
      body: { user_id, is_lead },
    });
    closeModal();
    notify('Intervenant assigné.', 'success');
    await loadInterventionDetail(interventionId);
  } catch (err) {
    errText.textContent = err.message || 'Erreur lors de l\'assignation.';
    errBox.hidden = false;
    btn.disabled = false;
  }
}

async function removeTechnician(args) {
  const ok = await confirmDialog(
    'Retirer l\'intervenant',
    'Voulez-vous vraiment retirer cet intervenant de l\'intervention ?',
    'Retirer',
    true
  );
  if (!ok) return;

  try {
    await api(`/api/interventions/${encodeURIComponent(args.interventionId)}/technicians/${encodeURIComponent(args.userId)}`, {
      method: 'DELETE',
    });
    notify('Intervenant retiré.', 'success');
    await loadInterventionDetail(args.interventionId);
  } catch (err) {
    notify(err.message || 'Erreur lors du retrait de l\'intervenant.', 'error');
  }
}

async function assignMyself(interventionId) {
  if (!state.currentUser) return;
  try {
    await api(`/api/interventions/${encodeURIComponent(interventionId)}/technicians`, {
      method: 'POST',
      body: { user_id: state.currentUser.id, is_lead: false },
    });
    notify('Vous êtes désormais assigné à cette intervention.', 'success');
    await loadInterventionDetail(interventionId);
  } catch (err) {
    notify(err.message || 'Impossible de vous assigner.', 'error');
  }
}

async function openCreateSubInterventionModal(parentId) {
  const parent = state.currentInterventionDetail;
  openCreateInterventionModal({
    parentId,
    equipment_id: parent ? parent.equipment_id : null,
    type: parent ? parent.type : 'corrective',
    priority: parent ? parent.priority : 'medium',
  });
}

async function deleteIntervention(id) {
  const ok = await confirmDialog(
    'Supprimer l\'intervention',
    'Voulez-vous vraiment supprimer logiquement cette intervention ? Cette action est réservée aux erreurs de saisie.',
    'Supprimer',
    true
  );
  if (!ok) return;

  try {
    await api(`/api/interventions/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    });
    notify('Intervention supprimée.', 'success');
    location.hash = '#/interventions';
  } catch (err) {
    notify(err.message || 'Suppression refusée.', 'error');
  }
}

// ============================================================================
// 15. SURVEILLANCE SERVEUR ET HORLOGE
// ============================================================================

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
  if (state.currentUser && currentRoute() === 'tableau-de-bord') {
    const view = document.getElementById('view');
    if (view && currentRoute() === 'tableau-de-bord') {
      view.innerHTML = dashboardPage();
    }
  }
}

function renderServerState() {
  const pill = document.getElementById('serverPill');
  if (pill) {
    pill.classList.toggle('offline', !state.online);
    pill.querySelector('.lab').textContent = state.online ? 'En ligne' : 'Injoignable';
  }
  if (state.health) {
    const prod = document.getElementById('statusProduct');
    const ver = document.getElementById('statusVersion');
    if (prod) prod.textContent = state.health.product || 'Apex Maint';
    if (ver) ver.textContent = state.health.version || '';
  }
  const chk = document.getElementById('statusCheck');
  if (chk) {
    chk.textContent = `Dernière vérification : ${formatTime(state.checkedAt)}`;
  }
}

function tickClock() {
  const el = document.getElementById('clock');
  if (!el) return;
  el.textContent = new Date().toLocaleString('fr-FR', {
    weekday: 'short',
    day: '2-digit',
    month: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function renderThemeToggle() {
  const btn = document.getElementById('themeToggle');
  if (!btn) return;
  const light = currentTheme() === 'light';
  btn.innerHTML = icon(light ? 'i-moon' : 'i-sun');
  btn.setAttribute('aria-label', light ? 'Passer au thème sombre' : 'Passer au thème clair');
  btn.title = btn.getAttribute('aria-label');
}

// ============================================================================
// 16. DÉLÉGATION GLOBALE D'ÉVÉNEMENTS (SANS ONCLICK / CSP STRICT)
// ============================================================================

function handleGlobalClick(e) {
  // Clic à l'extérieur pour fermer le dropdown utilisateur
  const userDropdown = document.getElementById('userDropdown');
  const userBtn = document.getElementById('userBtn');
  if (userDropdown && !userDropdown.hidden && !userDropdown.contains(e.target) && !userBtn.contains(e.target)) {
    closeUserDropdown();
  }

  const actEl = e.target.closest('[data-action]');
  if (!actEl) return;

  const action = actEl.dataset.action;
  let args = null;
  if (actEl.dataset.args) {
    try {
      args = JSON.parse(actEl.dataset.args);
    } catch (_) {
      args = actEl.dataset.args;
    }
  }

  switch (action) {
    case 'toggle-nav':
      toggleNav();
      break;
    case 'close-mobile-nav':
      setMobileNav(false);
      break;
    case 'toggle-theme':
      toggleTheme();
      renderThemeToggle();
      break;
    case 'toggle-user-menu':
      toggleUserDropdown();
      break;
    case 'close-user-menu':
      closeUserDropdown();
      break;
    case 'logout':
      handleLogout();
      break;
    case 'open-own-password-modal':
      openChangePasswordModal();
      break;
    case 'close-modal':
      closeModal();
      break;
    case 'confirm-dialog-ok':
      if (currentConfirmResolve) {
        currentConfirmResolve(true);
        currentConfirmResolve = null;
      }
      closeModal();
      break;
    case 'switch-users-tab':
      state.usersTab = args || 'users';
      render();
      break;
    case 'open-create-user':
      openCreateUserModal();
      break;
    case 'open-edit-user':
      openEditUserModal(args);
      break;
    case 'open-reset-user-pwd':
      openResetUserPwdModal(args);
      break;
    case 'toggle-user-active':
      toggleUserActive(args);
      break;
    case 'delete-user':
      deleteUser(args);
      break;
    case 'save-permissions-matrix':
      savePermissionsMatrix();
      break;
    case 'open-create-site':
      openCreateSiteModal();
      break;
    case 'open-edit-site':
      openEditSiteModal(args);
      break;
    case 'delete-site':
      deleteSite(args);
      break;

    // Phase 2 — Lignes de production
    case 'open-create-production-line':
      openCreateProductionLineModal();
      break;
    case 'open-edit-production-line':
      openEditProductionLineModal(args);
      break;
    case 'delete-production-line':
      deleteProductionLine(args);
      break;
    case 'load-more-production-lines':
      loadProductionLines(false);
      break;

    // Phase 2 — Équipements
    case 'switch-equipments-view':
      state.equipmentsViewMode = args || 'list';
      render();
      break;
    case 'open-create-equipment':
      openCreateEquipmentModal(args);
      break;
    case 'open-detail-equipment':
      openEquipmentDetailModal(args);
      break;
    case 'open-edit-equipment':
      openEditEquipmentModal(args);
      break;
    case 'delete-equipment':
      deleteEquipment(args);
      break;
    case 'toggle-equipment-node':
      toggleEquipmentTreeNode(args);
      break;
    case 'load-more-equipments':
      loadEquipmentsList(false);
      break;

    // Phase 2 — Emplacements
    case 'open-create-location':
      openCreateLocationModal(args);
      break;
    case 'open-edit-location':
      openEditLocationModal(args);
      break;
    case 'delete-location':
      deleteLocation(args);
      break;
    case 'toggle-location-node':
      toggleLocationTreeNode(args);
      break;
    case 'expand-all-locations':
      expandAllLocations(true);
      break;
    case 'collapse-all-locations':
      expandAllLocations(false);
      break;

    // Phase 2 — Fournisseurs
    case 'open-create-supplier':
      openCreateSupplierModal();
      break;
    case 'open-edit-supplier':
      openEditSupplierModal(args);
      break;
    case 'delete-supplier':
      deleteSupplier(args);
      break;
    case 'load-more-suppliers':
      loadSuppliers(false);
      break;

    // Phase 3 — Interventions
    case 'tab-interventions':
      state.interventionsQuickTab = args || 'all';
      document.querySelectorAll('.filter-tabs .filter-tab').forEach(btn => {
        btn.classList.toggle('active', btn.dataset.args === jsArg(state.interventionsQuickTab));
      });
      loadInterventionsList(true);
      break;
    case 'open-create-intervention':
      openCreateInterventionModal();
      break;
    case 'open-detail-intervention':
      location.hash = `#/interventions/${args}`;
      break;
    case 'open-edit-intervention':
      openEditInterventionModal(args);
      break;
    case 'take-intervention':
      takeIntervention(args);
      break;
    case 'open-complete-intervention':
      openCompleteInterventionModal(args);
      break;
    case 'open-cancel-intervention':
      openCancelInterventionModal(args);
      break;
    case 'open-technician-time':
      openTechnicianTimeModal(args);
      break;
    case 'open-assign-technician':
      openAssignTechnicianModal(args);
      break;
    case 'remove-technician':
      removeTechnician(args);
      break;
    case 'assign-myself':
      assignMyself(args);
      break;
    case 'open-create-sub-intervention':
      openCreateSubInterventionModal(args);
      break;
    case 'delete-intervention':
      deleteIntervention(args);
      break;
    case 'load-more-interventions':
      loadInterventionsList(false);
      break;

    default:
      break;
  }
}

function handleGlobalSubmit(e) {
  const form = e.target.closest('form[data-action]');
  if (!form) return;

  e.preventDefault();
  const action = form.dataset.action;

  switch (action) {
    case 'login-submit':
      handleLoginSubmit(form);
      break;
    case 'force-password-submit':
      handleForcePasswordSubmit(form);
      break;
    case 'own-password-submit':
      handleOwnPasswordSubmit(form, 'profilePwdErrorBox', 'profilePwdErrorText', 'profPwdSubmitBtn');
      break;
    case 'modal-password-submit':
      handleOwnPasswordSubmit(form, 'modalPwdErrorBox', 'modalPwdErrorText', 'modalPwdSubmitBtn');
      break;
    case 'create-user-submit':
      handleCreateUserSubmit(form);
      break;
    case 'edit-user-submit':
      handleEditUserSubmit(form);
      break;
    case 'reset-user-pwd-submit':
      handleResetUserPwdSubmit(form);
      break;
    case 'create-site-submit':
      handleCreateSiteSubmit(form);
      break;
    case 'edit-site-submit':
      handleEditSiteSubmit(form);
      break;

    // Phase 2
    case 'create-production-line-submit':
      handleCreateProductionLineSubmit(form);
      break;
    case 'edit-production-line-submit':
      handleEditProductionLineSubmit(form);
      break;
    case 'create-equipment-submit':
      handleCreateEquipmentSubmit(form);
      break;
    case 'edit-equipment-submit':
      handleEditEquipmentSubmit(form);
      break;
    case 'create-location-submit':
      handleCreateLocationSubmit(form);
      break;
    case 'edit-location-submit':
      handleEditLocationSubmit(form);
      break;
    case 'create-supplier-submit':
      handleCreateSupplierSubmit(form);
      break;
    case 'edit-supplier-submit':
      handleEditSupplierSubmit(form);
      break;

    // Phase 3
    case 'create-intervention-submit':
      handleCreateInterventionSubmit(form);
      break;
    case 'edit-intervention-submit':
      handleEditInterventionSubmit(form);
      break;
    case 'complete-intervention-submit':
      handleCompleteInterventionSubmit(form);
      break;
    case 'cancel-intervention-submit':
      handleCancelInterventionSubmit(form);
      break;
    case 'technician-time-submit':
      handleTechnicianTimeSubmit(form);
      break;
    case 'assign-technician-submit':
      handleAssignTechnicianSubmit(form);
      break;

    default:
      break;
  }
}

function handleGlobalInput(e) {
  const input = e.target.closest('input[data-action]');
  if (!input) return;

  const action = input.dataset.action;
  if (action === 'filter-users-search') {
    state.usersSearchQuery = input.value;
    renderUsersTable();
  } else if (action === 'filter-production-lines-search') {
    state.productionLinesSearchQuery = input.value;
    debounce('production-lines-search', () => loadProductionLines(true), 300);
  } else if (action === 'filter-equipments-search') {
    state.equipmentsSearchQuery = input.value;
    debounce('equipments-search', () => loadEquipmentsList(true), 300);
  } else if (action === 'filter-suppliers-search') {
    state.suppliersSearchQuery = input.value;
    debounce('suppliers-search', () => loadSuppliers(true), 300);
  } else if (action === 'filter-interventions-search') {
    state.interventionsSearchQuery = input.value;
    debounce('interventions-search', () => loadInterventionsList(true), 300);
  }
}

function handleGlobalChange(e) {
  // Sélecteur de site d'en-tête
  if (e.target && e.target.id === 'siteSelect') {
    state.selectedSiteId = e.target.value;
    const r = currentRoute();
    if (r === 'utilisateurs') {
      renderUsersTable();
    } else if (r === 'lignes-de-production') {
      loadProductionLines(true);
    } else if (r === 'equipements') {
      if (state.equipmentsViewMode === 'tree') {
        loadEquipmentsTree();
      } else {
        loadEquipmentsList(true);
      }
    } else if (r === 'emplacements') {
      loadLocationsTree();
    } else if (r === 'fournisseurs') {
      loadSuppliers(true);
    } else if (r === 'interventions') {
      loadInterventionsList(true);
    }
    return;
  }

  const sel = e.target.closest('select[data-action]');
  if (!sel) return;

  const action = sel.dataset.action;
  if (action === 'filter-users-role') {
    state.usersRoleFilter = sel.value;
    renderUsersTable();
  } else if (action === 'filter-users-site') {
    state.usersSiteFilter = sel.value;
    renderUsersTable();
  } else if (action === 'filter-equipments-status') {
    state.equipmentsStatusFilter = sel.value;
    loadEquipmentsList(true);
  } else if (action === 'filter-equipments-line') {
    state.equipmentsLineFilter = sel.value;
    loadEquipmentsList(true);
  } else if (action === 'filter-interventions-type') {
    state.interventionsTypeFilter = sel.value;
    loadInterventionsList(true);
  } else if (action === 'filter-interventions-status') {
    state.interventionsStatusFilter = sel.value;
    loadInterventionsList(true);
  } else if (action === 'filter-interventions-priority') {
    state.interventionsPriorityFilter = sel.value;
    loadInterventionsList(true);
  } else if (action === 'filter-interventions-equipment') {
    state.interventionsEquipmentFilter = sel.value;
    loadInterventionsList(true);
  } else if (action === 'on-intervention-type-change') {
    const isHorsEquip = sel.value === 'hors_equipement';
    const isAposteriori = sel.value === 'a_posteriori';
    const equipGroup = document.getElementById('intEquipGroup');
    const equipSelect = document.getElementById('intEquipmentId');
    const aposterioriFields = document.getElementById('intAposterioriFields');

    if (equipGroup) equipGroup.hidden = isHorsEquip;
    if (equipSelect) {
      if (isHorsEquip) {
        equipSelect.removeAttribute('required');
        equipSelect.value = '';
      } else {
        equipSelect.setAttribute('required', 'required');
      }
    }
    if (aposterioriFields) {
      aposterioriFields.hidden = !isAposteriori;
    }
  } else if (action === 'on-intervention-machine-state-change') {
    const alertBox = document.getElementById('intMachineStateAlert') || document.getElementById('editIntMachineStateAlert');
    if (alertBox) {
      alertBox.hidden = sel.value !== 'ARRET';
    }
  } else if (action === 'on-user-role-change') {
    // Si rôle admin sélectionné, le site devient optionnel / non requis
    const siteSel = document.getElementById('uSite');
    if (siteSel && isAdmin()) {
      if (sel.value === 'admin') {
        siteSel.value = '';
      }
    }
  } else if (action === 'on-modal-site-change') {
    const eqLine = document.getElementById('eqLine');
    const eqParent = document.getElementById('eqParent');
    if (eqLine || eqParent) {
      const siteId = sel.value;
      const siteParam = siteId ? `?site_id=${encodeURIComponent(siteId)}&limit=100` : '?limit=100';
      Promise.all([
        api(`/api/production-lines${siteParam}`),
        api(`/api/equipments${siteParam}`)
      ]).then(([lRes, eRes]) => {
        if (eqLine && lRes && Array.isArray(lRes.items)) {
          eqLine.innerHTML = `<option value="">-- Aucune ligne --</option>` +
            lRes.items.map(l => `<option value="${esc(l.id)}">${esc(l.code)} — ${esc(l.name)}</option>`).join('');
        }
        if (eqParent && eRes && Array.isArray(eRes.items)) {
          eqParent.innerHTML = `<option value="">-- Racine (aucun parent) --</option>` +
            eRes.items.map(p => `<option value="${esc(p.id)}">${esc(p.code)} — ${esc(p.name)}</option>`).join('');
        }
      }).catch(() => {});
    }
  }
}

function handleGlobalKeydown(e) {
  if (e.key === 'Escape') {
    const modal = document.getElementById('modalOverlay');
    if (modal && !modal.hidden) {
      closeModal();
      return;
    }
    closeUserDropdown();
  }
}

// ============================================================================
// 17. INITIALISATION
// ============================================================================

async function init() {
  restoreNavFold();
  renderThemeToggle();

  // Écouteurs de délégation globale
  document.addEventListener('click', handleGlobalClick);
  document.addEventListener('submit', handleGlobalSubmit);
  document.addEventListener('input', handleGlobalInput);
  document.addEventListener('change', handleGlobalChange);
  document.addEventListener('keydown', handleGlobalKeydown);

  // Synchronisation du thème si changement externe (OS)
  new MutationObserver(renderThemeToggle).observe(document.documentElement, { attributeFilter: ['data-theme'] });
  MOBILE_QUERY.addEventListener('change', () => setMobileNav(false));
  window.addEventListener('hashchange', render);

  tickClock();
  setInterval(tickClock, 15000);

  // Surveillance santé serveur
  checkHealth();
  setInterval(checkHealth, HEALTH_PERIOD_MS);

  // Authentification et chargement initial
  await checkAuth();
}

document.addEventListener('DOMContentLoaded', init);
