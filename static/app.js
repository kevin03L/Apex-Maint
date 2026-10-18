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

/** Date et heure locales au format AAAA-MM-JJTHH:MM (toISOString donnerait l'heure UTC). */
function localIso(d = new Date()) {
  const p = n => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** Formate une date RFC 3339 / ISO en date lisible en français. */
function formatDate(d) {
  if (!d) return '—';
  // Date civile AAAA-MM-JJ (échéances) : affichée telle quelle, sans heure ni conversion de fuseau.
  const civil = /^(\d{4})-(\d{2})-(\d{2})$/.exec(String(d));
  if (civil) return `${civil[3]}/${civil[2]}/${civil[1]}`;
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

/** Formate un montant en centimes d'euro en chaîne lisible en euros (ex. 1450 -> "14,50 €"). */
function formatCents(cents) {
  if (cents === null || cents === undefined || cents === '') return '—';
  const num = parseInt(cents, 10);
  if (isNaN(num)) return '—';
  return (num / 100).toLocaleString('fr-FR', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }) + ' €';
}

/** Formate des centimes pour un champ de formulaire type number step 0.01. */
function centsToEuroInput(cents) {
  if (cents === null || cents === undefined || cents === '') return '';
  const num = parseInt(cents, 10);
  if (isNaN(num)) return '';
  return (num / 100).toFixed(2);
}

/** Convertit une saisie utilisateur en euros ("14,50" ou "14.50") en centimes entiers. */
function eurosToCents(val) {
  if (val === null || val === undefined || val === '') return 0;
  const cleaned = String(val).replace(',', '.').trim();
  const num = parseFloat(cleaned);
  if (isNaN(num) || num < 0) return 0;
  return Math.round(num * 100);
}

/** Libellé convivial d'une catégorie de pièce détachée. */
function formatPartCategory(category) {
  switch (category) {
    case 'roulement':
      return 'Roulement';
    case 'visserie':
      return 'Visserie / Fixation';
    case 'moteur':
      return 'Moteur / Réducteur';
    case 'pneumatique':
      return 'Pneumatique';
    case 'hydraulique':
      return 'Hydraulique';
    case 'electrique':
      return 'Électrique';
    case 'capteur':
      return 'Capteur / Détection';
    case 'mecanique':
      return 'Mécanique';
    case 'consommable':
      return 'Consommable';
    default:
      return category || 'Autre';
  }
}

/** Libellé convivial pour l'unité de gestion d'une pièce. */
function formatPartUnit(unit) {
  switch (unit) {
    case 'piece':
      return 'Pièce(s)';
    case 'metre':
      return 'Mètre(s)';
    case 'litre':
      return 'Litre(s)';
    case 'kg':
      return 'Kg';
    case 'lot':
      return 'Lot(s)';
    case 'boite':
      return 'Boîte(s)';
    default:
      return unit || 'Pièce';
  }
}

/** Libellé convivial pour un type de mouvement de stock. */
function formatMovementType(type) {
  switch (type) {
    case 'in':
      return 'Entrée / Réception';
    case 'out':
      return 'Sortie intervention';
    case 'transfer':
      return 'Transfert interne';
    case 'adjustment':
      return 'Ajustement inventaire';
    default:
      return type || 'Mouvement';
  }
}

/** Badge stylé pour un type de mouvement de stock. */
function movementTypeBadge(type) {
  return `<span class="badge badge-mvt-${esc(type)}">${esc(formatMovementType(type))}</span>`;
}

/** Libellé convivial pour un motif d'ajustement. */
function formatAdjustmentKind(kind) {
  switch (kind) {
    case 'inventaire':
      return 'Écart d\'inventaire';
    case 'rebut':
      return 'Mise au rebut (qualité)';
    case 'perte':
      return 'Perte constatée';
    case 'casse':
      return 'Casse accidentelle';
    default:
      return kind || 'Ajustement';
  }
}

/** Libellé convivial pour un statut de réservation de pièce. */
function formatReservationStatus(status) {
  switch (status) {
    case 'prepared':
      return 'Préparée';
    case 'consumed':
      return 'Consommée';
    case 'released':
      return 'Remise en rayon';
    default:
      return status || '—';
  }
}

/** Badge stylé pour un statut de réservation. */
function reservationStatusBadge(status) {
  return `<span class="badge badge-res-${esc(status)}">${esc(formatReservationStatus(status))}</span>`;
}

/** Badge stylé pour le niveau de stock d'une pièce (En stock, Stock bas, Rupture). */
function partStockBadge(part) {
  if (part.total_stock === 0) {
    return `<span class="badge badge-stock-zero">${icon('i-alert')} <span>Rupture (0)</span></span>`;
  }
  if (part.is_low_stock) {
    return `<span class="badge badge-stock-low">${icon('i-alert')} <span>Stock bas (${esc(part.total_stock)} / seuil ${esc(part.effective_threshold)})</span></span>`;
  }
  return `<span class="badge badge-stock-ok"><span>En stock (${esc(part.total_stock)})</span></span>`;
}

/** Groupe de pastilles détaillant En stock / Réservé / Disponible. */
function stockLevelsPill(part) {
  const availCls = part.available_stock === 0 ? 'avail-zero' : (part.is_low_stock ? 'avail-low' : '');
  return `
    <div class="stock-pill-group" title="Physique : ${esc(part.total_stock)} — Réservé : ${esc(part.reserved_stock)} — Disponible : ${esc(part.available_stock)}">
      <span class="stock-pill">Stock: <b>${esc(part.total_stock)}</b></span>
      ${part.reserved_stock > 0 ? `<span class="stock-pill" title="Pièces réservées préparées pour interventions">Réservé: <b>${esc(part.reserved_stock)}</b></span>` : ''}
      <span class="stock-pill pill-avail ${availCls}">Dispo: <b>${esc(part.available_stock)}</b></span>
    </div>
  `;
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

// --- Helpers Phase 5 (Maintenance préventive & compteurs) ---

/** Libellé lisible d'une fréquence de planning. */
function formatPlanFrequency(freq) {
  switch (freq) {
    case 'daily':
      return 'Journalière';
    case 'weekly':
      return 'Hebdomadaire';
    case 'monthly':
      return 'Mensuelle';
    case 'quarterly':
      return 'Trimestrielle';
    case 'yearly':
      return 'Annuelle';
    case 'interval_days':
      return 'Intervalle (jours)';
    case 'counter':
      return 'Compteur';
    default:
      return freq || '—';
  }
}

/** Libellé complet de fréquence avec intervalle ou compteur. */
function planFrequencyLabel(plan) {
  if (!plan) return '—';
  if (plan.frequency === 'interval_days') {
    return `Tous les ${plan.interval_days} jours`;
  }
  if (plan.frequency === 'counter') {
    const unit = plan.counter ? ` ${plan.counter.unit}` : '';
    return `Tous les ${plan.counter_interval}${unit}`;
  }
  return formatPlanFrequency(plan.frequency);
}

/** Libellé lisible d'un état de planning. */
function planStateLabel(st) {
  switch (st) {
    case 'overdue':
      return 'En retard';
    case 'due_soon':
      return 'Bientôt';
    case 'ok':
      return 'À jour';
    case 'suspended':
      return 'Suspendu';
    default:
      return st || '—';
  }
}

/** Badge stylé pour l'état d'un planning. */
function planStateBadge(st) {
  return `<span class="badge badge-plan-${esc(st)}">${esc(planStateLabel(st))}</span>`;
}

/** Libellé lisible du statut d'une occurrence. */
function occurrenceStatusLabel(st) {
  switch (st) {
    case 'generated':
      return 'Intervention ouverte';
    case 'done':
      return 'Réalisée';
    case 'skipped':
      return 'Reportée';
    case 'missed':
      return 'Manquée';
    case 'projected':
      return 'Projetée';
    case 'overdue':
      return 'En retard';
    default:
      return st || '—';
  }
}

/** Badge stylé pour le statut d'une occurrence. */
function occurrenceStatusBadge(st) {
  return `<span class="badge badge-occ-${esc(st)}">${esc(occurrenceStatusLabel(st))}</span>`;
}

/** Formatage textuel de la prochaine échéance d'un planning. */
function formatPlanDue(p) {
  if (!p) return '—';
  if (p.frequency === 'counter') {
    if (p.next_due_counter === null || p.next_due_counter === undefined) return '—';
    const unit = p.counter ? ` ${p.counter.unit}` : '';
    const current = p.counter ? p.counter.current_value : 0;
    const diff = p.next_due_counter - current;
    if (diff <= 0) {
      return `Seuil atteint (${p.next_due_counter}${unit})`;
    }
    return `dans ${diff}${unit} (seuil ${p.next_due_counter})`;
  }
  return p.next_due_date ? formatDate(p.next_due_date) : '—';
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
        ${opts.secondaryActionsHtml || ''}
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
 * Rendu du menu déroulant d'export (CSV, Excel, PDF) pour les listes.
 */
function renderExportDropdown(listName, extraParams = {}) {
  if (!hasPermission('exports.run')) return '';
  const argsCsv = JSON.stringify({ list: listName, format: 'csv', ...extraParams });
  const argsXlsx = JSON.stringify({ list: listName, format: 'xlsx', ...extraParams });
  const argsPdf = JSON.stringify({ list: listName, format: 'pdf', ...extraParams });

  return `
    <div class="dropdown-wrap">
      <button type="button" class="btn secondary" data-action="toggle-export-dropdown" title="Exporter cette liste" aria-haspopup="true">
        ${icon('i-download')} <span>Exporter</span>
      </button>
      <div class="dropdown-menu" hidden>
        <button type="button" class="dropdown-item" data-action="run-export" data-args="${jsArg(argsCsv)}">
          ${icon('i-file-spreadsheet')} <span>Format CSV</span>
        </button>
        <button type="button" class="dropdown-item" data-action="run-export" data-args="${jsArg(argsXlsx)}">
          ${icon('i-file-spreadsheet')} <span>Format Excel (.xlsx)</span>
        </button>
        ${listName !== 'audit' ? `
          <button type="button" class="dropdown-item" data-action="run-export" data-args="${jsArg(argsPdf)}">
            ${icon('i-file-pdf')} <span>Format PDF (.pdf)</span>
          </button>
        ` : ''}
      </div>
    </div>
  `;
}

function toggleExportDropdown(btn) {
  const wrap = btn.closest('.dropdown-wrap');
  if (!wrap) return;
  const menu = wrap.querySelector('.dropdown-menu');
  if (!menu) return;
  const isHidden = menu.hidden;
  document.querySelectorAll('.dropdown-menu').forEach(m => { m.hidden = true; });
  menu.hidden = !isHidden;
  btn.setAttribute('aria-expanded', String(!isHidden));
}

async function handleRunExport(args) {
  let opts = args;
  if (typeof args === 'string') {
    try { opts = JSON.parse(args); } catch (_) { opts = {}; }
  }
  const list = (opts && opts.list) || 'interventions';
  const format = (opts && opts.format) || 'csv';

  const params = new URLSearchParams();
  params.set('format', format);

  if (list.startsWith('report-')) {
    const reportTab = (opts && opts.tab) || state.reportingTab || 'activity';
    params.set('period', state.reportingPeriod || '30d');
    if (state.reportingSiteId) params.set('site_id', state.reportingSiteId);
    if (state.reportingLineId) params.set('line_id', state.reportingLineId);
    if (state.reportingEquipmentId) params.set('equipment_id', state.reportingEquipmentId);
    if (state.reportingType) params.set('type', state.reportingType);
    if (state.reportingPeriod === 'custom') {
      if (state.reportingFrom) params.set('from', state.reportingFrom);
      if (state.reportingTo) params.set('to', state.reportingTo);
    }
    await downloadFile(`/api/reports/${encodeURIComponent(reportTab)}/export?${params.toString()}`);
    return;
  }

  if (list === 'interventions') {
    if (state.interventionsSearchQuery) params.set('q', state.interventionsSearchQuery);
    if (state.interventionsStatusFilter) params.set('status', state.interventionsStatusFilter);
    if (state.interventionsPriorityFilter) params.set('priority', state.interventionsPriorityFilter);
    if (state.interventionsTypeFilter) params.set('type', state.interventionsTypeFilter);
    if (state.interventionsEquipmentFilter) params.set('equipment_id', state.interventionsEquipmentFilter);
  } else if (list === 'equipments') {
    if (state.equipmentsSearchQuery) params.set('q', state.equipmentsSearchQuery);
    if (state.equipmentsStatusFilter) params.set('status', state.equipmentsStatusFilter);
    if (state.equipmentsLineFilter) params.set('line_id', state.equipmentsLineFilter);
  } else if (list === 'parts') {
    if (state.partsSearchQuery) params.set('q', state.partsSearchQuery);
  } else if (list === 'stock') {
    if (state.stockLevelsLocationFilter) params.set('location_id', state.stockLevelsLocationFilter);
  } else if (list === 'movements') {
    if (state.stockMovementsTypeFilter) params.set('type', state.stockMovementsTypeFilter);
  } else if (list === 'maintenance_plans') {
    if (state.maintenancePlansSearchQuery) params.set('q', state.maintenancePlansSearchQuery);
    if (state.maintenancePlansEquipmentFilter) params.set('equipment_id', state.maintenancePlansEquipmentFilter);
    if (state.maintenancePlansDueFilter && state.maintenancePlansDueFilter !== 'all') params.set('due', state.maintenancePlansDueFilter);
  } else if (list === 'audit') {
    if (state.auditQuery) params.set('q', state.auditQuery);
    if (state.auditAction) params.set('action', state.auditAction);
    if (state.auditEntity) params.set('entity', state.auditEntity);
    if (state.auditSiteId) params.set('site_id', state.auditSiteId);
    if (state.auditFrom) params.set('from', state.auditFrom);
    if (state.auditTo) params.set('to', state.auditTo);
  }

  await downloadFile(`/api/exports/${encodeURIComponent(list)}?${params.toString()}`);
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

/**
 * Télécharge un fichier depuis une URL de l'API avec transmission des cookies de session.
 * Affiche un message d'erreur en cas de réponse négative du serveur (403, 413, etc.).
 */
async function downloadFile(url) {
  try {
    notify('Préparation du téléchargement…', 'info');
    const res = await fetch(url, { credentials: 'same-origin' });
    if (!res.ok) {
      const err = await parseJsonSafe(res);
      notify(err && err.message ? err.message : `Erreur HTTP ${res.status}`, 'error');
      return;
    }
    const blob = await res.blob();
    const disposition = res.headers.get('content-disposition') || '';
    let filename = '';
    const m = disposition.match(/filename\*?=(?:UTF-8'')?["']?([^"';]+)["']?/i);
    if (m) filename = decodeURIComponent(m[1]);
    if (!filename) filename = url.split('/').pop().split('?')[0] || 'export';
    const blobUrl = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = blobUrl;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(blobUrl);
    notify('Téléchargement terminé.', 'success');
  } catch (err) {
    notify(err.message || 'Erreur lors du téléchargement.', 'error');
  }
}

/**
 * Adapte une ligne de stock, de mouvement, de réservation ou de pièce renvoyée à plat par le
 * serveur (part_reference, location_name, user_display_name…) aux objets imbriqués lus par les
 * vues (part, location, created_by…). Les champs déjà présents ne sont jamais écrasés.
 */
function adaptStock(o) {
  if (!o || typeof o !== 'object') return o;
  const r = { ...o };
  const ref = (id, code, name) => ({ id, code, name });
  if (!r.part && (o.part_reference || o.part_name)) {
    r.part = {
      id: o.part_id, reference: o.part_reference, name: o.part_name, unit: o.part_unit,
      available_stock: o.available_stock, category: o.part_category, unit_price_cents: o.unit_price_cents,
    };
  }
  if (!r.location && (o.location_code || o.location_name)) r.location = ref(o.location_id, o.location_code, o.location_name);
  if (!r.from_location && o.from_location_id) r.from_location = ref(o.from_location_id, o.from_location_code, o.from_location_name);
  if (!r.to_location && o.to_location_id) r.to_location = ref(o.to_location_id, o.to_location_code, o.to_location_name);
  if (!r.created_by && o.user_display_name) r.created_by = { id: o.user_id, display_name: o.user_display_name };
  if (!r.reserved_by && o.prepared_by_name) r.reserved_by = { id: o.prepared_by_id, display_name: o.prepared_by_name };
  if (!r.intervention && o.intervention_id && o.intervention_code) {
    r.intervention = { id: o.intervention_id, code: o.intervention_code, title: o.intervention_title };
  }
  if (r.reserved === undefined && o.reserved_quantity !== undefined) r.reserved = o.reserved_quantity;
  if (r.available === undefined && o.available_quantity !== undefined) r.available = o.available_quantity;
  if (r.available_stock === undefined && o.total_available !== undefined) r.available_stock = o.total_available;
  if (r.total_price_cents === undefined && o.movement_id && Number.isInteger(o.unit_price_cents)) {
    r.total_price_cents = o.quantity * o.unit_price_cents;
  }
  if (r.reference === undefined && o.part_reference) r.reference = o.part_reference;
  if (r.name === undefined && o.part_name) r.name = o.part_name;
  return r;
}

/**
 * Adapte un document renvoyé par le serveur de façon tolérante aux deux formes :
 * champs imbriqués (current_version { ... }, entity { ... }, uploaded_by { ... })
 * ou à plat (original_name, size_bytes, mime_type, entity_code, entity_label...).
 */
function adaptDocument(o) {
  if (!o || typeof o !== 'object') return o;
  const d = { ...o };
  if (!d.current_version && (o.original_name || o.version_no || o.size_bytes)) {
    d.current_version = {
      id: o.current_version_id || o.version_id,
      version_no: o.version_no || 1,
      original_name: o.original_name || 'document',
      mime_type: o.mime_type || 'application/octet-stream',
      extension: o.extension || '',
      size_bytes: o.size_bytes || 0,
      sha256: o.sha256 || '',
      comment: o.comment || null,
      uploaded_at: o.uploaded_at || o.created_at,
      uploaded_by: o.uploaded_by || (o.uploaded_by_name ? { id: o.uploaded_by_id, display_name: o.uploaded_by_name } : null),
    };
  }
  if (d.current_version && !d.current_version.uploaded_by && d.current_version.uploaded_by_name) {
    d.current_version.uploaded_by = { id: d.current_version.uploaded_by_id, display_name: d.current_version.uploaded_by_name };
  }
  if (!d.entity && (o.entity_code || o.entity_name || o.entity_label)) {
    d.entity = {
      type: o.entity_type,
      id: o.entity_id,
      code: o.entity_code,
      label: o.entity_label || o.entity_name,
    };
  }
  return d;
}

/** Formate une taille en octets en chaîne lisible en français (ex: "1,2 Mo"). */
function formatBytes(bytes) {
  if (bytes === null || bytes === undefined || isNaN(bytes)) return '—';
  const num = Number(bytes);
  if (num === 0) return '0 o';
  const k = 1024;
  const sizes = ['o', 'Ko', 'Mo', 'Go'];
  const i = Math.floor(Math.log(num) / Math.log(k));
  const val = (num / Math.pow(k, i)).toFixed(i === 0 ? 0 : 1).replace('.', ',');
  return `${val} ${sizes[i]}`;
}

/** Libellé convivial d'une catégorie de document. */
function formatDocCategory(cat) {
  switch (cat) {
    case 'notice': return 'Notice';
    case 'schéma':
    case 'schema': return 'Schéma';
    case 'certificat': return 'Certificat';
    case 'photo': return 'Photo';
    case 'rapport': return 'Rapport';
    case 'gamme': return 'Gamme';
    case 'autre': return 'Autre';
    default: return cat ? (cat.charAt(0).toUpperCase() + cat.slice(1)) : 'Non classé';
  }
}

/** Icône SVG selon le type MIME ou l'extension du document. */
function docFileIcon(mimeOrExt) {
  const str = String(mimeOrExt || '').toLowerCase();
  if (str.includes('pdf')) return 'i-doc';
  if (str.includes('image') || str.includes('jpg') || str.includes('jpeg') || str.includes('png') || str.includes('webp') || str.includes('gif')) return 'i-file-image';
  if (str.includes('word') || str.includes('doc') || str.includes('text') || str.includes('txt') || str.includes('odt')) return 'i-file-text';
  if (str.includes('excel') || str.includes('xls') || str.includes('sheet') || str.includes('csv') || str.includes('ods')) return 'i-chart';
  if (str.includes('dwg') || str.includes('dxf') || str.includes('step') || str.includes('stp') || str.includes('iges') || str.includes('igs')) return 'i-layers';
  return 'i-doc';
}

/** Le document peut-il être affiché en ligne dans un nouvel onglet (PDF ou image) ? */
function isDocInline(doc) {
  const v = doc && doc.current_version;
  if (!v) return false;
  const mime = String(v.mime_type || '').toLowerCase();
  const ext = String(v.extension || '').toLowerCase();
  if (mime.includes('pdf') || ext === '.pdf') return true;
  if (mime.startsWith('image/') || ['.jpg', '.jpeg', '.png', '.webp', '.gif'].includes(ext)) return true;
  return false;
}

/**
 * Charge une intervention avec ses pièces consommées et ses réservations, servies par le
 * serveur sur des routes dédiées (absentes sans la permission stock.view).
 */
async function fetchInterventionFull(id) {
  const base = `/api/interventions/${encodeURIComponent(id)}`;
  const inter = await api(base);
  if (hasPermission('stock.view')) {
    const [parts, reservations] = await Promise.all([
      api(`${base}/parts`).catch(() => []),
      api(`${base}/reservations`).catch(() => []),
    ]);
    inter.parts = (Array.isArray(parts) ? parts : []).map(adaptStock);
    inter.reservations = (Array.isArray(reservations) ? reservations : []).map(adaptStock);
  }
  return inter;
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

/** Phase actuelle du projet (Phase 7 : Pilotage et paramètres). */
const CURRENT_PHASE = 7;

/** Liste de tous les modules de l'application avec leur permission de vue requise. */
const MODULES = [
  { grp: 'Pilotage' },
  { id: 'tableau-de-bord', label: 'Tableau de bord', icon: 'i-dash', phase: 7, perm: 'dashboard.view' },
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

/** Domaines de permissions pour la matrice RBAC (39 permissions complètes de Permission::ALL). */
const PERMISSION_DOMAINS = [
  {
    name: 'Pilotage & Analyse',
    permissions: [
      { key: 'dashboard.view', label: 'Consulter le tableau de bord' },
      { key: 'dashboard.customize', label: 'Personnaliser son tableau de bord' },
      { key: 'dashboard.manage_defaults', label: 'Gérer les dispositions par défaut des rôles' },
      { key: 'reporting.view', label: 'Consulter les rapports et statistiques' },
      { key: 'exports.run', label: 'Exécuter des exports de données (CSV, Excel, PDF)' },
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
      { key: 'counters.record', label: 'Saisir un relevé de compteur' },
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
      { key: 'stock.adjust', label: 'Ajuster les stocks (inventaires et rebuts)' },
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
  currentEquipmentDetailId: null,

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

  // Pièces & Stock (Phase 4)
  parts: [],
  partsNextCursor: null,
  partsLoading: false,
  partsSearchQuery: '',
  partsCategoryFilter: '',
  partsSupplierFilter: '',
  partsLowStockFilter: false,
  partsLowStockCount: 0,
  stockActiveTab: 'catalog', // 'catalog' | 'stock' | 'movements' | 'alerts'

  stockLevels: [],
  stockLevelsLoading: false,
  stockLevelsLocationFilter: '',

  stockMovements: [],
  stockMovementsNextCursor: null,
  stockMovementsLoading: false,
  stockMovementsSearchQuery: '',
  stockMovementsTypeFilter: '',

  stockAlerts: [],
  stockAlertsLoading: false,

  currentPartDetail: null,
  currentPartLoading: false,

  // Maintenance préventive (Phase 5)
  maintenanceTab: 'plans', // 'plans' | 'schedule'
  maintenanceScheduleRange: 'month', // 'month' | '30' | '60' | '90'
  maintenanceScheduleMonth: localIso().slice(0, 7), // AAAA-MM
  maintenancePlans: [],
  maintenancePlansNextCursor: null,
  maintenancePlansLoading: false,
  maintenancePlansSearchQuery: '',
  maintenancePlansEquipmentFilter: '',
  maintenancePlansFrequencyFilter: '',
  maintenancePlansDueFilter: 'all', // 'all' | 'overdue' | 'soon'
  maintenancePlansActiveFilter: '',
  maintenanceScheduleEvents: [],
  maintenanceScheduleLoading: false,
  currentPlanDetail: null,
  currentPlanLoading: false,
  currentPlanOccurrences: [],
  currentPlanOccurrencesNextCursor: null,
  currentPlanOccurrencesLoading: false,

  // Vue Utilisateurs
  usersTab: 'users', // 'users' | 'permissions'
  usersSearchQuery: '',
  usersRoleFilter: '',
  usersSiteFilter: '',

  // Documents, QR code et Paramètres (Phase 6)
  selectedEquipmentIds: new Set(),
  documents: [],
  documentsNextCursor: null,
  documentsLoading: false,
  documentsSearchQuery: '',
  documentsEntityTypeFilter: '',
  documentsCategoryFilter: '',

  entityDocuments: new Map(),
  entityDocumentsLoading: new Set(),

  settingsTab: 'storage', // 'storage' | 'files' | 'labels'
  storageBackends: [],
  storageLoading: false,
  documentSettings: null,
  documentSettingsLoading: false,
  labelSettings: null,
  labelSettingsLoading: false,
  currentPrintEquipments: [],
  currentPrintOffset: 1,

  // État du serveur
  health: null,
  online: false,
  checkedAt: null,

  // Phase 7 : Pilotage, Dashboard, Reporting, Audit, Alertes & Paramètres
  branding: null,

  // Tableau de bord
  dashboardLayout: null,
  dashboardSource: 'role',
  dashboardCanCustomize: false,
  dashboardCustomizing: false,
  dashboardCatalog: [],
  dashboardWidgetData: new Map(),

  // Reporting
  reportingTab: 'activity',
  reportingPeriod: '30d',
  reportingFrom: '',
  reportingTo: '',
  reportingSiteId: '',
  reportingLineId: '',
  reportingEquipmentId: '',
  reportingType: '',
  reportingData: null,
  reportingLoading: false,

  // Journal d'audit
  auditItems: [],
  auditNextCursor: null,
  auditLoading: false,
  auditFrom: '',
  auditTo: '',
  auditSiteId: '',
  auditUserId: '',
  auditEntity: '',
  auditAction: '',
  auditEntityId: '',
  auditQuery: '',

  // Paramètres Phase 7
  settingsGeneral: null,
  settingsSecurity: null,
  settingsStock: null,
  settingsSmtp: null,
  settingsAlerts: null,
  settingsAudit: null,
  settingsOutbox: [],
  settingsDashboardRole: 'responsable',
  settingsDashboardRoleLayout: null,

  // Profil & Notifications
  profileNotifications: null,
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
  if (mod.id === 'stock') {
    return hasPermission('stock.view') || hasPermission('parts.view');
  }
  if (mod.perm && !hasPermission(mod.perm)) return false;
  return true;
}

// ============================================================================
// 7. GESTION DE L'AUTHENTIFICATION ET SESSIONS
// ============================================================================

/** Charge l'identité publique et l'état du mot de passe oublié. */
async function loadBranding() {
  if (state.branding) return state.branding;
  try {
    const b = await api('/api/public/branding');
    if (b) {
      state.branding = b;
      applyBranding(b);
    }
  } catch (_) {}
  return state.branding;
}

/** Applique le nom et le sous-titre de l'application à l'en-tête et au titre de la page. */
function applyBranding(b) {
  if (!b) return;
  const nameEl = document.getElementById('appName');
  if (nameEl && b.app_name) nameEl.textContent = b.app_name;
  const subEl = document.getElementById('appSubtitle');
  if (subEl && b.app_subtitle) subEl.textContent = b.app_subtitle;
  if (b.app_name) {
    document.title = `${b.app_name} — ${b.app_subtitle || 'GMAO'}`;
  }
}

/**
 * Vérifie l'état de la session au démarrage via GET /api/auth/me.
 */
async function checkAuth() {
  await loadBranding();
  if (currentRoute() === 'reinitialiser') {
    showResetPasswordScreen();
    return false;
  }
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

  const appName = (state.branding && state.branding.app_name) || 'Apex Maint';
  const appSub = (state.branding && state.branding.app_subtitle) || 'Gestion de Maintenance Assistée par Ordinateur';
  const showReset = Boolean(state.branding && state.branding.password_reset_available);

  card.innerHTML = `
    <div class="auth-header">
      <div class="logo">M</div>
      <h1>${esc(appName)}</h1>
      <p>${esc(appSub)}</p>
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
      ${showReset ? `
        <div class="auth-forgot">
          <a href="#/reinitialiser" class="link">Mot de passe oublié ?</a>
        </div>
      ` : ''}
    </form>
  `;

  overlay.hidden = false;
  setTimeout(() => {
    const u = document.getElementById('loginUsername');
    if (u) u.focus();
  }, 40);
}

/** Extrait le jeton de réinitialisation depuis l'ancre d'URL (#/reinitialiser?jeton=... ou token=...). */
function getResetTokenFromUrl() {
  const hash = window.location.hash || '';
  const qIdx = hash.indexOf('?');
  if (qIdx === -1) return '';
  const params = new URLSearchParams(hash.slice(qIdx));
  return params.get('jeton') || params.get('token') || '';
}

/** Affiche l'écran de mot de passe oublié ou de saisie du nouveau mot de passe. */
function showResetPasswordScreen() {
  const overlay = document.getElementById('authOverlay');
  const card = document.getElementById('authContent');
  if (!overlay || !card) return;

  const token = getResetTokenFromUrl();
  const appName = (state.branding && state.branding.app_name) || 'Apex Maint';

  if (token) {
    card.innerHTML = `
      <div class="auth-header">
        <div class="logo">${icon('i-key')}</div>
        <h1>Réinitialiser mon mot de passe</h1>
        <p>${esc(appName)}</p>
      </div>
      <form class="auth-form" data-action="reset-password-confirm-submit">
        <input type="hidden" name="token" value="${esc(token)}" />
        <div id="authErrorBox" class="auth-error" hidden>
          ${icon('i-alert')}
          <span id="authErrorText"></span>
        </div>
        <div class="form-group">
          <label for="resetNewPwd">Nouveau mot de passe</label>
          <input type="password" id="resetNewPwd" name="new_password" required minlength="8" autocomplete="new-password" autofocus />
          <span class="form-help">Minimum 8 caractères.</span>
        </div>
        <div class="form-group">
          <label for="resetConfirmPwd">Confirmer le nouveau mot de passe</label>
          <input type="password" id="resetConfirmPwd" name="confirm_password" required minlength="8" autocomplete="new-password" />
        </div>
        <button type="submit" id="resetSubmitBtn" class="btn primary">
          <span>Enregistrer le mot de passe</span>
        </button>
        <div class="auth-forgot">
          <a href="#/login" data-action="nav-login">Retour à la connexion</a>
        </div>
      </form>
    `;
  } else {
    card.innerHTML = `
      <div class="auth-header">
        <div class="logo">${icon('i-mail')}</div>
        <h1>Mot de passe oublié</h1>
        <p>Saisissez votre identifiant ou adresse e-mail pour recevoir un lien de réinitialisation.</p>
      </div>
      <form class="auth-form" data-action="reset-password-request-submit">
        <div id="authErrorBox" class="auth-error" hidden>
          ${icon('i-alert')}
          <span id="authErrorText"></span>
        </div>
        <div id="authSuccessBox" class="callout success" hidden>
          ${icon('i-check')}
          <span id="authSuccessText"></span>
        </div>
        <div class="form-group" id="resetIdentGroup">
          <label for="resetIdentifier">Identifiant ou adresse e-mail</label>
          <input type="text" id="resetIdentifier" name="identifier" required autocomplete="username" autofocus />
        </div>
        <button type="submit" id="resetRequestBtn" class="btn primary">
          <span>Envoyer le lien de réinitialisation</span>
        </button>
        <div class="auth-forgot">
          <a href="#/login" data-action="nav-login">Retour à la connexion</a>
        </div>
      </form>
    `;
  }

  overlay.hidden = false;
}

/** Demande de lien de réinitialisation. */
async function handleResetPasswordRequestSubmit(form) {
  const btn = document.getElementById('resetRequestBtn');
  const errorBox = document.getElementById('authErrorBox');
  const errorText = document.getElementById('authErrorText');
  const successBox = document.getElementById('authSuccessBox');
  const successText = document.getElementById('authSuccessText');
  const identGroup = document.getElementById('resetIdentGroup');

  const identifier = (form.identifier && form.identifier.value || '').trim();
  if (!identifier) return;

  btn.disabled = true;
  if (errorBox) errorBox.hidden = true;

  try {
    const res = await api('/api/auth/password-reset/request', {
      method: 'POST',
      body: { identifier },
    });
    btn.hidden = true;
    if (identGroup) identGroup.hidden = true;
    if (successBox) {
      successBox.hidden = false;
      successText.textContent = (res && res.message) || 'Si le compte le permet, un lien de réinitialisation vous sera envoyé.';
    }
  } catch (err) {
    btn.disabled = false;
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = err.message || 'Erreur lors de la demande.';
    }
  }
}

/** Confirmation du nouveau mot de passe avec le jeton. */
async function handleResetPasswordConfirmSubmit(form) {
  const btn = document.getElementById('resetSubmitBtn');
  const errorBox = document.getElementById('authErrorBox');
  const errorText = document.getElementById('authErrorText');

  const token = form.token ? form.token.value : '';
  const new_password = form.new_password ? form.new_password.value : '';
  const confirm_password = form.confirm_password ? form.confirm_password.value : '';

  if (new_password.length < 8) {
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = 'Le mot de passe doit comporter au moins 8 caractères.';
    }
    return;
  }
  if (new_password !== confirm_password) {
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = 'La confirmation ne correspond pas au nouveau mot de passe.';
    }
    return;
  }

  btn.disabled = true;
  if (errorBox) errorBox.hidden = true;

  try {
    await api('/api/auth/password-reset/confirm', {
      method: 'POST',
      body: { token, new_password },
    });
    notify('Mot de passe réinitialisé avec succès. Veuillez vous connecter.', 'success');
    window.location.hash = '#/tableau-de-bord';
    showLoginScreen();
  } catch (err) {
    btn.disabled = false;
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = err.message || 'Jeton invalide ou expiré.';
    }
  }
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
  const queryStr = raw.includes('?') ? raw.slice(raw.indexOf('?') + 1) : '';
  if (path === 'profil') return { route: 'profil', id: null, query: queryStr };
  if (path === 'etiquettes') return { route: 'etiquettes', id: null, query: queryStr };
  const parts = path.split('/');
  let root = parts[0];
  if (root === 'pieces') root = 'stock';
  const id = parts.length > 1 ? parts.slice(1).join('/') : null;
  const mod = MODULES.find(m => m.id === root);
  return {
    route: mod ? mod.id : DEFAULT_ROUTE,
    id: id || null,
    query: queryStr,
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
    await initProfilePageView();
    return;
  }

  // Route spéciale : Impression d'étiquettes
  if (route === 'etiquettes') {
    if (!hasPermission('equipments.view')) {
      document.title = 'Accès refusé — Apex Maint';
      view.innerHTML = accessDeniedPage();
      return;
    }
    document.title = 'Étiquettes QR code — Apex Maint';
    view.innerHTML = labelsPrintPage();
    await initLabelsPrintPageView();
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
    await initDashboardPageView();
  } else if (route === 'reporting') {
    view.innerHTML = reportingPage();
    await initReportingPageView();
  } else if (route === 'audit') {
    view.innerHTML = auditPage();
    await initAuditPageView();
  } else if (route === 'interventions') {
    if (id) {
      view.innerHTML = interventionDetailPage(id);
      await initInterventionDetailPageView(id);
    } else {
      view.innerHTML = interventionsPage();
      await initInterventionsPageView();
    }
  } else if (route === 'stock') {
    if (id) {
      view.innerHTML = partDetailPage(id);
      await initPartDetailPageView(id);
    } else {
      view.innerHTML = stockPage();
      await initStockPageView();
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
  } else if (route === 'maintenance-preventive') {
    if (id) {
      view.innerHTML = maintenancePlanDetailPage(id);
      await initMaintenancePlanDetailPageView(id);
    } else {
      view.innerHTML = maintenancePage();
      await initMaintenancePageView();
    }
  } else if (route === 'documents') {
    view.innerHTML = documentsPage();
    await initDocumentsPageView();
  } else if (route === 'parametres') {
    view.innerHTML = settingsPage();
    await initSettingsPageView();
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

// ============================================================================
// 10A. BIBLIOTHÈQUE DE GRAPHIQUES SVG ACCESSIBLES (PHASE 7)
// ============================================================================

/**
 * Rendu d'un graphique à barres horizontales SVG accessible.
 * Utilise les variables CSS du thème actif (clair/sombre) sans style en ligne.
 */
function renderSvgBarChart({ items = [], max = null, height = 180, unit = '' }) {
  if (!items.length) {
    return '<p class="hint">Aucune donnée à afficher.</p>';
  }
  const calcMax = max !== null ? max : Math.max(...items.map(it => it.value || 0), 1);
  const rowHeight = 28;
  const svgHeight = Math.max(height, items.length * rowHeight + 20);
  const chartWidth = 480;
  const labelWidth = 130;
  const barAreaWidth = chartWidth - labelWidth - 60;

  const bars = items.map((it, idx) => {
    const y = idx * rowHeight + 10;
    const val = it.value || 0;
    const barW = Math.max(2, Math.round((val / calcMax) * barAreaWidth));
    const fill = it.color || 'var(--accent, #3b82f6)';
    const textVal = `${val}${unit ? ' ' + unit : ''}`;
    return `
      <g class="chart-bar-group" role="row">
        <text x="${labelWidth - 8}" y="${y + 15}" text-anchor="end" fill="var(--text)" font-size="12">${esc(it.label)}</text>
        <rect x="${labelWidth}" y="${y + 2}" width="${barW}" height="18" rx="4" fill="${fill}"></rect>
        <text x="${labelWidth + barW + 8}" y="${y + 15}" fill="var(--muted)" font-size="12" font-weight="600">${esc(textVal)}</text>
      </g>
    `;
  }).join('');

  const tableRows = items.map(it => `
    <tr>
      <td>${esc(it.label)}</td>
      <td class="mono font-bold">${esc(it.value)}${unit ? ' ' + esc(unit) : ''}</td>
    </tr>
  `).join('');

  return `
    <div class="svg-chart-wrap">
      <svg class="svg-chart" viewBox="0 0 ${chartWidth} ${svgHeight}" role="img" aria-label="Graphique en barres" height="${svgHeight}">
        ${bars}
      </svg>
      <details class="chart-details">
        <summary class="hint link-inline">Afficher le tableau de données</summary>
        <table class="chart-accessible-table">
          <thead><tr><th>Catégorie</th><th>Valeur</th></tr></thead>
          <tbody>${tableRows}</tbody>
        </table>
      </details>
    </div>
  `;
}

/**
 * Rendu d'un graphique en anneau (donut) SVG accessible avec légende et tableau équivalent.
 */
function renderSvgDonutChart({ slices = [], size = 180, donutWidth = 24, title = '' }) {
  if (!slices.length || slices.every(s => !s.value)) {
    return '<p class="hint">Aucune donnée à afficher.</p>';
  }
  const total = slices.reduce((acc, s) => acc + (s.value || 0), 0);
  const cx = size / 2;
  const cy = size / 2;
  const radius = (size / 2) - 12;
  const innerRadius = radius - donutWidth;

  let currentAngle = -90; // Démarre à 12h
  const defaultColors = [
    'var(--accent)',
    'var(--ok-line, #10b981)',
    'var(--warn, #f59e0b)',
    'var(--danger, #ef4444)',
    '#8b5cf6',
    '#ec4899',
    '#06b6d4',
  ];

  const svgPaths = [];
  const legendItems = [];
  const tableRows = [];

  slices.forEach((s, idx) => {
    const val = s.value || 0;
    const color = s.color || defaultColors[idx % defaultColors.length];
    const pct = total > 0 ? (val / total) : 0;
    const angle = pct * 360;

    if (val > 0) {
      const startAngleRad = (currentAngle * Math.PI) / 180;
      const endAngleRad = ((currentAngle + angle) * Math.PI) / 180;
      const x1 = cx + radius * Math.cos(startAngleRad);
      const y1 = cy + radius * Math.sin(startAngleRad);
      const x2 = cx + radius * Math.cos(endAngleRad);
      const y2 = cy + radius * Math.sin(endAngleRad);
      const x3 = cx + innerRadius * Math.cos(endAngleRad);
      const y3 = cy + innerRadius * Math.sin(endAngleRad);
      const x4 = cx + innerRadius * Math.cos(startAngleRad);
      const y4 = cy + innerRadius * Math.sin(startAngleRad);
      const largeArc = angle > 180 ? 1 : 0;

      const pathData = total === val
        ? `M ${cx} ${cy - radius} A ${radius} ${radius} 0 1 1 ${cx - 0.01} ${cy - radius} L ${cx - 0.01} ${cy - innerRadius} A ${innerRadius} ${innerRadius} 0 1 0 ${cx} ${cy - innerRadius} Z`
        : `M ${x1} ${y1} A ${radius} ${radius} 0 ${largeArc} 1 ${x2} ${y2} L ${x3} ${y3} A ${innerRadius} ${innerRadius} 0 ${largeArc} 0 ${x4} ${y4} Z`;

      svgPaths.push(`<path d="${pathData}" fill="${color}"><title>${esc(s.label)}: ${val} (${Math.round(pct * 100)}%)</title></path>`);
    }

    currentAngle += angle;

    legendItems.push(`
      <div class="chart-legend-item">
        <svg width="12" height="12" viewBox="0 0 12 12"><rect width="12" height="12" rx="3" fill="${color}"></rect></svg>
        <span>${esc(s.label)} (${val})</span>
      </div>
    `);

    tableRows.push(`
      <tr>
        <td>${esc(s.label)}</td>
        <td class="mono font-bold">${val}</td>
        <td class="mono">${Math.round(pct * 100)}%</td>
      </tr>
    `);
  });

  return `
    <div class="svg-chart-wrap">
      <div class="donut-chart-flex">
        <svg class="svg-chart" viewBox="0 0 ${size} ${size}" width="${size}" height="${size}" role="img" aria-label="Graphique en anneau">
          ${svgPaths.join('')}
          <text x="${cx}" y="${cy - 2}" text-anchor="middle" font-size="20" font-weight="700" fill="var(--text)">${total}</text>
          <text x="${cx}" y="${cy + 16}" text-anchor="middle" font-size="11" fill="var(--muted)">${esc(title || 'Total')}</text>
        </svg>
        <div class="chart-legend">
          ${legendItems.join('')}
        </div>
      </div>
      <details class="chart-details">
        <summary class="hint link-inline">Afficher le tableau de données</summary>
        <table class="chart-accessible-table">
          <thead><tr><th>Catégorie</th><th>Nombre</th><th>Part</th></tr></thead>
          <tbody>${tableRows.join('')}</tbody>
        </table>
      </details>
    </div>
  `;
}

/**
 * Rendu d'une jauge semi-circulaire SVG (taux en pourcentage).
 */
function renderSvgGauge({ percent = 0, label = '', valueText = '', color = 'var(--accent)', size = 180 }) {
  const clamped = Math.max(0, Math.min(100, Math.round(percent)));
  const cx = size / 2;
  const cy = size / 2 + 10;
  const radius = size * 0.38;
  const strokeWidth = size * 0.12;

  const startRad = Math.PI;
  const endRad = 2 * Math.PI;
  const activeRad = Math.PI + (clamped / 100) * Math.PI;

  const x0 = cx + radius * Math.cos(startRad);
  const y0 = cy + radius * Math.sin(startRad);
  const x1 = cx + radius * Math.cos(endRad);
  const y1 = cy + radius * Math.sin(endRad);
  const xActive = cx + radius * Math.cos(activeRad);
  const yActive = cy + radius * Math.sin(activeRad);

  const bgPath = `M ${x0} ${y0} A ${radius} ${radius} 0 0 1 ${x1} ${y1}`;
  const activePath = clamped > 0
    ? `M ${x0} ${y0} A ${radius} ${radius} 0 0 1 ${xActive} ${yActive}`
    : '';

  const displayVal = valueText || `${clamped} %`;

  return `
    <div class="svg-chart-wrap svg-gauge-wrap">
      <svg class="svg-chart" viewBox="0 0 ${size} ${size * 0.7}" width="${size}" height="${size * 0.7}" role="img" aria-label="Jauge: ${esc(label)} ${esc(displayVal)}">
        <path d="${bgPath}" fill="none" stroke="var(--line-2)" stroke-width="${strokeWidth}" stroke-linecap="round"></path>
        ${activePath ? `<path d="${activePath}" fill="none" stroke="${color}" stroke-width="${strokeWidth}" stroke-linecap="round"></path>` : ''}
        <text x="${cx}" y="${cy - 10}" text-anchor="middle" font-size="24" font-weight="700" fill="var(--text)">${esc(displayVal)}</text>
        <text x="${cx}" y="${cy + 10}" text-anchor="middle" font-size="11" fill="var(--muted)">${esc(label)}</text>
      </svg>
      <div class="sr-only">Taux: ${esc(displayVal)} (${esc(label)})</div>
    </div>
  `;
}

// ============================================================================
// 10B. PAGE TABLEAU DE BORD (PHASE 7)
// ============================================================================

const DASHBOARD_TITLES = {
  interventions_summary: 'Synthèse des interventions',
  interventions_by_type: 'Répartition par type',
  interventions_by_priority: 'Répartition par priorité',
  response_time: 'Temps de prise en charge',
  mtbf_mttr: 'Indicateurs MTBF / MTTR',
  downtime: "Temps d'arrêt",
  preventive_compliance: 'Respect des préventifs',
  parts_cost: 'Coût des pièces',
  labor_time: 'Temps de main d’œuvre',
  stock_value: 'Valorisation du stock',
  stock_alerts: 'Alertes de stock',
  my_interventions: 'Mes interventions',
  machines_down: 'Équipements à l’arrêt',
  maintenance_overdue: 'Maintenances en retard',
};

function dashboardPage() {
  if (!hasPermission('dashboard.view')) {
    return accessDeniedPage();
  }

  const isCust = state.dashboardCustomizing;
  const canCust = state.dashboardCanCustomize;
  const source = state.dashboardSource;
  const sourceLabel = source === 'user' ? 'Personnalisé' : (source === 'role' ? 'Rôle' : 'Standard');

  return `
    <div class="page-head">
      <div class="page-head-title">
        ${icon('i-dash')}
        <div>
          <h1>Tableau de bord</h1>
          <p class="page-subtitle">Indicateurs clés, activités et surveillance des équipements.</p>
        </div>
      </div>
      <div class="page-head-actions">
        <span class="badge" title="Source de la disposition actuelle">${esc(sourceLabel)}</span>
        ${canCust ? `
          <button type="button" class="btn ${isCust ? 'secondary' : 'primary'}" data-action="toggle-dashboard-customize">
            ${icon(isCust ? 'i-check' : 'i-sliders')}
            <span>${isCust ? 'Terminer' : 'Personnaliser'}</span>
          </button>
        ` : ''}
      </div>
    </div>

    ${isCust ? `
      <div class="dashboard-custom-banner">
        <div>
          <b>Mode personnalisation actif</b>
          <p class="hint">Glissez-déposez ou utilisez les commandes des cartes pour déplacer, redimensionner ou configurer les widgets.</p>
        </div>
        <div class="dashboard-custom-actions">
          <button type="button" class="btn primary" data-action="open-add-widget-modal">
            ${icon('i-plus')} <span>Ajouter un widget</span>
          </button>
          <button type="button" class="btn" data-action="reset-dashboard-layout" title="Rétablir la disposition par défaut de votre rôle">
            ${icon('i-refresh')} <span>Rétablir par défaut</span>
          </button>
          <button type="button" class="btn primary" data-action="save-dashboard-layout">
            ${icon('i-check')} <span>Enregistrer</span>
          </button>
        </div>
      </div>
    ` : ''}

    <div class="dashboard-grid" id="dashboardGrid">
      <p class="hint">${icon('i-clock')} Chargement du tableau de bord…</p>
    </div>
  `;
}

async function initDashboardPageView() {
  await loadDashboard();
}

async function loadDashboard() {
  const container = document.getElementById('dashboardGrid');
  if (!container) return;

  try {
    const layout = await api('/api/dashboard');
    state.dashboardLayout = layout;
    state.dashboardSource = layout.source || 'role';
    state.dashboardCanCustomize = Boolean(layout.can_customize);
    // L'en-tête a été rendu avant la réponse : on le redessine avec la source et le droit réels.
    const head = document.querySelector('.page-head');
    if (head) {
      const tmp = document.createElement('div');
      tmp.innerHTML = dashboardPage();
      const freshHead = tmp.querySelector('.page-head');
      if (freshHead) head.replaceWith(freshHead);
    }
    renderDashboardGrid();

    // Chargement parallèle des données de chaque widget
    const widgets = (layout && Array.isArray(layout.widgets)) ? layout.widgets : [];
    await Promise.allSettled(widgets.map(w => loadDashboardWidgetData(w)));
  } catch (err) {
    if (container) {
      container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur lors du chargement du tableau de bord : ${esc(err.message)}</p>`;
    }
  }
}

async function loadDashboardWidgetData(w) {
  try {
    const params = w.params || {};
    const query = new URLSearchParams();
    if (params.period) query.set('period', params.period);
    if (params.site_id) query.set('site_id', params.site_id);
    if (params.line_id) query.set('line_id', params.line_id);
    if (params.equipment_id) query.set('equipment_id', params.equipment_id);
    if (params.from) query.set('from', params.from);
    if (params.to) query.set('to', params.to);

    const qs = query.toString() ? `?${query.toString()}` : '';
    const res = await api(`/api/dashboard/widgets/${encodeURIComponent(w.widget)}${qs}`);
    state.dashboardWidgetData.set(w.id, res ? res.data : null);

    const cardBody = document.getElementById(`widget-body-${w.id}`);
    if (cardBody) {
      cardBody.innerHTML = renderWidgetContent(w, res ? res.data : null);
    }
  } catch (err) {
    state.dashboardWidgetData.set(w.id, { _error: err.message });
    const cardBody = document.getElementById(`widget-body-${w.id}`);
    if (cardBody) {
      cardBody.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur : ${esc(err.message)}</p>`;
    }
  }
}

function renderDashboardGrid() {
  const container = document.getElementById('dashboardGrid');
  if (!container) return;

  const widgets = (state.dashboardLayout && Array.isArray(state.dashboardLayout.widgets))
    ? state.dashboardLayout.widgets
    : [];

  if (!widgets.length) {
    container.innerHTML = `
      <div class="empty-state">
        ${icon('i-dash')}
        <p>Aucun widget configuré pour votre tableau de bord.</p>
        ${state.dashboardCanCustomize ? `
          <button type="button" class="btn primary" data-action="open-add-widget-modal">
            ${icon('i-plus')} <span>Ajouter un widget</span>
          </button>
        ` : ''}
      </div>
    `;
    return;
  }

  container.innerHTML = widgets.map(w => renderWidgetCard(w, state.dashboardCustomizing)).join('');
}

function renderWidgetCard(w, isCustomize) {
  const title = DASHBOARD_TITLES[w.widget] || w.widget;
  const colSpan = Math.max(1, Math.min(12, w.w || 6));
  const data = state.dashboardWidgetData.get(w.id);

  let bodyHtml = `<p class="hint">${icon('i-clock')} Chargement…</p>`;
  if (data !== undefined) {
    if (data && data._error) {
      bodyHtml = `<p class="hint state-ko">${icon('i-alert')} ${esc(data._error)}</p>`;
    } else {
      bodyHtml = renderWidgetContent(w, data);
    }
  }

  return `
    <div class="dashboard-widget w-col-${colSpan} ${isCustomize ? 'customizing' : ''}" id="widget-card-${esc(w.id)}" data-widget-id="${esc(w.id)}">
      <div class="widget-head">
        <h3>
          ${isCustomize ? `
            <span class="widget-drag-handle" draggable="true" data-action="widget-drag-handle" data-args="${jsArg(w.id)}" title="Glisser pour réorganiser">
              ${icon('i-grip')}
            </span>
          ` : ''}
          <span>${esc(title)}</span>
        </h3>
        ${isCustomize ? `
          <div class="widget-custom-controls">
            <button type="button" class="btn sm" data-action="move-widget-prev" data-args="${jsArg(w.id)}" title="Déplacer vers la gauche / haut">
              ${icon('i-arrow-up')}
            </button>
            <button type="button" class="btn sm" data-action="move-widget-next" data-args="${jsArg(w.id)}" title="Déplacer vers la droite / bas">
              ${icon('i-arrow-down')}
            </button>
            <button type="button" class="btn sm" data-action="cycle-widget-size" data-args="${jsArg(w.id)}" title="Changer la largeur">
              <span>${colSpan} col</span>
            </button>
            <button type="button" class="btn sm" data-action="open-widget-config" data-args="${jsArg(w.id)}" title="Paramètres du widget">
              ${icon('i-sliders')}
            </button>
            <button type="button" class="btn sm danger" data-action="delete-dashboard-widget" data-args="${jsArg(w.id)}" title="Supprimer le widget">
              ${icon('i-trash')}
            </button>
          </div>
        ` : ''}
      </div>
      <div class="widget-body" id="widget-body-${esc(w.id)}">
        ${bodyHtml}
      </div>
    </div>
  `;
}

function renderWidgetContent(w, data) {
  if (!data) return '<p class="hint">Aucune donnée disponible.</p>';

  switch (w.widget) {
    case 'interventions_summary': {
      const items = Array.isArray(data.items) ? data.items : [];
      const openCount = (items.find(it => it.key === 'open') || {}).count || 0;
      const inProgCount = (items.find(it => it.key === 'in_progress') || {}).count || 0;
      const compCount = (items.find(it => it.key === 'completed') || {}).count || 0;

      const chartItems = items.map(it => ({
        label: formatInterventionStatus(it.key),
        value: it.count || 0,
        color: it.key === 'open' ? 'var(--warn)' : (it.key === 'in_progress' ? 'var(--accent)' : 'var(--ok-line)'),
      }));

      return `
        <div class="kpi-row">
          <div class="kpi-card">
            <span class="kpi-label">Ouvertes</span>
            <span class="kpi-val text-warn">${openCount}</span>
          </div>
          <div class="kpi-card">
            <span class="kpi-label">En cours</span>
            <span class="kpi-val text-accent">${inProgCount}</span>
          </div>
          <div class="kpi-card">
            <span class="kpi-label">Clôturées</span>
            <span class="kpi-val text-ok">${compCount}</span>
          </div>
        </div>
        ${renderSvgBarChart({ items: chartItems, height: 120 })}
      `;
    }

    case 'interventions_by_type': {
      const items = Array.isArray(data.items) ? data.items : [];
      const slices = items.map(it => ({
        label: formatInterventionType(it.key),
        value: it.count || 0,
      }));
      return renderSvgDonutChart({ slices, size: 160, title: 'Interventions' });
    }

    case 'interventions_by_priority': {
      const items = Array.isArray(data.items) ? data.items : [];
      const priorityColors = {
        low: 'var(--muted)',
        medium: 'var(--accent)',
        high: 'var(--warn)',
        critical: 'var(--danger)',
      };
      const slices = items.map(it => ({
        label: formatInterventionPriority(it.key),
        value: it.count || 0,
        color: priorityColors[it.key] || 'var(--accent)',
      }));
      return renderSvgDonutChart({ slices, size: 160, title: 'Priorités' });
    }

    case 'response_time': {
      const meanMin = Math.round(data.mean_minutes || 0);
      const medMin = Math.round(data.median_minutes || 0);
      return `
        <div class="kpi-row">
          <div class="kpi-card">
            <span class="kpi-label">Délai moyen de prise en charge</span>
            <span class="kpi-val">${meanMin} <small class="hint">min</small></span>
            <span class="kpi-sub">${Math.round(meanMin / 60)} h</span>
          </div>
          <div class="kpi-card">
            <span class="kpi-label">Délai médian</span>
            <span class="kpi-val">${medMin} <small class="hint">min</small></span>
            <span class="kpi-sub">${Math.round(medMin / 60)} h</span>
          </div>
        </div>
      `;
    }

    case 'mtbf_mttr': {
      if (data.no_failures) {
        return '<p class="hint">Aucune panne constatée sur la période sélectionnée.</p>';
      }
      const mtbfH = Math.round((data.mtbf_minutes || 0) / 60);
      const mttrMin = Math.round(data.mttr_minutes || 0);
      return `
        <div class="kpi-row">
          <div class="kpi-card">
            <span class="kpi-label">MTBF (Temps moyen entre pannes)</span>
            <span class="kpi-val">${mtbfH} <small class="hint">h</small></span>
          </div>
          <div class="kpi-card">
            <span class="kpi-label">MTTR (Temps moyen de réparation)</span>
            <span class="kpi-val">${mttrMin} <small class="hint">min</small></span>
          </div>
          <div class="kpi-card">
            <span class="kpi-label">Pannes constatées</span>
            <span class="kpi-val">${data.failures || 0}</span>
          </div>
        </div>
      `;
    }

    case 'downtime': {
      const totalMin = Math.round(data.downtime_minutes || 0);
      const hours = Math.floor(totalMin / 60);
      const mins = totalMin % 60;
      return `
        <div class="kpi-row">
          <div class="kpi-card">
            <span class="kpi-label">Temps d'arrêt total</span>
            <span class="kpi-val">${hours} h ${mins} min</span>
            <span class="kpi-sub">${totalMin} minutes cumulées</span>
          </div>
          <div class="kpi-card">
            <span class="kpi-label">Pannes totales</span>
            <span class="kpi-val">${data.failures || 0}</span>
            <span class="kpi-sub">${data.closed_failures || 0} clôturées</span>
          </div>
        </div>
      `;
    }

    case 'preventive_compliance': {
      const pct = data.compliance_percent || 0;
      const color = pct >= 80 ? 'var(--ok-line)' : (pct >= 50 ? 'var(--warn)' : 'var(--danger)');
      return `
        <div class="preventive-gauge-row">
          ${renderSvgGauge({ percent: pct, label: 'Respect des échéances', color, size: 160 })}
          <div class="kpi-row">
            <div class="kpi-card">
              <span class="kpi-label">Échéances dues</span>
              <span class="kpi-val">${data.due_count || 0}</span>
            </div>
            <div class="kpi-card">
              <span class="kpi-label">À l'heure</span>
              <span class="kpi-val text-ok">${data.on_time_count || 0}</span>
            </div>
          </div>
        </div>
      `;
    }

    case 'parts_cost': {
      const totalEuros = formatCents(data.parts_cost_cents || 0);
      const items = Array.isArray(data.by_equipment) ? data.by_equipment.slice(0, 5) : [];
      return `
        <div class="kpi-row">
          <div class="kpi-card">
            <span class="kpi-label">Coût total des pièces</span>
            <span class="kpi-val text-accent">${totalEuros}</span>
          </div>
        </div>
        ${items.length ? `
          <table class="data-table">
            <thead><tr><th>Équipement</th><th>Coût (€)</th></tr></thead>
            <tbody>
              ${items.map(it => `
                <tr>
                  <td class="font-bold" title="${esc(it.equipment_name || '')}">${esc(it.equipment_code || (it.equipment_id ? it.equipment_id.slice(0, 8) : 'Hors équipement'))}</td>
                  <td class="mono">${formatCents(it.cost_cents)}</td>
                </tr>
              `).join('')}
            </tbody>
          </table>
        ` : ''}
      `;
    }

    case 'labor_time': {
      const list = Array.isArray(data.labor) ? data.labor : [];
      const totalMin = list.reduce((acc, it) => acc + (it.duration_minutes || 0), 0);
      return `
        <div class="kpi-row">
          <div class="kpi-card">
            <span class="kpi-label">Main d'œuvre totale</span>
            <span class="kpi-val">${Math.round(totalMin / 60)} <small class="hint">h</small></span>
            <span class="kpi-sub">${totalMin} minutes</span>
          </div>
          <div class="kpi-card">
            <span class="kpi-label">Lignes de saisie</span>
            <span class="kpi-val">${list.length}</span>
          </div>
        </div>
      `;
    }

    case 'stock_value': {
      const valStr = formatCents(data.value_cents || 0);
      const items = Array.isArray(data.items) ? data.items : [];
      return `
        <div class="kpi-row">
          <div class="kpi-card">
            <span class="kpi-label">Valeur totale du stock</span>
            <span class="kpi-val text-accent">${valStr}</span>
          </div>
          <div class="kpi-card">
            <span class="kpi-label">Références en magasin</span>
            <span class="kpi-val">${items.length}</span>
          </div>
        </div>
      `;
    }

    case 'stock_alerts': {
      const items = Array.isArray(data.items) ? data.items : [];
      const lowItems = items.filter(it => it.low);
      return `
        <div class="kpi-row">
          <div class="kpi-card">
            <span class="kpi-label">Articles en alerte</span>
            <span class="kpi-val ${lowItems.length ? 'text-danger' : 'text-ok'}">${lowItems.length}</span>
          </div>
        </div>
        ${lowItems.length ? `
          <div class="table-container">
            <table class="data-table">
              <thead><tr><th>Réf</th><th>Désignation</th><th>Qté</th><th>Seuil</th></tr></thead>
              <tbody>
                ${lowItems.slice(0, 5).map(it => `
                  <tr>
                    <td class="mono font-bold">${esc(it.reference)}</td>
                    <td>${esc(it.name)}</td>
                    <td class="mono text-danger font-bold">${it.quantity}</td>
                    <td class="mono">${it.threshold}</td>
                  </tr>
                `).join('')}
              </tbody>
            </table>
          </div>
        ` : '<p class="hint">Aucun article sous le seuil d\'alerte.</p>'}
      `;
    }

    case 'my_interventions': {
      const items = Array.isArray(data.items) ? data.items : [];
      if (!items.length) {
        return '<p class="hint">Aucune intervention assignée en cours.</p>';
      }
      return `
        <div class="table-container">
          <table class="data-table">
            <thead><tr><th>Code</th><th>Titre</th><th>Priorité</th><th>Statut</th></tr></thead>
            <tbody>
              ${items.slice(0, 6).map(it => `
                <tr>
                  <td><a href="#/interventions/${esc(it.id)}" class="mono font-bold link">${esc(it.code)}</a></td>
                  <td>${esc(it.title)}</td>
                  <td>${interventionPriorityBadge(it.priority)}</td>
                  <td>${interventionStatusBadge(it.status)}</td>
                </tr>
              `).join('')}
            </tbody>
          </table>
        </div>
      `;
    }

    case 'machines_down': {
      const items = Array.isArray(data.items) ? data.items : [];
      if (!items.length) {
        return '<p class="hint text-ok">Aucun équipement à l\'arrêt actuellement.</p>';
      }
      return `
        <div class="table-container">
          <table class="data-table">
            <thead><tr><th>Code</th><th>Nom</th><th>État</th></tr></thead>
            <tbody>
              ${items.map(it => `
                <tr>
                  <td><a href="#/equipements" class="mono font-bold link">${esc(it.code)}</a></td>
                  <td>${esc(it.name)}</td>
                  <td><span class="badge badge-state-ko">À l'arrêt</span></td>
                </tr>
              `).join('')}
            </tbody>
          </table>
        </div>
      `;
    }

    case 'maintenance_overdue': {
      const items = Array.isArray(data.items) ? data.items : [];
      if (!items.length) {
        return '<p class="hint text-ok">Aucune maintenance préventive en retard.</p>';
      }
      return `
        <div class="table-container">
          <table class="data-table">
            <thead><tr><th>Code</th><th>Titre</th><th>Échéance</th></tr></thead>
            <tbody>
              ${items.map(it => `
                <tr>
                  <td><a href="#/maintenance-preventive/${esc(it.id)}" class="mono font-bold link">${esc(it.code)}</a></td>
                  <td>${esc(it.title)}</td>
                  <td class="mono text-danger">${it.next_due_date ? esc(formatDate(it.next_due_date)) : '—'}</td>
                </tr>
              `).join('')}
            </tbody>
          </table>
        </div>
      `;
    }

    default:
      return '<p class="hint">Données reçues.</p>';
  }
}

// Actions de personnalisation du tableau de bord

function toggleDashboardCustomize(enable) {
  state.dashboardCustomizing = enable !== undefined ? enable : !state.dashboardCustomizing;
  const view = document.getElementById('view');
  if (view) {
    view.innerHTML = dashboardPage();
    renderDashboardGrid();
  }
}

function moveDashboardWidget(widgetId, dir) {
  const widgets = (state.dashboardLayout && state.dashboardLayout.widgets) || [];
  const idx = widgets.findIndex(w => w.id === widgetId);
  if (idx === -1) return;

  const targetIdx = dir === 'prev' ? idx - 1 : idx + 1;
  if (targetIdx < 0 || targetIdx >= widgets.length) return;

  const tmp = widgets[idx];
  widgets[idx] = widgets[targetIdx];
  widgets[targetIdx] = tmp;

  renderDashboardGrid();
}

function cycleDashboardWidgetSize(widgetId) {
  const widgets = (state.dashboardLayout && state.dashboardLayout.widgets) || [];
  const w = widgets.find(it => it.id === widgetId);
  if (!w) return;

  const allowed = [4, 6, 8, 12];
  const curIdx = allowed.indexOf(w.w || 6);
  w.w = allowed[(curIdx + 1) % allowed.length];

  renderDashboardGrid();
}

function openDashboardWidgetConfigModal(widgetId) {
  const widgets = (state.dashboardLayout && state.dashboardLayout.widgets) || [];
  const w = widgets.find(it => it.id === widgetId);
  if (!w) return;

  const p = w.params || {};
  const isAdm = isAdmin();

  const bodyHtml = `
    <form class="form-grid" id="modalWidgetConfigForm" data-action="save-widget-config-submit" data-args="${jsArg(widgetId)}">
      <div class="form-group">
        <label for="wCfgPeriod">Période d'analyse</label>
        <select id="wCfgPeriod" name="period">
          <option value="7d" ${p.period === '7d' ? 'selected' : ''}>7 derniers jours</option>
          <option value="30d" ${!p.period || p.period === '30d' ? 'selected' : ''}>30 derniers jours</option>
          <option value="90d" ${p.period === '90d' ? 'selected' : ''}>90 derniers jours</option>
          <option value="12m" ${p.period === '12m' ? 'selected' : ''}>12 derniers mois</option>
          <option value="month" ${p.period === 'month' ? 'selected' : ''}>Mois civil en cours</option>
          <option value="year" ${p.period === 'year' ? 'selected' : ''}>Année civile en cours</option>
        </select>
      </div>
      ${isAdm ? `
        <div class="form-group">
          <label for="wCfgSite">Filtrer par site</label>
          <select id="wCfgSite" name="site_id">
            <option value="">Tous les sites</option>
            ${state.sites.map(s => `<option value="${esc(s.id)}" ${p.site_id === s.id ? 'selected' : ''}>${esc(s.name)}</option>`).join('')}
          </select>
        </div>
      ` : ''}
    </form>
  `;

  const footerHtml = `
    <button type="button" class="btn" data-action="close-modal">Annuler</button>
    <button type="submit" form="modalWidgetConfigForm" class="btn primary">Enregistrer les paramètres</button>
  `;

  openModal(`Configurer : ${DASHBOARD_TITLES[w.widget] || w.widget}`, bodyHtml, footerHtml);
}

function handleSaveWidgetConfigSubmit(widgetId, form) {
  const widgets = (state.dashboardLayout && state.dashboardLayout.widgets) || [];
  const w = widgets.find(it => it.id === widgetId);
  if (!w) return;

  w.params = w.params || {};
  w.params.period = form.period ? form.period.value : '30d';
  if (form.site_id) {
    w.params.site_id = form.site_id.value || null;
  }

  closeModal();
  loadDashboardWidgetData(w);
}

function removeDashboardWidget(widgetId) {
  if (!state.dashboardLayout || !state.dashboardLayout.widgets) return;
  state.dashboardLayout.widgets = state.dashboardLayout.widgets.filter(w => w.id !== widgetId);
  renderDashboardGrid();
}

async function openAddDashboardWidgetModal() {
  try {
    if (!state.dashboardCatalog.length) {
      const cat = await api('/api/dashboard/catalog');
      state.dashboardCatalog = Array.isArray(cat) ? cat : [];
    }

    const cards = state.dashboardCatalog.map(it => `
      <div class="widget-catalog-card">
        <div>
          <h4>${esc(it.title || it.widget)}</h4>
          <p class="hint">${esc(it.widget)}</p>
        </div>
        <button type="button" class="btn primary sm" data-action="add-widget-from-catalog" data-args="${jsArg(it.widget)}">
          ${icon('i-plus')} <span>Ajouter au tableau</span>
        </button>
      </div>
    `).join('');

    const bodyHtml = `
      <div class="widget-catalog-grid">
        ${cards || '<p class="hint">Aucun widget disponible dans le catalogue.</p>'}
      </div>
    `;

    openModal('Catalogue des widgets', bodyHtml, '<button type="button" class="btn" data-action="close-modal">Fermer</button>');
  } catch (err) {
    notify(`Impossible de charger le catalogue : ${err.message}`, 'error');
  }
}

function addWidgetFromCatalog(widgetKey) {
  if (!state.dashboardLayout) {
    state.dashboardLayout = { widgets: [] };
  }
  const widgets = state.dashboardLayout.widgets || [];
  const newId = `w_${Date.now()}`;
  const newW = {
    id: newId,
    widget: widgetKey,
    x: 0,
    y: widgets.length,
    w: 6,
    h: 3,
    params: { period: '30d' },
  };

  widgets.push(newW);
  closeModal();
  renderDashboardGrid();
  loadDashboardWidgetData(newW);
}

async function saveDashboardLayout() {
  if (!state.dashboardLayout) return;
  try {
    const res = await api('/api/dashboard', {
      method: 'PUT',
      body: { widgets: state.dashboardLayout.widgets || [] },
    });
    state.dashboardLayout = res;
    state.dashboardSource = res.source || 'user';
    notify('Disposition du tableau de bord enregistrée avec succès.', 'success');
    toggleDashboardCustomize(false);
  } catch (err) {
    notify(`Erreur lors de l'enregistrement : ${err.message}`, 'error');
  }
}

async function resetDashboardLayout() {
  const ok = await confirmDialog(
    'Rétablir la disposition par défaut',
    'Voulez-vous supprimer votre disposition personnalisée et revenir à la configuration par défaut de votre rôle ?',
    'Rétablir',
    true
  );
  if (!ok) return;

  try {
    const res = await api('/api/dashboard', { method: 'DELETE' });
    state.dashboardLayout = res;
    state.dashboardSource = res.source || 'role';
    notify('Disposition par défaut rétablie.', 'success');
    toggleDashboardCustomize(false);
  } catch (err) {
    notify(`Erreur lors de la réinitialisation : ${err.message}`, 'error');
  }
}

// ============================================================================
// 10C. PAGE REPORTING (PHASE 7)
// ============================================================================

function reportingPage() {
  if (!hasPermission('reporting.view')) {
    return accessDeniedPage();
  }

  const tab = state.reportingTab || 'activity';
  const period = state.reportingPeriod || '30d';
  const isAdm = isAdmin();
  const hasStockPerm = hasPermission('stock.view');

  return `
    <div class="page-head">
      <div class="page-head-title">
        ${icon('i-chart')}
        <div>
          <h1>Reporting & Statistiques</h1>
          <p class="page-subtitle">Analyse d'activité, fiabilité des équipements, respect du préventif et coûts.</p>
        </div>
      </div>
      <div class="page-head-actions">
        ${renderExportDropdown(`report-${tab}`, { period, tab })}
      </div>
    </div>

    <div class="tabs-nav reporting-tabs-nav">
      <button type="button" class="tab-btn ${tab === 'activity' ? 'on' : ''}" data-action="switch-reporting-tab" data-args="activity">
        ${icon('i-wrench')} <span>Activité</span>
      </button>
      <button type="button" class="tab-btn ${tab === 'reliability' ? 'on' : ''}" data-action="switch-reporting-tab" data-args="reliability">
        ${icon('i-shield')} <span>Fiabilité (MTBF/MTTR)</span>
      </button>
      <button type="button" class="tab-btn ${tab === 'preventive' ? 'on' : ''}" data-action="switch-reporting-tab" data-args="preventive">
        ${icon('i-cal')} <span>Maintenance préventive</span>
      </button>
      ${hasStockPerm ? `
        <button type="button" class="tab-btn ${tab === 'costs' ? 'on' : ''}" data-action="switch-reporting-tab" data-args="costs">
          ${icon('i-box')} <span>Coûts & Pièces</span>
        </button>
        <button type="button" class="tab-btn ${tab === 'stock' ? 'on' : ''}" data-action="switch-reporting-tab" data-args="stock">
          ${icon('i-folder')} <span>Valorisation du stock</span>
        </button>
      ` : ''}
    </div>

    <div class="toolbar">
      <div class="toolbar-group">
        <select data-action="filter-reporting-period" aria-label="Sélectionner la période">
          <option value="7d" ${period === '7d' ? 'selected' : ''}>7 derniers jours</option>
          <option value="30d" ${period === '30d' ? 'selected' : ''}>30 derniers jours</option>
          <option value="90d" ${period === '90d' ? 'selected' : ''}>90 derniers jours</option>
          <option value="12m" ${period === '12m' ? 'selected' : ''}>12 derniers mois</option>
          <option value="month" ${period === 'month' ? 'selected' : ''}>Mois civil en cours</option>
          <option value="year" ${period === 'year' ? 'selected' : ''}>Année civile en cours</option>
          <option value="custom" ${period === 'custom' ? 'selected' : ''}>Période personnalisée…</option>
        </select>

        <div id="reportingCustomDates" class="inline-dates" ${period === 'custom' ? '' : 'hidden'}>
          <input type="date" value="${esc(state.reportingFrom)}" data-action="filter-reporting-from" aria-label="Date de début" />
          <span>à</span>
          <input type="date" value="${esc(state.reportingTo)}" data-action="filter-reporting-to" aria-label="Date de fin" />
        </div>

        ${isAdm ? `
          <select data-action="filter-reporting-site" aria-label="Filtrer par site">
            <option value="">Tous les sites</option>
            ${state.sites.map(s => `<option value="${esc(s.id)}" ${state.reportingSiteId === s.id ? 'selected' : ''}>${esc(s.name)}</option>`).join('')}
          </select>
        ` : ''}

        <select data-action="filter-reporting-line" aria-label="Filtrer par ligne">
          <option value="">Toutes les lignes</option>
          ${state.productionLines.map(l => `<option value="${esc(l.id)}" ${state.reportingLineId === l.id ? 'selected' : ''}>${esc(l.name)}</option>`).join('')}
        </select>

        <select data-action="filter-reporting-equipment" aria-label="Filtrer par équipement">
          <option value="">Tous les équipements</option>
          ${state.equipments.map(e => `<option value="${esc(e.id)}" ${state.reportingEquipmentId === e.id ? 'selected' : ''}>${esc(e.code)} — ${esc(e.name)}</option>`).join('')}
        </select>
      </div>
      <div class="toolbar-actions">
        <button type="button" class="btn" data-action="refresh-reporting-data">
          ${icon('i-refresh')} <span>Actualiser</span>
        </button>
      </div>
    </div>

    <div id="reportingTabContent" class="report-layout">
      <p class="hint">${icon('i-clock')} Chargement du rapport…</p>
    </div>
  `;
}

async function initReportingPageView() {
  await switchReportingTab(state.reportingTab || 'activity');
}

async function switchReportingTab(tab) {
  state.reportingTab = tab;
  document.querySelectorAll('.reporting-tabs-nav .tab-btn').forEach(btn => {
    btn.classList.toggle('on', btn.dataset.args === tab);
  });
  await loadReportingData();
}

async function loadReportingData() {
  const container = document.getElementById('reportingTabContent');
  if (!container) return;

  state.reportingLoading = true;
  container.innerHTML = `<p class="hint">${icon('i-clock')} Calcul des indicateurs en cours…</p>`;

  try {
    const tab = state.reportingTab || 'activity';
    const query = new URLSearchParams();
    query.set('period', state.reportingPeriod || '30d');
    if (state.reportingSiteId) query.set('site_id', state.reportingSiteId);
    if (state.reportingLineId) query.set('line_id', state.reportingLineId);
    if (state.reportingEquipmentId) query.set('equipment_id', state.reportingEquipmentId);
    if (state.reportingType) query.set('type', state.reportingType);
    if (state.reportingPeriod === 'custom') {
      if (state.reportingFrom) query.set('from', state.reportingFrom);
      if (state.reportingTo) query.set('to', state.reportingTo);
    }

    const res = await api(`/api/reports/${encodeURIComponent(tab)}?${query.toString()}`);
    state.reportingData = res ? res.data : null;
    renderReportingContent();
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur lors du calcul du rapport : ${esc(err.message)}</p>`;
  } finally {
    state.reportingLoading = false;
  }
}

function renderReportingContent() {
  const container = document.getElementById('reportingTabContent');
  if (!container) return;

  const tab = state.reportingTab;
  const data = state.reportingData;
  if (!data) {
    container.innerHTML = '<p class="hint">Aucune donnée disponible.</p>';
    return;
  }

  if (tab === 'activity') {
    const byStatus = Array.isArray(data.by_status) ? data.by_status : [];
    const byType = Array.isArray(data.by_type) ? data.by_type : [];
    const byPriority = Array.isArray(data.by_priority) ? data.by_priority : [];
    const totalInt = byStatus.reduce((acc, it) => acc + (it.count || 0), 0);

    const statusChartItems = byStatus.map(it => ({
      label: formatInterventionStatus(it.key),
      value: it.count || 0,
      color: it.key === 'open' ? 'var(--warn)' : (it.key === 'in_progress' ? 'var(--accent)' : 'var(--ok-line)'),
    }));

    const typeSlices = byType.map(it => ({
      label: formatInterventionType(it.key),
      value: it.count || 0,
    }));

    const prioritySlices = byPriority.map(it => ({
      label: formatInterventionPriority(it.key),
      value: it.count || 0,
    }));

    container.innerHTML = `
      <div class="kpi-row">
        <div class="kpi-card">
          <span class="kpi-label">Total interventions</span>
          <span class="kpi-val text-accent">${totalInt}</span>
        </div>
        <div class="kpi-card">
          <span class="kpi-label">Délai moyen de réponse</span>
          <span class="kpi-val">${Math.round(data.response_mean_minutes || 0)} <small class="hint">min</small></span>
        </div>
        <div class="kpi-card">
          <span class="kpi-label">Délai médian</span>
          <span class="kpi-val">${Math.round(data.response_median_minutes || 0)} <small class="hint">min</small></span>
        </div>
      </div>

      <div class="report-chart-grid">
        <div class="card">
          <h2>${icon('i-wrench')} Répartition par statut</h2>
          ${renderSvgBarChart({ items: statusChartItems, height: 160 })}
        </div>
        <div class="card">
          <h2>${icon('i-folder')} Répartition par type</h2>
          ${renderSvgDonutChart({ slices: typeSlices, size: 180, title: 'Types' })}
        </div>
        <div class="card">
          <h2>${icon('i-alert')} Répartition par priorité</h2>
          ${renderSvgDonutChart({ slices: prioritySlices, size: 180, title: 'Priorités' })}
        </div>
      </div>
    `;
  } else if (tab === 'reliability') {
    const items = Array.isArray(data.items) ? data.items : [];
    const totalDowntimeMin = Math.round(data.downtime_minutes || 0);

    container.innerHTML = `
      <div class="kpi-row">
        <div class="kpi-card">
          <span class="kpi-label">MTBF global</span>
          <span class="kpi-val">${data.mtbf_minutes !== null && data.mtbf_minutes !== undefined ? Math.round(data.mtbf_minutes / 60) : '—'} <small class="hint">h</small></span>
        </div>
        <div class="kpi-card">
          <span class="kpi-label">MTTR global</span>
          <span class="kpi-val">${data.mttr_minutes !== null && data.mttr_minutes !== undefined ? Math.round(data.mttr_minutes) : '—'} <small class="hint">min</small></span>
        </div>
        <div class="kpi-card">
          <span class="kpi-label">Temps d'arrêt total</span>
          <span class="kpi-val">${Math.floor(totalDowntimeMin / 60)} h ${totalDowntimeMin % 60} min</span>
        </div>
        <div class="kpi-card">
          <span class="kpi-label">Pannes</span>
          <span class="kpi-val">${data.failures || 0}</span>
          <span class="kpi-sub">${data.closed_failures || 0} réparées</span>
        </div>
      </div>

      <div class="card">
        <h2>${icon('i-gear')} Détail par équipement</h2>
        <div class="table-container">
          <table class="data-table">
            <thead>
              <tr>
                <th>Équipement</th>
                <th>Pannes totales</th>
                <th>Pannes closes</th>
                <th>Temps d'arrêt</th>
                <th>MTTR</th>
                <th>MTBF</th>
              </tr>
            </thead>
            <tbody>
              ${items.length ? items.map(it => `
                <tr>
                  <td class="font-bold" title="${esc(it.equipment_name || '')}">${esc(it.equipment_code || (it.equipment_id ? it.equipment_id.slice(0, 8) : 'Hors équipement'))}</td>
                  <td class="mono font-bold">${it.failures || 0}</td>
                  <td class="mono">${it.closed_failures || 0}</td>
                  <td class="mono">${Math.round(it.downtime_minutes || 0)} min</td>
                  <td class="mono">${it.mttr_minutes !== null ? `${Math.round(it.mttr_minutes)} min` : '—'}</td>
                  <td class="mono">${it.mtbf_minutes !== null ? `${Math.round(it.mtbf_minutes / 60)} h` : '—'}</td>
                </tr>
              `).join('') : '<tr><td colspan="6" class="empty-cell">Aucune panne relevée sur les équipements filtrés.</td></tr>'}
            </tbody>
          </table>
        </div>
      </div>
    `;
  } else if (tab === 'preventive') {
    const pct = data.compliance_percent || 0;
    const color = pct >= 80 ? 'var(--ok-line)' : (pct >= 50 ? 'var(--warn)' : 'var(--danger)');
    const byType = Array.isArray(data.by_type) ? data.by_type : [];
    const typeSlices = byType.map(it => ({
      label: formatInterventionType(it.key),
      value: it.count || 0,
    }));

    container.innerHTML = `
      <div class="kpi-row">
        <div class="kpi-card">
          <span class="kpi-label">Échéances dues</span>
          <span class="kpi-val text-accent">${data.due_count || 0}</span>
        </div>
        <div class="kpi-card">
          <span class="kpi-label">Réalisées à l'heure</span>
          <span class="kpi-val text-ok">${data.on_time_count || 0}</span>
        </div>
        <div class="kpi-card">
          <span class="kpi-label">Taux de conformité</span>
          <span class="kpi-val" style="color:${color};">${pct} %</span>
        </div>
      </div>

      <div class="report-chart-grid">
        <div class="card">
          <h2>${icon('i-cal')} Taux de respect</h2>
          ${renderSvgGauge({ percent: pct, label: 'Respect du préventif', color, size: 200 })}
        </div>
        <div class="card">
          <h2>${icon('i-wrench')} Répartition par type de maintenance</h2>
          ${renderSvgDonutChart({ slices: typeSlices, size: 180, title: 'Préventif' })}
        </div>
      </div>
    `;
  } else if (tab === 'costs') {
    const byEq = Array.isArray(data.by_equipment) ? data.by_equipment : [];
    const labor = Array.isArray(data.labor) ? data.labor : [];
    const totalLaborMin = labor.reduce((acc, it) => acc + (it.duration_minutes || 0), 0);

    container.innerHTML = `
      <div class="kpi-row">
        <div class="kpi-card">
          <span class="kpi-label">Coût des pièces consommées</span>
          <span class="kpi-val text-accent">${formatCents(data.parts_cost_cents || 0)}</span>
        </div>
        <div class="kpi-card">
          <span class="kpi-label">Main d'œuvre cumulée</span>
          <span class="kpi-val">${Math.round(totalLaborMin / 60)} <small class="hint">h</small></span>
          <span class="kpi-sub">${totalLaborMin} minutes</span>
        </div>
      </div>

      <div class="report-chart-grid">
        <div class="card">
          <h2>${icon('i-box')} Coût des pièces par équipement</h2>
          <div class="table-container">
            <table class="data-table">
              <thead><tr><th>Équipement</th><th>Coût (€)</th></tr></thead>
              <tbody>
                ${byEq.length ? byEq.map(it => `
                  <tr>
                    <td class="font-bold" title="${esc(it.equipment_name || '')}">${esc(it.equipment_code || (it.equipment_id ? it.equipment_id.slice(0, 8) : 'Hors équipement'))}</td>
                    <td class="mono font-bold">${formatCents(it.cost_cents)}</td>
                  </tr>
                `).join('') : '<tr><td colspan="2" class="empty-cell">Aucune pièce consommée.</td></tr>'}
              </tbody>
            </table>
          </div>
        </div>
        <div class="card">
          <h2>${icon('i-user')} Temps de main d'œuvre</h2>
          <div class="table-container">
            <table class="data-table">
              <thead><tr><th>Équipement</th><th>Technicien</th><th>Durée</th></tr></thead>
              <tbody>
                ${labor.length ? labor.slice(0, 10).map(it => `
                  <tr>
                    <td class="mono">${esc(it.equipment_code || (it.equipment_id ? it.equipment_id.slice(0, 8) : 'Hors équipement'))}</td>
                    <td class="mono">${esc(it.user_display_name || '—')}</td>
                    <td class="mono font-bold">${it.duration_minutes} min</td>
                  </tr>
                `).join('') : '<tr><td colspan="3" class="empty-cell">Aucune saisie de temps.</td></tr>'}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    `;
  } else if (tab === 'stock') {
    const items = Array.isArray(data.items) ? data.items : [];
    const lowCount = items.filter(it => it.low).length;

    container.innerHTML = `
      <div class="kpi-row">
        <div class="kpi-card">
          <span class="kpi-label">Valeur totale du stock</span>
          <span class="kpi-val text-accent">${formatCents(data.value_cents || 0)}</span>
        </div>
        <div class="kpi-card">
          <span class="kpi-label">Références en alerte</span>
          <span class="kpi-val ${lowCount ? 'text-danger' : 'text-ok'}">${lowCount}</span>
        </div>
        <div class="kpi-card">
          <span class="kpi-label">Total articles</span>
          <span class="kpi-val">${items.length}</span>
        </div>
      </div>

      <div class="card">
        <h2>${icon('i-box')} Valorisation détaillée des pièces</h2>
        <div class="table-container">
          <table class="data-table">
            <thead>
              <tr>
                <th>Référence</th>
                <th>Désignation</th>
                <th>Quantité</th>
                <th>Seuil</th>
                <th>Prix unitaire</th>
                <th>Valeur totale</th>
                <th>État</th>
              </tr>
            </thead>
            <tbody>
              ${items.length ? items.map(it => `
                <tr>
                  <td class="mono font-bold">${esc(it.reference)}</td>
                  <td>${esc(it.name)}</td>
                  <td class="mono ${it.low ? 'text-danger font-bold' : ''}">${it.quantity}</td>
                  <td class="mono">${it.threshold}</td>
                  <td class="mono">${formatCents(it.unit_price_cents)}</td>
                  <td class="mono font-bold">${formatCents(it.value_cents)}</td>
                  <td>${it.low ? '<span class="badge badge-state-ko">Stock bas</span>' : '<span class="badge badge-state-ok">Normal</span>'}</td>
                </tr>
              `).join('') : '<tr><td colspan="7" class="empty-cell">Aucune pièce en magasin.</td></tr>'}
            </tbody>
          </table>
        </div>
      </div>
    `;
  }
}

// ============================================================================
// 10D. PAGE JOURNAL D'AUDIT (PHASE 7)
// ============================================================================

function formatAuditAction(action) {
  switch (action) {
    case 'create': return 'Création';
    case 'update': return 'Modification';
    case 'delete': return 'Suppression';
    case 'login': return 'Connexion';
    case 'logout': return 'Déconnexion';
    case 'password_change': return 'Changement mdp';
    case 'password_reset_request': return 'Demande réinit. mdp';
    case 'password_reset_confirm': return 'Confirmation réinit. mdp';
    case 'export': return 'Export';
    default: return action || '—';
  }
}

function formatAuditEntity(entity) {
  switch (entity) {
    case 'users': return 'Utilisateur';
    case 'sites': return 'Site';
    case 'production_lines': return 'Ligne de prod.';
    case 'equipments': return 'Équipement';
    case 'locations': return 'Emplacement';
    case 'suppliers': return 'Fournisseur';
    case 'parts': return 'Pièce';
    case 'stock_movements': return 'Mvt stock';
    case 'interventions': return 'Intervention';
    case 'maintenance_plans': return 'Plan maint.';
    case 'documents': return 'Document';
    case 'settings': return 'Paramètre';
    case 'dashboard': return 'Tableau de bord';
    default: return entity || '—';
  }
}

function auditPage() {
  if (!hasPermission('audit.view')) {
    return accessDeniedPage();
  }

  const isAdm = isAdmin();

  return `
    <div class="page-head">
      <div class="page-head-title">
        ${icon('i-audit')}
        <div>
          <h1>Journal d'audit</h1>
          <p class="page-subtitle">Traçabilité complète des actions, créations, modifications et exports.</p>
        </div>
      </div>
      <div class="page-head-actions">
        ${renderExportDropdown('audit')}
      </div>
    </div>

    <div class="toolbar">
      <div class="toolbar-group">
        <div class="search-input-wrap">
          ${icon('i-search')}
          <input type="search" placeholder="Rechercher dans l'audit…" value="${esc(state.auditQuery)}" data-action="filter-audit-query" aria-label="Recherche dans le journal d'audit" />
        </div>

        <select data-action="filter-audit-action" aria-label="Filtrer par action">
          <option value="">Toutes les actions</option>
          <option value="create" ${state.auditAction === 'create' ? 'selected' : ''}>Création</option>
          <option value="update" ${state.auditAction === 'update' ? 'selected' : ''}>Modification</option>
          <option value="delete" ${state.auditAction === 'delete' ? 'selected' : ''}>Suppression</option>
          <option value="login" ${state.auditAction === 'login' ? 'selected' : ''}>Connexion</option>
          <option value="logout" ${state.auditAction === 'logout' ? 'selected' : ''}>Déconnexion</option>
          <option value="password_change" ${state.auditAction === 'password_change' ? 'selected' : ''}>Changement mot de passe</option>
          <option value="export" ${state.auditAction === 'export' ? 'selected' : ''}>Export</option>
        </select>

        <select data-action="filter-audit-entity" aria-label="Filtrer par entité">
          <option value="">Toutes les entités</option>
          <option value="users" ${state.auditEntity === 'users' ? 'selected' : ''}>Utilisateurs</option>
          <option value="equipments" ${state.auditEntity === 'equipments' ? 'selected' : ''}>Équipements</option>
          <option value="interventions" ${state.auditEntity === 'interventions' ? 'selected' : ''}>Interventions</option>
          <option value="parts" ${state.auditEntity === 'parts' ? 'selected' : ''}>Pièces détachées</option>
          <option value="maintenance_plans" ${state.auditEntity === 'maintenance_plans' ? 'selected' : ''}>Maintenance préventive</option>
          <option value="documents" ${state.auditEntity === 'documents' ? 'selected' : ''}>Documents</option>
          <option value="settings" ${state.auditEntity === 'settings' ? 'selected' : ''}>Paramètres</option>
        </select>

        ${isAdm ? `
          <select data-action="filter-audit-site" aria-label="Filtrer par site">
            <option value="">Tous les sites</option>
            ${state.sites.map(s => `<option value="${esc(s.id)}" ${state.auditSiteId === s.id ? 'selected' : ''}>${esc(s.name)}</option>`).join('')}
          </select>
        ` : ''}
      </div>
      <div class="toolbar-actions">
        <button type="button" class="btn" data-action="refresh-audit-list">
          ${icon('i-refresh')} <span>Actualiser</span>
        </button>
      </div>
    </div>

    <div id="auditContainer">
      <p class="hint">${icon('i-clock')} Chargement du journal d'audit…</p>
    </div>
  `;
}

async function initAuditPageView() {
  await loadAuditList(true);
}

async function loadAuditList(reset = false) {
  const container = document.getElementById('auditContainer');
  if (!container) return;

  if (reset) {
    state.auditItems = [];
    state.auditNextCursor = null;
    container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement du journal d'audit…</p>`;
  }

  state.auditLoading = true;

  try {
    const query = new URLSearchParams();
    query.set('limit', '50');
    if (state.auditNextCursor) query.set('cursor', state.auditNextCursor);
    if (state.auditQuery) query.set('q', state.auditQuery);
    if (state.auditAction) query.set('action', state.auditAction);
    if (state.auditEntity) query.set('entity', state.auditEntity);
    if (state.auditSiteId) query.set('site_id', state.auditSiteId);
    if (state.auditFrom) query.set('from', state.auditFrom);
    if (state.auditTo) query.set('to', state.auditTo);

    const res = await api(`/api/audit?${query.toString()}`);
    const items = Array.isArray(res) ? res : (res && Array.isArray(res.items) ? res.items : []);
    state.auditNextCursor = res && res.next_cursor ? res.next_cursor : null;

    if (reset) {
      state.auditItems = items;
    } else {
      state.auditItems = [...state.auditItems, ...items];
    }

    renderAuditTable();
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur lors du chargement de l'audit : ${esc(err.message)}</p>`;
  } finally {
    state.auditLoading = false;
  }
}

function renderAuditTable() {
  const container = document.getElementById('auditContainer');
  if (!container) return;

  const items = state.auditItems || [];
  const sitesMap = new Map(state.sites.map(s => [s.id, s.name]));
  const usersMap = new Map((state.usersList || []).map(u => [u.id, u.display_name || u.username]));

  const rows = items.map(it => {
    const siteName = it.site_id ? (sitesMap.get(it.site_id) || 'Site') : 'Global';
    const userName = it.user_display_name || (it.user_id ? (usersMap.get(it.user_id) || it.user_id.slice(0, 8)) : 'Système');

    let actionBadgeClass = 'badge';
    if (it.action === 'create') actionBadgeClass = 'badge badge-state-ok';
    else if (it.action === 'delete') actionBadgeClass = 'badge badge-state-ko';
    else if (it.action === 'update') actionBadgeClass = 'badge badge-state-warn';

    return `
      <tr>
        <td class="mono font-bold">${esc(formatDate(it.created_at))}</td>
        <td>${esc(userName)}</td>
        <td><span class="${actionBadgeClass}">${esc(formatAuditAction(it.action))}</span></td>
        <td><span class="badge">${esc(formatAuditEntity(it.entity))}</span></td>
        <td class="mono hint">${it.entity_id ? esc(it.entity_id) : '—'}</td>
        <td>${esc(siteName)}</td>
        <td>
          <button type="button" class="btn sm" data-action="open-audit-detail" data-args="${jsArg(it.id)}" title="Voir les modifications">
            ${icon('i-doc')} <span>Détail</span>
          </button>
        </td>
      </tr>
    `;
  }).join('');

  container.innerHTML = `
    <div class="table-container">
      <table class="data-table">
        <thead>
          <tr>
            <th>Date & heure</th>
            <th>Utilisateur</th>
            <th>Action</th>
            <th>Entité</th>
            <th>Identifiant</th>
            <th>Site</th>
            <th>Détail</th>
          </tr>
        </thead>
        <tbody>
          ${rows || '<tr><td colspan="7" class="empty-cell">Aucune entrée trouvée dans le journal d\'audit.</td></tr>'}
        </tbody>
      </table>
    </div>
    ${renderLoadMore(state.auditNextCursor, 'load-more-audit', state.auditLoading)}
  `;
}

async function openAuditDetailModal(auditId) {
  try {
    const item = await api(`/api/audit/${encodeURIComponent(auditId)}`);
    if (!item) return;

    const diffRows = [];
    if (Array.isArray(item.diff) && item.diff.length > 0) {
      item.diff.forEach(d => {
        diffRows.push(`
          <tr>
            <td class="mono font-bold">${esc(d.field)}</td>
            <td class="diff-before mono">${esc(d.before !== null && d.before !== undefined ? JSON.stringify(d.before) : '—')}</td>
            <td class="diff-after mono">${esc(d.after !== null && d.after !== undefined ? JSON.stringify(d.after) : '—')}</td>
          </tr>
        `);
      });
    } else if (item.before || item.after) {
      const allKeys = Array.from(new Set([
        ...Object.keys(item.before || {}),
        ...Object.keys(item.after || {}),
      ]));
      allKeys.forEach(k => {
        const b = item.before ? item.before[k] : undefined;
        const a = item.after ? item.after[k] : undefined;
        if (JSON.stringify(b) !== JSON.stringify(a)) {
          diffRows.push(`
            <tr>
              <td class="mono font-bold">${esc(k)}</td>
              <td class="diff-before mono">${esc(b !== undefined ? JSON.stringify(b) : '—')}</td>
              <td class="diff-after mono">${esc(a !== undefined ? JSON.stringify(a) : '—')}</td>
            </tr>
          `);
        }
      });
    }

    const bodyHtml = `
      <dl class="profile-info-dl">
        <dt>Date & heure</dt><dd class="mono">${esc(formatDate(item.created_at))}</dd>
        <dt>Action</dt><dd><span class="badge">${esc(formatAuditAction(item.action))}</span></dd>
        <dt>Entité</dt><dd><span class="badge">${esc(formatAuditEntity(item.entity))}</span> (${esc(item.entity_id || '—')})</dd>
        <dt>Adresse IP</dt><dd class="mono">${esc(item.ip || 'Non enregistrée')}</dd>
      </dl>

      <h3>Modifications apportées</h3>
      ${diffRows.length ? `
        <table class="diff-table">
          <thead>
            <tr>
              <th>Champ</th>
              <th>Avant</th>
              <th>Après</th>
            </tr>
          </thead>
          <tbody>
            ${diffRows.join('')}
          </tbody>
        </table>
      ` : '<p class="hint">Aucun détail de modification disponible pour cette entrée.</p>'}
    `;

    openModal(`Détail d'audit #${item.id.slice(0, 8)}`, bodyHtml, '<button type="button" class="btn" data-action="close-modal">Fermer</button>');
  } catch (err) {
    notify(`Impossible de charger l'entrée d'audit : ${err.message}`, 'error');
  }
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
        <h2>${icon('i-mail')} Adresse e-mail</h2>
        <p class="hint form-help">Utilisée pour la réception des alertes et la réinitialisation de mot de passe.</p>
        <form class="form-grid" data-action="profile-email-submit">
          <div id="profileEmailErrorBox" class="form-error" hidden>
            ${icon('i-alert')}
            <span id="profileEmailErrorText"></span>
          </div>
          <div class="form-group">
            <label for="profEmailInput">Adresse e-mail</label>
            <input type="email" id="profEmailInput" name="email" value="${esc(user.email || '')}" placeholder="prenom.nom@example.com" />
          </div>
          <button type="submit" class="btn primary" id="profEmailSubmitBtn">
            <span>Enregistrer mon adresse e-mail</span>
          </button>
        </form>
      </section>

      <section class="card">
        <h2>${icon('i-bell')} Préférences d'alertes par e-mail</h2>
        <p class="hint form-help">Cochez les événements pour lesquels vous souhaitez recevoir une notification par e-mail.</p>
        <form id="profileNotifsForm" data-action="profile-notifications-submit">
          <div id="profileNotifsLoading">
            <p class="hint">${icon('i-clock')} Chargement de vos préférences…</p>
          </div>
          <div id="profileNotifsContent" hidden>
            <div class="notif-prefs-grid" id="profileNotifsGrid"></div>
            <button type="submit" class="btn primary mt-4" id="profNotifsSubmitBtn">
              <span>Enregistrer mes alertes</span>
            </button>
          </div>
        </form>
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

const NOTIF_EVENT_LABELS = {
  stock_low: 'Alerte de stock bas',
  maintenance_generated: 'Nouveau bon de travail préventif généré',
  maintenance_overdue: 'Rappel quotidien des maintenances en retard',
  intervention_critical: 'Intervention critique créée',
  intervention_assigned: 'Intervention qui m’est assignée',
};

async function initProfilePageView() {
  await loadProfileNotifications();
}

async function loadProfileNotifications() {
  const loading = document.getElementById('profileNotifsLoading');
  const content = document.getElementById('profileNotifsContent');
  const grid = document.getElementById('profileNotifsGrid');
  if (!grid) return;

  try {
    const prefs = await api('/api/profile/notifications');
    state.profileNotifications = prefs || {};

    const entries = Object.entries(state.profileNotifications);
    if (!entries.length) {
      grid.innerHTML = '<p class="hint">Aucune notification disponible pour votre rôle.</p>';
    } else {
      grid.innerHTML = entries.map(([key, val]) => `
        <label class="notif-pref-row">
          <span>${esc(NOTIF_EVENT_LABELS[key] || key)}</span>
          <input type="checkbox" name="${esc(key)}" ${val ? 'checked' : ''} />
        </label>
      `).join('');
    }

    if (loading) loading.hidden = true;
    if (content) content.hidden = false;
  } catch (err) {
    if (loading) loading.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur : ${esc(err.message)}</p>`;
  }
}

async function handleProfileEmailSubmit(form) {
  const errorBox = document.getElementById('profileEmailErrorBox');
  const errorText = document.getElementById('profileEmailErrorText');
  const btn = document.getElementById('profEmailSubmitBtn');

  const email = (form.email ? form.email.value : '').trim() || null;
  if (btn) btn.disabled = true;
  if (errorBox) errorBox.hidden = true;

  try {
    const res = await api(`/api/users/${encodeURIComponent(state.currentUser.id)}`, {
      method: 'PATCH',
      body: { email },
    });
    if (res) {
      state.currentUser.email = res.email !== undefined ? res.email : email;
    }
    notify('Adresse e-mail mise à jour avec succès.', 'success');
  } catch (err) {
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = err.message || 'Impossible de mettre à jour votre adresse e-mail.';
    }
  } finally {
    if (btn) btn.disabled = false;
  }
}

async function handleProfileNotificationsSubmit(form) {
  const btn = document.getElementById('profNotifsSubmitBtn');
  if (btn) btn.disabled = true;

  const payload = {};
  const currentKeys = Object.keys(state.profileNotifications || {});
  currentKeys.forEach(key => {
    const input = form.elements[key];
    if (input) {
      payload[key] = Boolean(input.checked);
    }
  });

  try {
    const res = await api('/api/profile/notifications', {
      method: 'PUT',
      body: payload,
    });
    state.profileNotifications = res || payload;
    notify('Vos préférences de notifications ont été enregistrées.', 'success');
  } catch (err) {
    notify(`Erreur : ${err.message}`, 'error');
  } finally {
    if (btn) btn.disabled = false;
  }
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
          ${u.email ? `<br/><span class="hint">${esc(u.email)}</span>` : ''}
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
      <div class="form-group">
        <label for="uEmail">Adresse e-mail</label>
        <input type="email" id="uEmail" name="email" placeholder="utilisateur@example.com" />
        <span class="form-help">Optionnelle. Utilisée pour les alertes et la réinitialisation de mot de passe.</span>
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
      <div class="form-group">
        <label for="uEmail">Adresse e-mail</label>
        <input type="email" id="uEmail" name="email" value="${esc(user.email || '')}" placeholder="utilisateur@example.com" />
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
  const email = (form.email ? form.email.value : '').trim() || null;
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
      body: { username, display_name, email, role, site_id, password, active: true },
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
  if (form.email) payload.email = form.email.value.trim() || null;
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
    ${state.selectedEquipmentIds && state.selectedEquipmentIds.size > 0 ? `
      <button type="button" class="btn primary sm" data-action="open-print-selected-labels">
        ${icon('i-print')} <span>Imprimer les étiquettes (${state.selectedEquipmentIds.size})</span>
      </button>
      <button type="button" class="btn sm" data-action="clear-selected-equipments">Désélectionner</button>
    ` : ''}
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
      secondaryActionsHtml: renderExportDropdown('equipments'),
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

  const allSelected = items.length > 0 && items.every(eq => state.selectedEquipmentIds.has(eq.id));

  const rows = items.map(eq => {
    const lineLabel = eq.production_line ? eq.production_line.name : '—';
    const parentLabel = eq.parent ? eq.parent.name : '—';
    const isSelected = state.selectedEquipmentIds.has(eq.id);

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
      <tr class="${isSelected ? 'row-selected' : ''}">
        <td class="td-cb">
          <input type="checkbox" data-action="toggle-select-equipment" data-args="${jsArg(eq.id)}" ${isSelected ? 'checked' : ''} aria-label="Sélectionner ${esc(eq.code)}" />
        </td>
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
            <th class="th-cb">
              <input type="checkbox" data-action="toggle-select-all-equipments" ${allSelected ? 'checked' : ''} aria-label="Tout sélectionner" />
            </th>
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

    state.currentEquipmentDetailId = eq.id;

    let subItems = [];
    let eqParts = [];
    let counters = [];
    let eqPlans = [];
    try {
      const [subRes, partsRes, cntRes, planRes] = await Promise.all([
        api(`/api/equipments?parent_id=${encodeURIComponent(equipmentId)}&limit=100`).catch(() => ({ items: [] })),
        hasPermission('parts.view') ? api(`/api/equipments/${encodeURIComponent(equipmentId)}/parts`).catch(() => []) : Promise.resolve([]),
        api(`/api/equipments/${encodeURIComponent(equipmentId)}/counters`).catch(() => []),
        hasPermission('maintenance.view') ? api(`/api/maintenance/plans?equipment_id=${encodeURIComponent(equipmentId)}&limit=100`).catch(() => ({ items: [] })) : Promise.resolve({ items: [] }),
      ]);
      subItems = subRes && Array.isArray(subRes.items) ? subRes.items : [];
      eqParts = (Array.isArray(partsRes) ? partsRes : []).map(adaptStock);
      counters = Array.isArray(cntRes) ? cntRes : (cntRes && Array.isArray(cntRes.items) ? cntRes.items : []);
      eqPlans = planRes && Array.isArray(planRes.items) ? planRes.items : [];
    } catch (_) {}

    const canEdit = hasPermission('equipments.edit');
    const canManageParts = canEdit && hasPermission('parts.edit');
    const canRecordCounter = hasPermission('counters.record');
    const canViewMaintenance = hasPermission('maintenance.view');
    const canManageMaintenance = hasPermission('maintenance.manage');
    const lineLabel = eq.production_line ? `${eq.production_line.code} — ${eq.production_line.name}` : 'Aucune ligne rattachée';
    const parentLabel = eq.parent ? `${eq.parent.code} — ${eq.parent.name}` : 'Équipement racine (aucun parent)';

    const countersSectionHtml = `
      <div class="detail-card detail-card-spaced">
        <div class="detail-nav-bar">
          <h3>${icon('i-clock')} Compteurs (${counters.length})</h3>
          ${canEdit ? `
            <button type="button" class="btn sm" data-action="open-create-counter" data-args="${jsArg(eq.id)}">
              ${icon('i-plus')} <span>Ajouter un compteur</span>
            </button>
          ` : ''}
        </div>
        ${counters.length ? `
          <div class="counters-grid">
            ${counters.map(c => `
              <div class="counter-card">
                <div class="counter-card-header">
                  <div class="counter-card-title">${esc(c.name)}</div>
                  <span class="badge ${c.active ? 'badge-active-on' : 'badge-active-off'}">${c.active ? 'Actif' : 'Inactif'}</span>
                </div>
                <div class="counter-value-box">
                  <span class="counter-large-val mono">${esc(c.current_value)}</span>
                  <span class="counter-unit">${esc(c.unit)}</span>
                </div>
                <div class="counter-last-reading">
                  ${c.last_reading_at ? `Dernier relevé : ${formatDate(c.last_reading_at)}` : 'Aucun relevé enregistré'}
                </div>
                <div class="hint">
                  ${c.plans_count} planning${c.plans_count > 1 ? 's' : ''} associé${c.plans_count > 1 ? 's' : ''}
                </div>
                <div class="counter-card-actions">
                  ${canRecordCounter ? `
                    <button type="button" class="btn sm primary" data-action="open-record-reading" data-args="${jsArg({ counterId: c.id, equipmentId: eq.id })}" title="Saisir un relevé">
                      ${icon('i-edit')} <span>Relever</span>
                    </button>
                    <button type="button" class="btn sm" data-action="open-reset-counter" data-args="${jsArg({ counterId: c.id, equipmentId: eq.id })}" title="Remettre à zéro ou remplacer le compteur">
                      ${icon('i-refresh')} <span>RàZ</span>
                    </button>
                  ` : ''}
                  <button type="button" class="btn sm" data-action="open-counter-history" data-args="${jsArg({ counterId: c.id, equipmentId: eq.id })}" title="Historique des relevés">
                    ${icon('i-clock')} <span>Historique</span>
                  </button>
                  ${canEdit ? `
                    <button type="button" class="btn sm" data-action="open-edit-counter" data-args="${jsArg({ counterId: c.id, equipmentId: eq.id })}" title="Modifier le compteur">
                      ${icon('i-edit')}
                    </button>
                    <button type="button" class="btn sm danger" data-action="delete-counter" data-args="${jsArg({ counterId: c.id, equipmentId: eq.id })}" title="Supprimer le compteur">
                      ${icon('i-trash')}
                    </button>
                  ` : ''}
                </div>
              </div>
            `).join('')}
          </div>
        ` : `
          <p class="hint">Aucun compteur configuré pour cet équipement.</p>
        `}
      </div>
    `;

    const plansSectionHtml = canViewMaintenance ? `
      <div class="detail-card detail-card-spaced">
        <div class="detail-nav-bar">
          <h3>${icon('i-cal')} Plannings de maintenance préventive (${eqPlans.length})</h3>
          ${canManageMaintenance ? `
            <button type="button" class="btn sm" data-action="open-create-maintenance-plan" data-args="${jsArg({ equipment_id: eq.id })}">
              ${icon('i-plus')} <span>Nouveau planning</span>
            </button>
          ` : ''}
        </div>
        ${eqPlans.length ? `
          <div class="table-container">
            <table class="data-table">
              <thead>
                <tr>
                  <th>Code</th>
                  <th>Titre</th>
                  <th>Fréquence</th>
                  <th>Prochaine échéance</th>
                  <th>État</th>
                  <th>Action</th>
                </tr>
              </thead>
              <tbody>
                ${eqPlans.map(p => `
                  <tr>
                    <td><b class="mono">${esc(p.code)}</b></td>
                    <td><b>${esc(p.title)}</b></td>
                    <td><span class="badge badge-freq">${esc(planFrequencyLabel(p))}</span></td>
                    <td>${esc(formatPlanDue(p))}</td>
                    <td>${planStateBadge(p.state)}</td>
                    <td>
                      <a href="#/maintenance-preventive/${esc(p.id)}" class="btn sm" data-action="close-modal" title="Ouvrir la fiche planning">
                        ${icon('i-info')} <span>Consulter</span>
                      </a>
                    </td>
                  </tr>
                `).join('')}
              </tbody>
            </table>
          </div>
        ` : '<p class="hint">Aucun planning de maintenance associé à cet équipement.</p>'}
      </div>
    ` : '';

    const partsRows = eqParts.map(item => {
      const p = item.part || {};
      const avail = p.available_stock !== undefined ? p.available_stock : (p.total_stock || 0);
      return `
        <tr>
          <td>
            <a href="#/pieces/${esc(p.id)}" class="link-action mono" data-action="close-modal">
              <b>${esc(p.reference || '—')}</b>
            </a>
          </td>
          <td>${esc(p.name || '—')}</td>
          <td class="mono"><b>${esc(item.quantity_recommended || 1)}</b></td>
          <td><span class="hint">${esc(item.notes || '—')}</span></td>
          <td>
            <span class="stock-pill pill-avail ${avail <= (p.effective_threshold || 0) ? 'avail-low' : ''}">
              Dispo: <b>${esc(avail)}</b> ${esc(formatPartUnit(p.unit))}
            </span>
          </td>
          ${canManageParts ? `
            <td>
              <button type="button" class="btn sm danger" title="Dissocier la pièce" data-action="delete-equipment-part" data-args="${jsArg({ equipmentId: eq.id, partId: p.id })}">
                ${icon('i-trash')}
              </button>
            </td>
          ` : ''}
        </tr>
      `;
    }).join('');

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

      <!-- PIÈCES DÉTACHÉES RECOMMANDÉES -->
      <div class="detail-card detail-card-spaced">
        <div class="detail-nav-bar">
          <h3>${icon('i-box')} Pièces détachées recommandées (${eqParts.length})</h3>
          ${canManageParts ? `
            <button type="button" class="btn sm" data-action="open-add-equipment-part" data-args="${jsArg(eq.id)}">
              ${icon('i-plus')} <span>Associer une pièce</span>
            </button>
          ` : ''}
        </div>
        ${eqParts.length ? `
          <div class="table-container">
            <table class="data-table">
              <thead>
                <tr>
                  <th>Référence</th>
                  <th>Désignation</th>
                  <th>Qté préconisée</th>
                  <th>Notes d'assemblage</th>
                  <th>Stock magasin</th>
                  ${canManageParts ? `<th>Action</th>` : ''}
                </tr>
              </thead>
              <tbody>
                ${partsRows}
              </tbody>
            </table>
          </div>
        ` : '<p class="hint">Aucune pièce détachée n\'est encore associée à cet équipement.</p>'}
      </div>
      ${countersSectionHtml}
      ${plansSectionHtml}

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

      <!-- SECTION ÉTIQUETTE ET QR CODE (PHASE 6) -->
      <div class="detail-card detail-card-spaced qr-card">
        <div class="detail-nav-bar">
          <h3>${icon('i-qrcode')} Étiquette et QR code</h3>
          <button type="button" class="btn sm" data-action="print-equipment-label" data-args="${jsArg(eq.id)}">
            ${icon('i-print')} <span>Imprimer l'étiquette</span>
          </button>
        </div>
        <div class="qr-preview-box">
          <img src="/api/equipments/${esc(eq.id)}/qr.svg" alt="QR Code équipement" class="qr-preview-img" width="100" height="100" />
          <div class="qr-preview-info">
            <p>Scannez ce QR code sur le terrain pour ouvrir directement la fiche de cet équipement.</p>
            <div class="hint mono">${esc(eq.code)} — ${esc(eq.name)}</div>
          </div>
        </div>
      </div>

      <!-- SECTION DOCUMENTS (PHASE 6) -->
      <div id="equipmentDocumentsContainer"></div>
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
    loadAndRenderEntityDocuments('equipment', eq.id, 'equipmentDocumentsContainer');
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
      secondaryActionsHtml: renderExportDropdown('interventions'),
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
        ${hasPermission('exports.run') ? `
          <button type="button" class="btn secondary" data-action="download-intervention-pdf" data-args="${jsArg(id)}" title="Télécharger la fiche intervention en PDF">
            ${icon('i-file-pdf')} <span>Fiche PDF</span>
          </button>
        ` : ''}
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
    const inter = await fetchInterventionFull(id);
    state.currentInterventionDetail = inter;
    renderInterventionDetailView(inter);
    loadAndRenderEntityDocuments('intervention', inter.id, 'interventionDocumentsContainer');
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

  // --- Phase 4 : Réservations et pièces consommées ---
  const canManageParts = hasPermission('stock.movements') && (isOpen || isInProgress);

  // Réservations de pièces préparées
  const resList = Array.isArray(inter.reservations) ? inter.reservations : [];
  let resRows = resList.map(r => {
    const actions = [];
    if (r.status === 'prepared' && canManageParts) {
      actions.push(`
        <button type="button" class="btn sm primary" data-action="open-consume-reservation" data-args="${jsArg({ reservationId: r.id, partName: r.part ? r.part.name : '', maxQuantity: r.quantity, interventionId: inter.id })}" title="Consommer sur l'intervention">
          ${icon('i-check')} <span>Consommer</span>
        </button>
        <button type="button" class="btn sm danger" data-action="release-reservation" data-args="${jsArg({ reservationId: r.id, interventionId: inter.id })}" title="Remettre en rayon">
          ${icon('i-ban')} <span>Remettre en rayon</span>
        </button>
      `);
    }

    return `
      <tr>
        <td>
          <a href="#/pieces/${esc(r.part_id)}" class="link-action mono"><b>${esc(r.part ? r.part.reference : '—')}</b></a>
          <div>${esc(r.part ? r.part.name : '—')}</div>
        </td>
        <td><b>${esc(r.location ? r.location.name : '—')}</b></td>
        <td class="mono"><b>${esc(r.quantity)}</b> ${esc(formatPartUnit(r.part ? r.part.unit : 'piece'))}</td>
        <td>${reservationStatusBadge(r.status)}</td>
        <td>
          ${esc(r.reserved_by ? r.reserved_by.display_name : '—')}
          <div class="hint">${formatDate(r.created_at)}</div>
        </td>
        <td><div class="cell-actions">${actions.length ? actions.join('') : '—'}</div></td>
      </tr>
    `;
  }).join('');

  if (!resList.length) {
    resRows = `<tr><td colspan="6" class="empty-cell">Aucune pièce préparée pour cette intervention.</td></tr>`;
  }

  // Pièces consommées & valorisation
  const partsList = Array.isArray(inter.parts) ? inter.parts : [];
  let consumedPartsRows = partsList.map(ip => {
    const actions = [];
    if (canManageParts) {
      actions.push(`
        <button type="button" class="btn sm danger" data-action="delete-intervention-part" data-args="${jsArg({ interventionId: inter.id, itemId: ip.id })}" title="Annuler cette sortie et réintégrer le stock">
          ${icon('i-trash')} <span>Annuler</span>
        </button>
      `);
    }

    return `
      <tr>
        <td>
          <a href="#/pieces/${esc(ip.part_id)}" class="link-action mono"><b>${esc(ip.part ? ip.part.reference : '—')}</b></a>
          <div>${esc(ip.part ? ip.part.name : '—')}</div>
        </td>
        <td>${esc(ip.location ? ip.location.name : '—')}</td>
        <td class="mono"><b>${esc(ip.quantity)}</b> ${esc(formatPartUnit(ip.part ? ip.part.unit : 'piece'))}</td>
        <td class="mono">${formatCents(ip.unit_price_cents)}</td>
        <td class="mono font-bold">${formatCents(ip.total_price_cents)}</td>
        <td>${formatDate(ip.created_at)}</td>
        <td><div class="cell-actions">${actions.length ? actions.join('') : '—'}</div></td>
      </tr>
    `;
  }).join('');

  if (!partsList.length) {
    consumedPartsRows = `<tr><td colspan="7" class="empty-cell">Aucune pièce détachée consommée sur cette intervention.</td></tr>`;
  }

  const totalPartsCostCents = partsList.reduce((acc, p) => acc + (p.total_price_cents || 0), 0);

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
          ${inter.maintenance_plan || inter.maintenance_occurrence ? `
            <dt>Planning préventif</dt>
            <dd>
              ${inter.maintenance_plan ? `
                <a href="#/maintenance-preventive/${esc(inter.maintenance_plan.id)}" class="link-action">
                  <b>${esc(inter.maintenance_plan.code)}</b> — ${esc(inter.maintenance_plan.title)}
                </a>
              ` : 'Planning préventif lié'}
              ${inter.maintenance_occurrence && inter.maintenance_occurrence.due_date ? `
                <span class="hint"> (Échéance prévue : <b>${esc(formatDate(inter.maintenance_occurrence.due_date))}</b>)</span>
              ` : ''}
              ${inter.maintenance_occurrence && inter.maintenance_occurrence.due_counter !== null && inter.maintenance_occurrence.due_counter !== undefined ? `
                <span class="hint"> (Seuil compteur : <b>${esc(inter.maintenance_occurrence.due_counter)}</b>)</span>
              ` : ''}
            </dd>
          ` : ''}
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

    <!-- PIÈCES PRÉPARÉES (RÉSERVATIONS) -->
    <section class="card">
      <div class="detail-nav-bar">
        <h2>${icon('i-layers')} Pièces préparées (réservations) (${esc(resList.length)})</h2>
        ${canManageParts ? `
          <button type="button" class="btn sm" data-action="open-add-intervention-reservation" data-args="${jsArg(inter.id)}">
            ${icon('i-plus')} <span>Préparer une pièce</span>
          </button>
        ` : ''}
      </div>
      <div class="table-container">
        <table class="data-table">
          <thead>
            <tr>
              <th>Pièce détachée</th>
              <th>Emplacement</th>
              <th>Quantité</th>
              <th>Statut</th>
              <th>Préparé par</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            ${resRows}
          </tbody>
        </table>
      </div>
    </section>

    <!-- PIÈCES CONSOMMÉES ET COÛTS -->
    <section class="card">
      <div class="detail-nav-bar">
        <h2>${icon('i-basket')} Pièces consommées & valorisation (${esc(partsList.length)})</h2>
        ${canManageParts ? `
          <button type="button" class="btn sm primary" data-action="open-add-intervention-part" data-args="${jsArg(inter.id)}">
            ${icon('i-plus')} <span>Sortie de pièce</span>
          </button>
        ` : ''}
      </div>
      <div class="table-container">
        <table class="data-table">
          <thead>
            <tr>
              <th>Pièce détachée</th>
              <th>Emplacement</th>
              <th>Quantité</th>
              <th>PMP unitaire HT</th>
              <th>Total HT</th>
              <th>Sortie le</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            ${consumedPartsRows}
          </tbody>
        </table>
      </div>
      <div class="parts-total-box">
        <span>Coût total des pièces consommées (valorisation PMP) :</span>
        <span class="parts-total-value mono font-bold">${formatCents(totalPartsCostCents)}</span>
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

    <!-- SECTION DOCUMENTS (PHASE 6) -->
    <div id="interventionDocumentsContainer"></div>
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
      inter = await fetchInterventionFull(id);
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
      inter = await fetchInterventionFull(id);
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

  const activeReservations = Array.isArray(inter.reservations)
    ? inter.reservations.filter(r => r.status === 'prepared')
    : [];
  let reservationWarningHtml = '';
  if (activeReservations.length > 0) {
    const totalQty = activeReservations.reduce((acc, r) => acc + (r.quantity || 0), 0);
    reservationWarningHtml = `
      <div class="machine-state-alert">
        ${icon('i-alert')}
        <span><b>Pièces préparées non consommées :</b> Cette intervention comporte ${activeReservations.length} ligne(s) de réservation (${totalQty} pièce(s)) en statut préparé. Lors de la clôture, elles seront automatiquement remises en rayon.</span>
      </div>
    `;
  }

  const fieldsHtml = `
    ${timeWarningHtml}
    ${reservationWarningHtml}
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
      inter = await fetchInterventionFull(id);
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
// 14F. MODULE MAGASIN & PIÈCES DÉTACHÉES (PHASE 4)
// ============================================================================

function stockPage() {
  const alertsCount = state.stockAlerts.length || state.partsLowStockCount || 0;
  const currentList = state.stockActiveTab === 'stock' ? 'stock' : (state.stockActiveTab === 'movements' ? 'movements' : 'parts');

  return `
    <div class="page-head">
      <div class="page-head-title">
        ${icon('i-basket')}
        <h1>Magasin & Pièces détachées</h1>
      </div>
      <div class="page-head-actions">
        ${renderExportDropdown(currentList)}
      </div>
    </div>

    <div class="tabs-bar">
      <button type="button" class="tab-btn ${state.stockActiveTab === 'catalog' ? 'active' : ''}" data-action="switch-stock-tab" data-args="${jsArg('catalog')}">
        ${icon('i-basket')} <span>Catalogue des pièces</span>
      </button>
      <button type="button" class="tab-btn ${state.stockActiveTab === 'stock' ? 'active' : ''}" data-action="switch-stock-tab" data-args="${jsArg('stock')}">
        ${icon('i-layers')} <span>Niveaux par emplacement</span>
      </button>
      <button type="button" class="tab-btn ${state.stockActiveTab === 'movements' ? 'active' : ''}" data-action="switch-stock-tab" data-args="${jsArg('movements')}">
        ${icon('i-transfer')} <span>Mouvements de stock</span>
      </button>
      <button type="button" class="tab-btn ${state.stockActiveTab === 'alerts' ? 'active' : ''}" data-action="switch-stock-tab" data-args="${jsArg('alerts')}">
        ${icon('i-alert')} <span>Alertes stock bas</span>
        ${alertsCount > 0 ? `<span class="badge-count">${alertsCount}</span>` : ''}
      </button>
    </div>

    <div id="stockTabContent">
      <p class="hint">${icon('i-clock')} Chargement du magasin…</p>
    </div>
  `;
}

async function initStockPageView() {
  // Charge discrètement les alertes pour actualiser le compteur d'alerte
  api('/api/stock/alerts').then(res => {
    state.stockAlerts = (Array.isArray(res) ? res : (res && res.items ? res.items : [])).map(adaptStock);
    state.partsLowStockCount = state.stockAlerts.length;
    const alertTab = document.querySelector('[data-action="switch-stock-tab"][data-args*="alerts"]');
    if (alertTab) {
      const badge = alertTab.querySelector('.badge-count');
      if (state.stockAlerts.length > 0) {
        if (badge) badge.textContent = String(state.stockAlerts.length);
        else alertTab.insertAdjacentHTML('beforeend', `<span class="badge-count">${state.stockAlerts.length}</span>`);
      } else if (badge) {
        badge.remove();
      }
    }
  }).catch(() => {});

  if (state.stockActiveTab === 'stock') {
    await loadStockLevelsView();
  } else if (state.stockActiveTab === 'movements') {
    await loadStockMovementsListView(true);
  } else if (state.stockActiveTab === 'alerts') {
    await loadStockAlertsView();
  } else {
    state.stockActiveTab = 'catalog';
    await loadPartsListView(true);
  }
}

function partDetailPage(id) {
  return `
    <div id="partDetailContainer">
      <p class="hint">${icon('i-clock')} Chargement des détails de la pièce…</p>
    </div>
  `;
}

async function initPartDetailPageView(id) {
  await loadPartDetail(id);
}

async function loadPartDetail(id) {
  const container = document.getElementById('partDetailContainer');
  if (!container) return;

  state.currentPartLoading = true;
  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement des détails de la pièce…</p>`;

  try {
    const part = await api(`/api/parts/${encodeURIComponent(id)}`);
    state.currentPartDetail = part;
    renderPartDetailView(part);
    loadAndRenderEntityDocuments('part', part.id, 'partDocumentsContainer');
  } catch (err) {
    container.innerHTML = `
      <div class="card">
        <p class="hint state-ko">${icon('i-alert')} Erreur lors du chargement de la pièce : ${esc(err.message)}</p>
        <p><a href="#/stock" class="btn">${icon('i-arrow-left')} <span>Retour au magasin</span></a></p>
      </div>
    `;
  } finally {
    state.currentPartLoading = false;
  }
}

function renderPartDetailView(part) {
  const container = document.getElementById('partDetailContainer');
  if (!container) return;

  const canMgParts = hasPermission('parts.manage');
  const canMvStock = hasPermission('stock.movements');
  const canAdjStock = hasPermission('stock.adjust');

  const totalValueCents = (part.total_stock || 0) * (part.unit_price_cents || 0);

  const quickActions = [];
  if (canMvStock) {
    quickActions.push(`
      <button type="button" class="btn primary" data-action="open-movement-in" data-args="${jsArg({ partId: part.id })}">
        ${icon('i-arrow-down')} <span>Entrée en stock</span>
      </button>
      <button type="button" class="btn" data-action="open-movement-transfer" data-args="${jsArg({ partId: part.id })}">
        ${icon('i-transfer')} <span>Transférer</span>
      </button>
    `);
  }
  if (canAdjStock) {
    quickActions.push(`
      <button type="button" class="btn danger" data-action="open-movement-adjust" data-args="${jsArg({ partId: part.id })}">
        ${icon('i-sliders')} <span>Ajuster</span>
      </button>
    `);
  }
  if (canMgParts) {
    quickActions.push(`
      <button type="button" class="btn" data-action="open-edit-part" data-args="${jsArg(part.id)}">
        ${icon('i-edit')} <span>Modifier</span>
      </button>
      <button type="button" class="btn danger" data-action="delete-part" data-args="${jsArg(part.id)}">
        ${icon('i-trash')} <span>Supprimer</span>
      </button>
    `);
  }

  // Stock par emplacement
  const locationsList = Array.isArray(part.stock_by_location) ? part.stock_by_location : [];
  let locRows = locationsList.map(loc => {
    const isZero = loc.available_quantity <= 0;
    return `
      <tr>
        <td>
          <b>${esc(loc.location_name)}</b>
          <div class="hint mono">${esc(loc.location_code)} (${esc(loc.location_kind)})</div>
        </td>
        <td class="mono font-bold">${esc(loc.quantity)}</td>
        <td class="mono">${esc(loc.reserved_quantity)}</td>
        <td class="mono font-bold ${isZero ? 'state-ko' : ''}">${esc(loc.available_quantity)}</td>
        <td>
          <div class="cell-actions">
            ${canMvStock ? `
              <button type="button" class="btn sm" data-action="open-movement-in" data-args="${jsArg({ partId: part.id, locationId: loc.location_id })}" title="Entrée sur cet emplacement">${icon('i-arrow-down')}</button>
              <button type="button" class="btn sm" data-action="open-movement-transfer" data-args="${jsArg({ partId: part.id, fromLocationId: loc.location_id })}" title="Transférer depuis cet emplacement">${icon('i-transfer')}</button>
            ` : ''}
            ${canAdjStock ? `
              <button type="button" class="btn sm danger" data-action="open-movement-adjust" data-args="${jsArg({ partId: part.id, locationId: loc.location_id })}" title="Ajuster cet emplacement">${icon('i-sliders')}</button>
            ` : ''}
          </div>
        </td>
      </tr>
    `;
  }).join('');

  if (!locationsList.length) {
    locRows = `<tr><td colspan="5" class="empty-cell">Aucun stock répertorié sur aucun emplacement pour cette pièce.</td></tr>`;
  }

  // Équipements associés
  const eqList = Array.isArray(part.equipments) ? part.equipments : [];
  let eqRows = eqList.map(eq => `
    <tr>
      <td>
        <button type="button" class="link-action mono" data-action="open-detail-equipment" data-args="${jsArg(eq.equipment_id)}">
          <b>${esc(eq.equipment_code)}</b> — ${esc(eq.equipment_name)}
        </button>
      </td>
      <td class="mono">${esc(eq.quantity_recommended || 1)}</td>
      <td>${esc(eq.notes || '—')}</td>
      <td>
        <div class="cell-actions">
          <button type="button" class="btn sm" data-action="open-detail-equipment" data-args="${jsArg(eq.equipment_id)}" title="Voir l'équipement">${icon('i-eye')}</button>
          ${canMgParts ? `
            <button type="button" class="btn sm danger" data-action="delete-part-equipment" data-args="${jsArg({ partId: part.id, equipmentId: eq.equipment_id })}" title="Dissocier cet équipement">${icon('i-trash')}</button>
          ` : ''}
        </div>
      </td>
    </tr>
  `).join('');

  if (!eqList.length) {
    eqRows = `<tr><td colspan="4" class="empty-cell">Cette pièce n'est préconisée pour aucun équipement du parc.</td></tr>`;
  }

  // Mouvements récents
  const mvts = Array.isArray(part.recent_movements) ? part.recent_movements : [];
  let mvtRows = mvts.map(m => {
    let locStr = '—';
    if (m.movement_type === 'in') locStr = `Vers ${esc(m.to_location ? m.to_location.name : '—')}`;
    else if (m.movement_type === 'out') locStr = `Depuis ${esc(m.from_location ? m.from_location.name : '—')}`;
    else if (m.movement_type === 'transfer') locStr = `${esc(m.from_location ? m.from_location.name : '—')} → ${esc(m.to_location ? m.to_location.name : '—')}`;
    else if (m.movement_type === 'adjustment') locStr = `Emplacement ${esc(m.from_location ? m.from_location.name : (m.to_location ? m.to_location.name : '—'))}`;

    let qtyStr = '';
    if (m.movement_type === 'in') qtyStr = `<span class="state-ok font-bold">+${esc(m.quantity)}</span>`;
    else if (m.movement_type === 'out') qtyStr = `<span class="state-ko font-bold">-${esc(m.quantity)}</span>`;
    else if (m.movement_type === 'adjustment') qtyStr = m.delta > 0 ? `<span class="state-ok font-bold">+${esc(m.delta)}</span>` : `<span class="state-ko font-bold">${esc(m.delta)}</span>`;
    else qtyStr = `<span class="mono font-bold">${esc(m.quantity)}</span>`;

    let contextStr = '—';
    if (m.intervention) {
      contextStr = `<a href="#/interventions/${esc(m.intervention.id)}" class="link-action mono">${esc(m.intervention.code)}</a>`;
    } else if (m.adjustment_kind) {
      contextStr = `<span class="badge">${esc(formatAdjustmentKind(m.adjustment_kind))}</span>`;
    }

    return `
      <tr>
        <td class="mono">${formatDate(m.created_at)}</td>
        <td>${movementTypeBadge(m.movement_type)}</td>
        <td>${locStr}</td>
        <td class="mono">${qtyStr}</td>
        <td class="mono">${formatCents(m.unit_price_cents)}</td>
        <td>${contextStr}</td>
        <td>${esc(m.created_by ? m.created_by.display_name : '—')}</td>
      </tr>
    `;
  }).join('');

  if (!mvts.length) {
    mvtRows = `<tr><td colspan="7" class="empty-cell">Aucun mouvement enregistré pour cette référence.</td></tr>`;
  }

  container.innerHTML = `
    <div class="breadcrumb">
      <a href="#/stock" class="link-action">${icon('i-basket')} Magasin & Stock</a>
      <span>/</span>
      <span class="mono">${esc(part.reference)}</span>
    </div>

    <!-- CARTE D'EN-TÊTE -->
    <section class="detail-header-card">
      <div class="detail-header-top">
        <div class="detail-title-block">
          <div class="detail-code-row">
            <span class="detail-code-label mono">${esc(part.reference)}</span>
            <span class="detail-title-label">${esc(part.name)}</span>
          </div>
          <div class="detail-badges-row">
            <span class="badge">${esc(formatPartCategory(part.category))}</span>
            ${part.barcode ? `<span class="badge mono">${icon('i-barcode')} ${esc(part.barcode)}</span>` : ''}
            ${partStockBadge(part.total_stock, part.effective_threshold)}
          </div>
        </div>
      </div>
      ${quickActions.length ? `<div class="quick-actions-bar">${quickActions.join('')}</div>` : ''}
    </section>

    <!-- INDICATEURS CLÉS / MÉTRIQUES -->
    <section class="metrics-row">
      <div class="metric-card">
        <div class="metric-label">Stock total</div>
        <div class="metric-value mono">${esc(part.total_stock)} <small>${esc(formatPartUnit(part.unit))}</small></div>
      </div>
      <div class="metric-card">
        <div class="metric-label">Stock réservé</div>
        <div class="metric-value mono">${esc(part.reserved_stock || 0)}</div>
      </div>
      <div class="metric-card">
        <div class="metric-label">Stock disponible</div>
        <div class="metric-value mono font-bold ${part.available_stock <= 0 ? 'state-ko' : ''}">${esc(part.available_stock !== undefined ? part.available_stock : part.total_stock)}</div>
      </div>
      <div class="metric-card">
        <div class="metric-label">Prix moyen pondéré (PMP)</div>
        <div class="metric-value mono font-bold">${formatCents(part.unit_price_cents)}</div>
      </div>
      <div class="metric-card">
        <div class="metric-label">Valeur totale en stock</div>
        <div class="metric-value mono font-bold">${formatCents(totalValueCents)}</div>
      </div>
    </section>

    <!-- GRILLE D'INFORMATIONS -->
    <div class="detail-grid">
      <!-- INFORMATIONS GÉNÉRALES -->
      <section class="card">
        <h2>${icon('i-info')} Fiche technique</h2>
        <dl class="detail-dl">
          <dt>Catégorie</dt><dd>${esc(formatPartCategory(part.category))}</dd>
          <dt>Unité de gestion</dt><dd>${esc(formatPartUnit(part.unit))}</dd>
          <dt>Code-barres / EAN</dt><dd class="mono">${esc(part.barcode || '—')}</dd>
          <dt>Fournisseur habituel</dt><dd>${part.supplier ? esc(part.supplier.name) : '—'}</dd>
          <dt>Référence fournisseur</dt><dd class="mono">${esc(part.supplier_reference || '—')}</dd>
          <dt>Fabricant d'origine</dt><dd>${esc(part.manufacturer || '—')}</dd>
          <dt>Référence fabricant</dt><dd class="mono">${esc(part.manufacturer_reference || '—')}</dd>
          <dt>Seuil d'alerte spécifique</dt><dd class="mono">${part.alert_threshold !== null && part.alert_threshold !== undefined ? esc(part.alert_threshold) : 'Non défini (utilise le seuil global)'}</dd>
          <dt>Seuil effectif appliqué</dt><dd class="mono font-bold">${esc(part.effective_threshold)}</dd>
          <dt>Créée le</dt><dd>${formatDate(part.created_at)}</dd>
          <dt>Description</dt><dd>${esc(part.description || 'Aucune description complémentaire.')}</dd>
        </dl>
      </section>

      <!-- STOCK PAR EMPLACEMENT -->
      <section class="card">
        <div class="detail-nav-bar">
          <h2>${icon('i-layers')} Répartition par emplacement</h2>
        </div>
        <div class="table-container">
          <table class="data-table">
            <thead>
              <tr>
                <th>Emplacement</th>
                <th>Total</th>
                <th>Réservé</th>
                <th>Disponible</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              ${locRows}
            </tbody>
          </table>
        </div>
      </section>
    </div>

    <!-- ÉQUIPEMENTS ASSOCIÉS -->
    <section class="card">
      <div class="detail-nav-bar">
        <h2>${icon('i-gear')} Équipements compatibles préconisés (${esc(eqList.length)})</h2>
        ${canMgParts ? `
          <button type="button" class="btn sm" data-action="open-add-part-equipment" data-args="${jsArg(part.id)}">
            ${icon('i-plus')} <span>Associer un équipement</span>
          </button>
        ` : ''}
      </div>
      <div class="table-container">
        <table class="data-table">
          <thead>
            <tr>
              <th>Équipement</th>
              <th>Quantité préconisée</th>
              <th>Notes d'assemblage</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            ${eqRows}
          </tbody>
        </table>
      </div>
    </section>

    <!-- HISTORIQUE DES MOUVEMENTS RÉCENTS -->
    <section class="card">
      <div class="detail-nav-bar">
        <h2>${icon('i-transfer')} Mouvements récents</h2>
      </div>
      <div class="table-container">
        <table class="data-table">
          <thead>
            <tr>
              <th>Date & Heure</th>
              <th>Type</th>
              <th>Emplacement(s)</th>
              <th>Quantité</th>
              <th>PMP (€)</th>
              <th>Contexte</th>
              <th>Auteur</th>
            </tr>
          </thead>
          <tbody>
            ${mvtRows}
          </tbody>
        </table>
      </div>
    </section>

    <!-- SECTION DOCUMENTS (PHASE 6) -->
    <div id="partDocumentsContainer"></div>
  `;
}

// --- SOUS-VUES DU MAGASIN ---

async function loadPartsListView(reset = true) {
  const container = document.getElementById('stockTabContent');
  if (!container) return;

  if (reset) {
    state.parts = [];
    state.partsNextCursor = null;
    container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement du catalogue des pièces…</p>`;
  }

  state.partsLoading = true;
  try {
    const params = new URLSearchParams();
    if (state.selectedSiteId) params.set('site_id', state.selectedSiteId);
    if (state.partsSearchQuery) params.set('q', state.partsSearchQuery);
    if (state.partsCategoryFilter) params.set('category', state.partsCategoryFilter);
    if (state.partsSupplierFilter) params.set('supplier_id', state.partsSupplierFilter);
    if (state.partsLowStockFilter) params.set('low_stock', 'true');
    if (!reset && state.partsNextCursor) params.set('cursor', state.partsNextCursor);
    params.set('limit', '25');

    const res = await api(`/api/parts?${params.toString()}`);
    const items = (Array.isArray(res) ? res : (res && res.items ? res.items : [])).map(adaptStock);
    state.partsNextCursor = res && res.next_cursor ? res.next_cursor : null;

    if (reset) {
      state.parts = items;
    } else {
      state.parts = state.parts.concat(items);
    }

    renderPartsListView();
  } catch (err) {
    if (reset) {
      container.innerHTML = `<p class="hint state-ko">Erreur lors du chargement des pièces : ${esc(err.message)}</p>`;
    } else {
      notify(err.message || 'Erreur lors du chargement de la suite.', 'error');
    }
  } finally {
    state.partsLoading = false;
  }
}

function renderPartsListView() {
  const container = document.getElementById('stockTabContent');
  if (!container) return;

  const canMgParts = hasPermission('parts.manage');
  const items = state.parts;

  let rows = items.map(p => {
    const isLow = typeof p.is_low_stock === 'boolean' ? p.is_low_stock : (p.total_stock <= (p.alert_threshold || 0));
    const isZero = p.total_stock <= 0;
    const badgeStock = isZero
      ? `<span class="badge badge-stock-zero">Rupture (${esc(p.total_stock)})</span>`
      : (isLow
        ? `<span class="badge badge-stock-low">Stock bas (${esc(p.total_stock)})</span>`
        : `<span class="badge badge-stock-ok">En stock (${esc(p.total_stock)})</span>`);

    return `
      <tr>
        <td>
          <a href="#/pieces/${esc(p.id)}" class="link-action mono"><b>${esc(p.reference)}</b></a>
          ${p.barcode ? `<div class="hint mono">${icon('i-barcode')} ${esc(p.barcode)}</div>` : ''}
        </td>
        <td>
          <a href="#/pieces/${esc(p.id)}" class="link-action"><b>${esc(p.name)}</b></a>
          ${p.description ? `<div class="hint">${esc(p.description.slice(0, 70))}${p.description.length > 70 ? '…' : ''}</div>` : ''}
        </td>
        <td><span class="badge">${esc(formatPartCategory(p.category))}</span></td>
        <td>${p.supplier ? esc(p.supplier.name) : '<span class="hint">—</span>'}</td>
        <td class="mono font-bold">${formatCents(p.unit_price_cents)}</td>
        <td class="mono"><b>${esc(p.total_stock)}</b> ${esc(formatPartUnit(p.unit))}</td>
        <td>${badgeStock}</td>
        <td>
          <div class="cell-actions">
            <a href="#/pieces/${esc(p.id)}" class="btn sm" title="Consulter la fiche">${icon('i-eye')} <span>Voir</span></a>
            ${canMgParts ? `
              <button type="button" class="btn sm" data-action="open-edit-part" data-args="${jsArg(p.id)}" title="Modifier la référence">${icon('i-edit')}</button>
              <button type="button" class="btn sm danger" data-action="delete-part" data-args="${jsArg(p.id)}" title="Supprimer la référence">${icon('i-trash')}</button>
            ` : ''}
          </div>
        </td>
      </tr>
    `;
  }).join('');

  if (!items.length) {
    rows = `<tr><td colspan="8" class="empty-cell">Aucune pièce détachée trouvée.</td></tr>`;
  }

  container.innerHTML = `
    <div class="list-toolbar">
      <div class="toolbar-search">
        <input type="search" placeholder="Rechercher par référence, désignation, code-barres…" value="${esc(state.partsSearchQuery)}" data-action="filter-parts-search" autocomplete="off" />
      </div>
      <div class="toolbar-filters">
        <select data-action="filter-parts-category">
          <option value="">Toutes catégories</option>
          <option value="roulement" ${state.partsCategoryFilter === 'roulement' ? 'selected' : ''}>Roulement</option>
          <option value="visserie" ${state.partsCategoryFilter === 'visserie' ? 'selected' : ''}>Visserie / Fixation</option>
          <option value="moteur" ${state.partsCategoryFilter === 'moteur' ? 'selected' : ''}>Moteur / Réducteur</option>
          <option value="pneumatique" ${state.partsCategoryFilter === 'pneumatique' ? 'selected' : ''}>Pneumatique</option>
          <option value="hydraulique" ${state.partsCategoryFilter === 'hydraulique' ? 'selected' : ''}>Hydraulique</option>
          <option value="electrique" ${state.partsCategoryFilter === 'electrique' ? 'selected' : ''}>Électrique</option>
          <option value="capteur" ${state.partsCategoryFilter === 'capteur' ? 'selected' : ''}>Capteur / Détection</option>
          <option value="mecanique" ${state.partsCategoryFilter === 'mecanique' ? 'selected' : ''}>Mécanique</option>
          <option value="consommable" ${state.partsCategoryFilter === 'consommable' ? 'selected' : ''}>Consommable</option>
        </select>
        <label class="checkbox-label">
          <input type="checkbox" data-action="filter-parts-low-stock" ${state.partsLowStockFilter ? 'checked' : ''} />
          <span>${icon('i-alert')} Alertes stock bas uniquement</span>
        </label>
      </div>
      <div class="toolbar-actions">
        ${canMgParts ? `
          <button type="button" class="btn primary" data-action="open-create-part">
            ${icon('i-plus')} <span>Nouvelle pièce</span>
          </button>
        ` : ''}
      </div>
    </div>

    <div class="table-container">
      <table class="data-table">
        <thead>
          <tr>
            <th>Référence</th>
            <th>Désignation</th>
            <th>Catégorie</th>
            <th>Fournisseur</th>
            <th>PMP unitaire HT</th>
            <th>Stock total</th>
            <th>État stock</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          ${rows}
        </tbody>
      </table>
    </div>

    ${state.partsNextCursor ? `
      <div class="pagination-bar">
        <button type="button" class="btn" data-action="load-more-parts">
          ${icon('i-arrow-down')} <span>Charger plus de pièces</span>
        </button>
      </div>
    ` : ''}
  `;
}

async function loadStockLevelsView() {
  const container = document.getElementById('stockTabContent');
  if (!container) return;

  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement des niveaux de stock par emplacement…</p>`;
  state.stockLevelsLoading = true;

  try {
    const params = new URLSearchParams();
    if (state.selectedSiteId) params.set('site_id', state.selectedSiteId);
    if (state.stockLevelsLocationFilter) params.set('location_id', state.stockLevelsLocationFilter);

    const [stockRes, locRes] = await Promise.all([
      api(`/api/stock?${params.toString()}`),
      state.locations && state.locations.length ? Promise.resolve(state.locations) : api(`/api/locations${state.selectedSiteId ? `?site_id=${encodeURIComponent(state.selectedSiteId)}` : ''}`)
    ]);

    state.stockLevels = (Array.isArray(stockRes) ? stockRes : (stockRes && stockRes.items ? stockRes.items : [])).map(adaptStock);
    if (Array.isArray(locRes)) state.locations = locRes;
    else if (locRes && Array.isArray(locRes.items)) state.locations = locRes.items;

    renderStockLevelsView();
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">Erreur lors du chargement du stock : ${esc(err.message)}</p>`;
  } finally {
    state.stockLevelsLoading = false;
  }
}

function renderStockLevelsView() {
  const container = document.getElementById('stockTabContent');
  if (!container) return;

  const canMvStock = hasPermission('stock.movements');
  const canAdjStock = hasPermission('stock.adjust');
  const items = state.stockLevels;
  const locations = Array.isArray(state.locations) ? state.locations : [];

  let rows = items.map(s => {
    const isLow = s.available <= (s.effective_threshold !== undefined && s.effective_threshold !== null ? s.effective_threshold : (s.alert_threshold || 0));
    const isZero = s.available <= 0;
    const badge = isZero
      ? `<span class="badge badge-stock-zero">Rupture</span>`
      : (isLow ? `<span class="badge badge-stock-low">Stock bas</span>` : `<span class="badge badge-stock-ok">Normal</span>`);

    return `
      <tr>
        <td>
          <a href="#/pieces/${esc(s.part_id)}" class="link-action mono"><b>${esc(s.part ? s.part.reference : '—')}</b></a>
          <div>${esc(s.part ? s.part.name : '—')}</div>
        </td>
        <td><b>${esc(s.location ? s.location.name : '—')}</b></td>
        <td class="mono font-bold">${esc(s.quantity)}</td>
        <td class="mono">${esc(s.reserved || 0)}</td>
        <td class="mono font-bold">${esc(s.available)}</td>
        <td class="mono">${esc(s.effective_threshold !== null && s.effective_threshold !== undefined ? s.effective_threshold : (s.alert_threshold || '—'))}</td>
        <td>${badge}</td>
        <td>
          <div class="cell-actions">
            ${canMvStock ? `
              <button type="button" class="btn sm" data-action="open-movement-in" data-args="${jsArg({ partId: s.part_id, locationId: s.location_id })}" title="Réception / Entrée">${icon('i-arrow-down')} <span>Entrée</span></button>
              <button type="button" class="btn sm" data-action="open-movement-transfer" data-args="${jsArg({ partId: s.part_id, fromLocationId: s.location_id })}" title="Transférer vers un autre emplacement">${icon('i-transfer')} <span>Transférer</span></button>
            ` : ''}
            ${canAdjStock ? `
              <button type="button" class="btn sm danger" data-action="open-movement-adjust" data-args="${jsArg({ partId: s.part_id, locationId: s.location_id })}" title="Ajustement d'inventaire">${icon('i-sliders')} <span>Ajuster</span></button>
            ` : ''}
          </div>
        </td>
      </tr>
    `;
  }).join('');

  if (!items.length) {
    rows = `<tr><td colspan="8" class="empty-cell">Aucun stock répertorié sur cet emplacement.</td></tr>`;
  }

  container.innerHTML = `
    <div class="list-toolbar">
      <div class="toolbar-filters">
        <select data-action="filter-stock-location">
          <option value="">Tous les emplacements</option>
          ${locations.map(l => `<option value="${esc(l.id)}" ${l.id === state.stockLevelsLocationFilter ? 'selected' : ''}>${esc(l.name)}</option>`).join('')}
        </select>
      </div>
      <div class="toolbar-actions">
        ${canMvStock ? `
          <button type="button" class="btn primary" data-action="open-movement-in">
            ${icon('i-arrow-down')} <span>Entrée en stock</span>
          </button>
          <button type="button" class="btn" data-action="open-movement-transfer">
            ${icon('i-transfer')} <span>Transfert interne</span>
          </button>
        ` : ''}
        ${canAdjStock ? `
          <button type="button" class="btn danger" data-action="open-movement-adjust">
            ${icon('i-sliders')} <span>Ajustement inventaire</span>
          </button>
        ` : ''}
      </div>
    </div>

    <div class="table-container">
      <table class="data-table">
        <thead>
          <tr>
            <th>Pièce détachée</th>
            <th>Emplacement</th>
            <th>Stock total</th>
            <th>Réservé</th>
            <th>Disponible</th>
            <th>Seuil alerte</th>
            <th>Statut</th>
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

async function loadStockMovementsListView(reset = true) {
  const container = document.getElementById('stockTabContent');
  if (!container) return;

  if (reset) {
    state.stockMovements = [];
    state.stockMovementsNextCursor = null;
    container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement des mouvements de stock…</p>`;
  }

  state.stockMovementsLoading = true;
  try {
    const params = new URLSearchParams();
    if (state.selectedSiteId) params.set('site_id', state.selectedSiteId);
    if (state.stockMovementsTypeFilter) params.set('movement_type', state.stockMovementsTypeFilter);
    if (!reset && state.stockMovementsNextCursor) params.set('cursor', state.stockMovementsNextCursor);
    params.set('limit', '25');

    const res = await api(`/api/stock/movements?${params.toString()}`);
    const items = (Array.isArray(res) ? res : (res && res.items ? res.items : [])).map(adaptStock);
    state.stockMovementsNextCursor = res && res.next_cursor ? res.next_cursor : null;

    if (reset) {
      state.stockMovements = items;
    } else {
      state.stockMovements = state.stockMovements.concat(items);
    }

    renderStockMovementsListView();
  } catch (err) {
    if (reset) {
      container.innerHTML = `<p class="hint state-ko">Erreur lors du chargement des mouvements : ${esc(err.message)}</p>`;
    } else {
      notify(err.message || 'Erreur lors du chargement de la suite.', 'error');
    }
  } finally {
    state.stockMovementsLoading = false;
  }
}

function renderStockMovementsListView() {
  const container = document.getElementById('stockTabContent');
  if (!container) return;

  const canMvStock = hasPermission('stock.movements');
  const canAdjStock = hasPermission('stock.adjust');
  const items = state.stockMovements;

  let rows = items.map(m => {
    let locStr = '—';
    if (m.movement_type === 'in') {
      locStr = `Vers <b>${esc(m.to_location ? m.to_location.name : '—')}</b>`;
    } else if (m.movement_type === 'out') {
      locStr = `Depuis <b>${esc(m.from_location ? m.from_location.name : '—')}</b>`;
    } else if (m.movement_type === 'transfer') {
      locStr = `${esc(m.from_location ? m.from_location.name : '—')} → <b>${esc(m.to_location ? m.to_location.name : '—')}</b>`;
    } else if (m.movement_type === 'adjustment') {
      locStr = `Emplacement <b>${esc(m.from_location ? m.from_location.name : (m.to_location ? m.to_location.name : '—'))}</b>`;
    }

    let qtyStr = '';
    if (m.movement_type === 'in') {
      qtyStr = `<span class="state-ok font-bold">+${esc(m.quantity)}</span>`;
    } else if (m.movement_type === 'out') {
      qtyStr = `<span class="state-ko font-bold">-${esc(m.quantity)}</span>`;
    } else if (m.movement_type === 'adjustment') {
      qtyStr = m.delta > 0
        ? `<span class="state-ok font-bold">+${esc(m.delta)}</span>`
        : `<span class="state-ko font-bold">${esc(m.delta)}</span>`;
    } else {
      qtyStr = `<span class="mono font-bold">${esc(m.quantity)}</span>`;
    }

    let contextStr = '—';
    if (m.intervention) {
      contextStr = `
        <a href="#/interventions/${esc(m.intervention.id)}" class="link-action mono"><b>${esc(m.intervention.code)}</b></a>
        <div class="hint">${esc(m.intervention.title || '')}</div>
      `;
    } else if (m.adjustment_kind) {
      contextStr = `
        <span class="badge">${esc(formatAdjustmentKind(m.adjustment_kind))}</span>
        ${m.reason ? `<div class="hint">« ${esc(m.reason)} »</div>` : ''}
      `;
    }

    return `
      <tr>
        <td class="mono">${formatDate(m.created_at)}</td>
        <td>${movementTypeBadge(m.movement_type)}</td>
        <td>
          <a href="#/pieces/${esc(m.part_id)}" class="link-action mono"><b>${esc(m.part ? m.part.reference : '—')}</b></a>
          <div>${esc(m.part ? m.part.name : '—')}</div>
        </td>
        <td>${locStr}</td>
        <td class="mono">${qtyStr}</td>
        <td class="mono">${formatCents(m.unit_price_cents)}</td>
        <td>${contextStr}</td>
        <td>${esc(m.created_by ? m.created_by.display_name : '—')}</td>
      </tr>
    `;
  }).join('');

  if (!items.length) {
    rows = `<tr><td colspan="8" class="empty-cell">Aucun mouvement enregistré.</td></tr>`;
  }

  container.innerHTML = `
    <div class="list-toolbar">
      <div class="toolbar-filters">
        <select data-action="filter-stock-movements-type">
          <option value="">Tous les types de mouvements</option>
          <option value="in" ${state.stockMovementsTypeFilter === 'in' ? 'selected' : ''}>Entrées en stock</option>
          <option value="out" ${state.stockMovementsTypeFilter === 'out' ? 'selected' : ''}>Sorties interventions</option>
          <option value="transfer" ${state.stockMovementsTypeFilter === 'transfer' ? 'selected' : ''}>Transferts internes</option>
          <option value="adjustment" ${state.stockMovementsTypeFilter === 'adjustment' ? 'selected' : ''}>Ajustements d'inventaire</option>
        </select>
      </div>
      <div class="toolbar-actions">
        ${canMvStock ? `
          <button type="button" class="btn primary" data-action="open-movement-in">
            ${icon('i-arrow-down')} <span>Nouvelle entrée</span>
          </button>
          <button type="button" class="btn" data-action="open-movement-transfer">
            ${icon('i-transfer')} <span>Transférer</span>
          </button>
        ` : ''}
        ${canAdjStock ? `
          <button type="button" class="btn danger" data-action="open-movement-adjust">
            ${icon('i-sliders')} <span>Ajuster</span>
          </button>
        ` : ''}
      </div>
    </div>

    <div class="table-container">
      <table class="data-table">
        <thead>
          <tr>
            <th>Date & Heure</th>
            <th>Type</th>
            <th>Pièce détachée</th>
            <th>Emplacement(s)</th>
            <th>Quantité</th>
            <th>PMP unitaire HT</th>
            <th>Contexte / Motif</th>
            <th>Auteur</th>
          </tr>
        </thead>
        <tbody>
          ${rows}
        </tbody>
      </table>
    </div>

    ${state.stockMovementsNextCursor ? `
      <div class="pagination-bar">
        <button type="button" class="btn" data-action="load-more-stock-movements">
          ${icon('i-arrow-down')} <span>Charger plus de mouvements</span>
        </button>
      </div>
    ` : ''}
  `;
}

async function loadStockAlertsView() {
  const container = document.getElementById('stockTabContent');
  if (!container) return;

  container.innerHTML = `<p class="hint">${icon('i-clock')} Analyse des niveaux de stock et recherche des alertes…</p>`;
  state.stockAlertsLoading = true;

  try {
    const params = new URLSearchParams();
    if (state.selectedSiteId) params.set('site_id', state.selectedSiteId);

    const res = await api(`/api/stock/alerts?${params.toString()}`);
    state.stockAlerts = (Array.isArray(res) ? res : (res && res.items ? res.items : [])).map(adaptStock);
    state.partsLowStockCount = state.stockAlerts.length;

    renderStockAlertsView();
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">Erreur lors de la détection des alertes : ${esc(err.message)}</p>`;
  } finally {
    state.stockAlertsLoading = false;
  }
}

function renderStockAlertsView() {
  const container = document.getElementById('stockTabContent');
  if (!container) return;

  const canMvStock = hasPermission('stock.movements');
  const items = state.stockAlerts;

  if (!items.length) {
    container.innerHTML = `
      <div class="card card-empty-state">
        ${icon('i-check')}
        <h2>Aucune alerte de stock bas</h2>
        <p class="hint">Toutes les pièces détachées enregistrées sont actuellement au-dessus de leur seuil d'alerte. Aucun approvisionnement d'urgence n'est requis.</p>
      </div>
    `;
    return;
  }

  let rows = items.map(a => {
    const deficit = Math.max(0, (a.effective_threshold || 0) - a.total_stock);
    const isZero = a.total_stock <= 0;
    const badge = isZero
      ? `<span class="badge badge-stock-zero">Rupture totale</span>`
      : `<span class="badge badge-stock-low">Stock bas critique</span>`;

    return `
      <tr>
        <td>
          <a href="#/pieces/${esc(a.id)}" class="link-action mono"><b>${esc(a.reference)}</b></a>
        </td>
        <td>
          <a href="#/pieces/${esc(a.id)}" class="link-action"><b>${esc(a.name)}</b></a>
          ${a.supplier ? `<div class="hint">Fournisseur : ${esc(a.supplier.name)}</div>` : ''}
        </td>
        <td><span class="badge">${esc(formatPartCategory(a.category))}</span></td>
        <td class="mono font-bold ${isZero ? 'state-ko' : ''}">${esc(a.total_stock)} ${esc(formatPartUnit(a.unit))}</td>
        <td class="mono font-bold">${esc(a.available_stock !== undefined ? a.available_stock : a.total_stock)}</td>
        <td class="mono font-bold">${esc(a.effective_threshold)}</td>
        <td class="mono state-ko font-bold">+${esc(deficit)}</td>
        <td>${badge}</td>
        <td>
          <div class="cell-actions">
            ${canMvStock ? `
              <button type="button" class="btn sm primary" data-action="open-movement-in" data-args="${jsArg({ partId: a.id })}">
                ${icon('i-arrow-down')} <span>Réceptionner</span>
              </button>
            ` : ''}
            <a href="#/pieces/${esc(a.id)}" class="btn sm" title="Voir la fiche">${icon('i-eye')}</a>
          </div>
        </td>
      </tr>
    `;
  }).join('');

  container.innerHTML = `
    <div class="stock-warning-alert">
      ${icon('i-alert')}
      <div>
        <b>${esc(items.length)} référence(s) en alerte de réapprovisionnement</b>
        <div>Le stock disponible ou total de ces pièces est inférieur ou égal au seuil paramétré. Lancez les commandes de réapprovisionnement auprès de vos fournisseurs.</div>
      </div>
    </div>

    <div class="table-container">
      <table class="data-table">
        <thead>
          <tr>
            <th>Référence</th>
            <th>Désignation</th>
            <th>Catégorie</th>
            <th>Stock total</th>
            <th>Disponible</th>
            <th>Seuil alerte</th>
            <th>Déficit min.</th>
            <th>Gravité</th>
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

// --- BOÎTES DE DIALOGUE MODALES (PHASE 4) ---

async function openCreatePartModal() {
  let suppliers = [];
  const siteId = state.selectedSiteId || (state.site ? state.site.id : '');

  try {
    const siteParam = siteId ? `?site_id=${encodeURIComponent(siteId)}&limit=100` : '?limit=100';
    const supRes = await api(`/api/suppliers${siteParam}`);
    suppliers = Array.isArray(supRes) ? supRes : (supRes && Array.isArray(supRes.items) ? supRes.items : []);
  } catch (_) {}

  const fieldsHtml = `
    ${renderSiteFormField(siteId)}
    <div class="form-row">
      <div class="form-group">
        <label for="pRef">Référence article *</label>
        <input type="text" id="pRef" name="reference" required autocomplete="off" autofocus placeholder="ex. ROUL-6204-2RS" />
        <span class="form-help">Code unique par site.</span>
      </div>
      <div class="form-group">
        <label for="pName">Désignation *</label>
        <input type="text" id="pName" name="name" required autocomplete="off" placeholder="ex. Roulement à billes étanche" />
      </div>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="pCategory">Catégorie *</label>
        <select id="pCategory" name="category" required>
          <option value="roulement">Roulement</option>
          <option value="visserie">Visserie / Fixation</option>
          <option value="moteur">Moteur / Réducteur</option>
          <option value="pneumatique">Pneumatique</option>
          <option value="hydraulique">Hydraulique</option>
          <option value="electrique">Électrique</option>
          <option value="capteur">Capteur / Détection</option>
          <option value="mecanique" selected>Mécanique</option>
          <option value="consommable">Consommable</option>
        </select>
      </div>
      <div class="form-group">
        <label for="pUnit">Unité de gestion *</label>
        <select id="pUnit" name="unit" required>
          <option value="piece" selected>Pièce(s)</option>
          <option value="metre">Mètre(s)</option>
          <option value="litre">Litre(s)</option>
          <option value="kg">Kg</option>
          <option value="lot">Lot(s)</option>
          <option value="boite">Boîte(s)</option>
        </select>
      </div>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="pPrice">Prix unitaire initial (€ HT)</label>
        <input type="number" step="0.01" min="0" id="pPrice" name="unit_price" placeholder="0.00" />
        <span class="form-help">Converti en centimes pour le calcul du PMP.</span>
      </div>
      <div class="form-group">
        <label for="pAlertThreshold">Seuil d'alerte stock bas</label>
        <input type="number" min="0" id="pAlertThreshold" name="alert_threshold" placeholder="Par défaut : 5" />
        <span class="form-help">Laissez vide pour appliquer le seuil global.</span>
      </div>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="pSupplier">Fournisseur habituel</label>
        <select id="pSupplier" name="supplier_id">
          <option value="">-- Aucun fournisseur --</option>
          ${suppliers.map(s => `<option value="${esc(s.id)}">${esc(s.name)}</option>`).join('')}
        </select>
      </div>
      <div class="form-group">
        <label for="pSupplierRef">Réf. fournisseur</label>
        <input type="text" id="pSupplierRef" name="supplier_reference" placeholder="ex. SKF-6204-2RSH" />
      </div>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="pManufacturer">Fabricant</label>
        <input type="text" id="pManufacturer" name="manufacturer" placeholder="ex. SKF, Festo, SMC…" />
      </div>
      <div class="form-group">
        <label for="pManufacturerRef">Réf. fabricant</label>
        <input type="text" id="pManufacturerRef" name="manufacturer_reference" placeholder="ex. 6204-2RSH" />
      </div>
    </div>
    <div class="form-group">
      <label for="pBarcode">Code-barres / EAN</label>
      <input type="text" id="pBarcode" name="barcode" placeholder="ex. 7316570534201" />
    </div>
    <div class="form-group">
      <label for="pDesc">Description technique / Dimensions</label>
      <textarea id="pDesc" name="description" rows="2" placeholder="Spécifications, cotes, compatibilité…"></textarea>
    </div>
  `;

  openFormModal({
    title: 'Nouvelle pièce détachée',
    formId: 'modalCreatePartForm',
    submitAction: 'create-part-submit',
    submitLabel: 'Créer la pièce',
    fieldsHtml,
  });
}

async function handleCreatePartSubmit(form) {
  const btn = document.getElementById('modalCreatePartFormSubmitBtn');
  const errBox = document.getElementById('modalCreatePartFormErrorBox');
  const errText = document.getElementById('modalCreatePartFormErrorBoxText');

  const site_id = form.site_id ? form.site_id.value : (state.site ? state.site.id : null);
  const reference = (form.reference.value || '').trim();
  const name = (form.name.value || '').trim();
  const category = form.category.value;
  const unit = form.unit.value;
  const unit_price_cents = eurosToCents(form.unit_price.value);
  const alert_threshold = form.alert_threshold.value !== '' ? parseInt(form.alert_threshold.value, 10) : null;
  const supplier_id = form.supplier_id.value || null;
  const supplier_reference = (form.supplier_reference.value || '').trim() || null;
  const manufacturer = (form.manufacturer.value || '').trim() || null;
  const manufacturer_reference = (form.manufacturer_reference.value || '').trim() || null;
  const barcode = (form.barcode.value || '').trim() || null;
  const description = (form.description.value || '').trim() || null;

  btn.disabled = true;
  errBox.hidden = true;

  try {
    const payload = {
      reference,
      name,
      category,
      unit,
      unit_price_cents,
      alert_threshold,
      supplier_id,
      supplier_reference,
      manufacturer,
      manufacturer_reference,
      barcode,
      description,
    };

    if (isAdmin()) {
      if (!site_id) throw new Error('Veuillez sélectionner un site.');
      payload.site_id = site_id;
    }

    const created = await api('/api/parts', {
      method: 'POST',
      body: payload,
    });

    notify(`Pièce « ${reference} » créée avec succès.`, 'success');
    closeModal();
    if (state.stockActiveTab === 'catalog') {
      await loadPartsListView(true);
    } else {
      location.hash = `#/pieces/${encodeURIComponent(created.id)}`;
    }
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || 'Impossible de créer la pièce.';
  }
}

async function openEditPartModal(partId) {
  let part = state.currentPartDetail && state.currentPartDetail.id === partId ? state.currentPartDetail : null;
  if (!part) {
    part = state.parts.find(p => p.id === partId);
  }
  if (!part) {
    try {
      part = await api(`/api/parts/${encodeURIComponent(partId)}`);
    } catch (err) {
      notify('Impossible de charger la pièce : ' + err.message, 'error');
      return;
    }
  }

  let suppliers = [];
  try {
    const siteParam = part.site_id ? `?site_id=${encodeURIComponent(part.site_id)}&limit=100` : '?limit=100';
    const supRes = await api(`/api/suppliers${siteParam}`);
    suppliers = Array.isArray(supRes) ? supRes : (supRes && Array.isArray(supRes.items) ? supRes.items : []);
  } catch (_) {}

  const fieldsHtml = `
    <div class="form-row">
      <div class="form-group">
        <label for="epRef">Référence article *</label>
        <input type="text" id="epRef" name="reference" required autocomplete="off" value="${esc(part.reference)}" />
      </div>
      <div class="form-group">
        <label for="epName">Désignation *</label>
        <input type="text" id="epName" name="name" required autocomplete="off" value="${esc(part.name)}" />
      </div>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="epCategory">Catégorie *</label>
        <select id="epCategory" name="category" required>
          <option value="roulement" ${part.category === 'roulement' ? 'selected' : ''}>Roulement</option>
          <option value="visserie" ${part.category === 'visserie' ? 'selected' : ''}>Visserie / Fixation</option>
          <option value="moteur" ${part.category === 'moteur' ? 'selected' : ''}>Moteur / Réducteur</option>
          <option value="pneumatique" ${part.category === 'pneumatique' ? 'selected' : ''}>Pneumatique</option>
          <option value="hydraulique" ${part.category === 'hydraulique' ? 'selected' : ''}>Hydraulique</option>
          <option value="electrique" ${part.category === 'electrique' ? 'selected' : ''}>Électrique</option>
          <option value="capteur" ${part.category === 'capteur' ? 'selected' : ''}>Capteur / Détection</option>
          <option value="mecanique" ${part.category === 'mecanique' ? 'selected' : ''}>Mécanique</option>
          <option value="consommable" ${part.category === 'consommable' ? 'selected' : ''}>Consommable</option>
        </select>
      </div>
      <div class="form-group">
        <label for="epUnit">Unité de gestion *</label>
        <select id="epUnit" name="unit" required>
          <option value="piece" ${part.unit === 'piece' ? 'selected' : ''}>Pièce(s)</option>
          <option value="metre" ${part.unit === 'metre' ? 'selected' : ''}>Mètre(s)</option>
          <option value="litre" ${part.unit === 'litre' ? 'selected' : ''}>Litre(s)</option>
          <option value="kg" ${part.unit === 'kg' ? 'selected' : ''}>Kg</option>
          <option value="lot" ${part.unit === 'lot' ? 'selected' : ''}>Lot(s)</option>
          <option value="boite" ${part.unit === 'boite' ? 'selected' : ''}>Boîte(s)</option>
        </select>
      </div>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label>PMP courant HT</label>
        <input type="text" readonly disabled value="${formatCents(part.unit_price_cents)}" />
        <span class="form-help">Le PMP est actualisé automatiquement lors des réceptions d'entrées.</span>
      </div>
      <div class="form-group">
        <label for="epAlertThreshold">Seuil d'alerte spécifique</label>
        <input type="number" min="0" id="epAlertThreshold" name="alert_threshold" value="${part.alert_threshold !== null && part.alert_threshold !== undefined ? esc(part.alert_threshold) : ''}" placeholder="Seuil global par défaut" />
      </div>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="epSupplier">Fournisseur habituel</label>
        <select id="epSupplier" name="supplier_id">
          <option value="">-- Aucun fournisseur --</option>
          ${suppliers.map(s => `<option value="${esc(s.id)}" ${s.id === part.supplier_id ? 'selected' : ''}>${esc(s.name)}</option>`).join('')}
        </select>
      </div>
      <div class="form-group">
        <label for="epSupplierRef">Réf. fournisseur</label>
        <input type="text" id="epSupplierRef" name="supplier_reference" value="${esc(part.supplier_reference || '')}" />
      </div>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="epManufacturer">Fabricant</label>
        <input type="text" id="epManufacturer" name="manufacturer" value="${esc(part.manufacturer || '')}" />
      </div>
      <div class="form-group">
        <label for="epManufacturerRef">Réf. fabricant</label>
        <input type="text" id="epManufacturerRef" name="manufacturer_reference" value="${esc(part.manufacturer_reference || '')}" />
      </div>
    </div>
    <div class="form-group">
      <label for="epBarcode">Code-barres / EAN</label>
      <input type="text" id="epBarcode" name="barcode" value="${esc(part.barcode || '')}" />
    </div>
    <div class="form-group">
      <label for="epDesc">Description technique</label>
      <textarea id="epDesc" name="description" rows="2">${esc(part.description || '')}</textarea>
    </div>
  `;

  openFormModal({
    title: `Modifier « ${part.reference} »`,
    formId: 'modalEditPartForm',
    submitAction: 'edit-part-submit',
    submitArgs: part.id,
    submitLabel: 'Enregistrer les modifications',
    fieldsHtml,
  });
}

async function handleEditPartSubmit(form, partId) {
  const btn = document.getElementById('modalEditPartFormSubmitBtn');
  const errBox = document.getElementById('modalEditPartFormErrorBox');
  const errText = document.getElementById('modalEditPartFormErrorBoxText');

  const reference = (form.reference.value || '').trim();
  const name = (form.name.value || '').trim();
  const category = form.category.value;
  const unit = form.unit.value;
  const alert_threshold = form.alert_threshold.value !== '' ? parseInt(form.alert_threshold.value, 10) : null;
  const supplier_id = form.supplier_id.value || null;
  const supplier_reference = (form.supplier_reference.value || '').trim() || null;
  const manufacturer = (form.manufacturer.value || '').trim() || null;
  const manufacturer_reference = (form.manufacturer_reference.value || '').trim() || null;
  const barcode = (form.barcode.value || '').trim() || null;
  const description = (form.description.value || '').trim() || null;

  btn.disabled = true;
  errBox.hidden = true;

  try {
    await api(`/api/parts/${encodeURIComponent(partId)}`, {
      method: 'PATCH',
      body: {
        reference,
        name,
        category,
        unit,
        alert_threshold,
        supplier_id,
        supplier_reference,
        manufacturer,
        manufacturer_reference,
        barcode,
        description,
      },
    });

    notify(`Pièce « ${reference} » mise à jour.`, 'success');
    closeModal();
    if (state.currentPartDetail && state.currentPartDetail.id === partId) {
      await loadPartDetail(partId);
    }
    if (state.stockActiveTab === 'catalog') {
      await loadPartsListView(true);
    }
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || 'Impossible de modifier la pièce.';
  }
}

async function handleDeletePart(partId) {
  const part = (state.currentPartDetail && state.currentPartDetail.id === partId)
    ? state.currentPartDetail
    : state.parts.find(p => p.id === partId);
  const ref = part ? part.reference : 'cette pièce';

  if (part && part.total_stock > 0) {
    notify(`Suppression impossible : la pièce possède encore ${part.total_stock} unité(s) en stock. Veuillez d'abord solder ou ajuster le stock.`, 'error');
    return;
  }

  const ok = await confirmDialog(
    'Supprimer la pièce détachée',
    `Voulez-vous vraiment supprimer la pièce « ${ref} » du catalogue ?\nCette opération est irréversible si elle n'a plus de mouvements rattachés.`,
    'Supprimer',
    true
  );
  if (!ok) return;

  try {
    await api(`/api/parts/${encodeURIComponent(partId)}`, {
      method: 'DELETE',
    });
    notify(`Pièce « ${ref} » supprimée.`, 'success');
    if (state.currentPartDetail && state.currentPartDetail.id === partId) {
      location.hash = '#/stock';
    } else {
      await loadPartsListView(true);
    }
  } catch (err) {
    notify(err.message || 'Suppression refusée.', 'error');
  }
}

async function openMovementInModal(prefill = {}) {
  const currentSiteId = state.selectedSiteId || (state.site ? state.site.id : '');
  let partsList = state.parts;
  let locationsList = state.locations;

  try {
    const siteParam = currentSiteId ? `?site_id=${encodeURIComponent(currentSiteId)}&limit=100` : '?limit=100';
    if (!partsList.length) {
      const pRes = await api(`/api/parts${siteParam}`);
      partsList = Array.isArray(pRes) ? pRes : (pRes && Array.isArray(pRes.items) ? pRes.items : []);
    }
    if (!locationsList.length) {
      const lRes = await api(`/api/locations${siteParam}`);
      locationsList = Array.isArray(lRes) ? lRes : (lRes && Array.isArray(lRes.items) ? lRes.items : []);
    }
  } catch (_) {}

  const targetPartId = prefill.partId || '';
  const targetLocId = prefill.locationId || '';

  const selectedPart = partsList.find(p => p.id === targetPartId);
  const defaultPrice = selectedPart ? centsToEuroInput(selectedPart.unit_price_cents) : '';

  const fieldsHtml = `
    <div class="form-group">
      <label for="mInPart">Pièce détachée à réceptionner *</label>
      <select id="mInPart" name="part_id" required data-action="on-movement-in-part-change">
        <option value="" ${!targetPartId ? 'selected disabled' : ''}>-- Choisir une pièce --</option>
        ${partsList.map(p => `
          <option value="${esc(p.id)}" data-pmp="${esc(p.unit_price_cents)}" ${p.id === targetPartId ? 'selected' : ''}>
            ${esc(p.reference)} — ${esc(p.name)} (${formatCents(p.unit_price_cents)} HT)
          </option>
        `).join('')}
      </select>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="mInLoc">Emplacement de stockage de destination *</label>
        <select id="mInLoc" name="to_location_id" required>
          <option value="" ${!targetLocId ? 'selected disabled' : ''}>-- Choisir un emplacement --</option>
          ${locationsList.map(l => `<option value="${esc(l.id)}" ${l.id === targetLocId ? 'selected' : ''}>${esc(l.name)} (${esc(l.code)})</option>`).join('')}
        </select>
      </div>
      <div class="form-group">
        <label for="mInQty">Quantité réceptionnée *</label>
        <input type="number" min="1" step="1" id="mInQty" name="quantity" required placeholder="ex. 5" />
      </div>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="mInPrice">Prix d'achat unitaire HT (€) *</label>
        <input type="number" min="0" step="0.01" id="mInPrice" name="unit_price" required value="${esc(defaultPrice)}" placeholder="ex. 14.50" />
        <span class="form-help">Obligatoire pour l'actualisation du Prix Moyen Pondéré (PMP).</span>
      </div>
      <div class="form-group">
        <label for="mInReason">N° Bon de livraison / Référence commande</label>
        <input type="text" id="mInReason" name="reason" placeholder="ex. BL-2026-981" />
      </div>
    </div>
  `;

  openFormModal({
    title: 'Entrée en stock / Réception fournisseur',
    formId: 'modalMovementInForm',
    submitAction: 'movement-in-submit',
    submitLabel: 'Valider la réception',
    fieldsHtml,
  });
}

async function handleMovementInSubmit(form) {
  const btn = document.getElementById('modalMovementInFormSubmitBtn');
  const errBox = document.getElementById('modalMovementInFormErrorBox');
  const errText = document.getElementById('modalMovementInFormErrorBoxText');

  const part_id = form.part_id.value;
  const to_location_id = form.to_location_id.value;
  const quantity = parseInt(form.quantity.value, 10);
  const unit_price_cents = eurosToCents(form.unit_price.value);
  const reason = (form.reason.value || '').trim() || null;

  btn.disabled = true;
  errBox.hidden = true;

  try {
    if (!part_id) throw new Error('Veuillez sélectionner une pièce.');
    if (!to_location_id) throw new Error('Veuillez sélectionner un emplacement.');
    if (isNaN(quantity) || quantity <= 0) throw new Error('La quantité doit être un nombre entier strictement positif.');
    if (unit_price_cents < 0) throw new Error('Le prix d\'achat unitaire ne peut pas être négatif.');

    await api('/api/stock/movements', {
      method: 'POST',
      body: {
        movement_type: 'in',
        part_id,
        to_location_id,
        quantity,
        unit_price_cents,
        reason,
      },
    });

    notify('Entrée en stock enregistrée avec succès (PMP mis à jour).', 'success');
    closeModal();

    if (state.currentPartDetail && state.currentPartDetail.id === part_id) {
      await loadPartDetail(part_id);
    }
    if (state.stockActiveTab === 'catalog') await loadPartsListView(true);
    else if (state.stockActiveTab === 'stock') await loadStockLevelsView();
    else if (state.stockActiveTab === 'movements') await loadStockMovementsListView(true);
    else if (state.stockActiveTab === 'alerts') await loadStockAlertsView();
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || 'Impossible d\'enregistrer l\'entrée en stock.';
  }
}

async function openMovementTransferModal(prefill = {}) {
  const currentSiteId = state.selectedSiteId || (state.site ? state.site.id : '');
  let partsList = state.parts;
  let locationsList = state.locations;

  try {
    const siteParam = currentSiteId ? `?site_id=${encodeURIComponent(currentSiteId)}&limit=100` : '?limit=100';
    if (!partsList.length) {
      const pRes = await api(`/api/parts${siteParam}`);
      partsList = Array.isArray(pRes) ? pRes : (pRes && Array.isArray(pRes.items) ? pRes.items : []);
    }
    if (!locationsList.length) {
      const lRes = await api(`/api/locations${siteParam}`);
      locationsList = Array.isArray(lRes) ? lRes : (lRes && Array.isArray(lRes.items) ? lRes.items : []);
    }
  } catch (_) {}

  const targetPartId = prefill.partId || '';
  const targetFromLocId = prefill.fromLocationId || '';

  const fieldsHtml = `
    <div class="form-group">
      <label for="mTrPart">Pièce à transférer *</label>
      <select id="mTrPart" name="part_id" required>
        <option value="" ${!targetPartId ? 'selected disabled' : ''}>-- Choisir une pièce --</option>
        ${partsList.map(p => `
          <option value="${esc(p.id)}" ${p.id === targetPartId ? 'selected' : ''}>
            ${esc(p.reference)} — ${esc(p.name)}
          </option>
        `).join('')}
      </select>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="mTrFromLoc">Emplacement d'origine *</label>
        <select id="mTrFromLoc" name="from_location_id" required>
          <option value="" ${!targetFromLocId ? 'selected disabled' : ''}>-- Emplacement de départ --</option>
          ${locationsList.map(l => `<option value="${esc(l.id)}" ${l.id === targetFromLocId ? 'selected' : ''}>${esc(l.name)}</option>`).join('')}
        </select>
      </div>
      <div class="form-group">
        <label for="mTrToLoc">Emplacement de destination *</label>
        <select id="mTrToLoc" name="to_location_id" required>
          <option value="" selected disabled>-- Emplacement d'arrivée --</option>
          ${locationsList.map(l => `<option value="${esc(l.id)}">${esc(l.name)}</option>`).join('')}
        </select>
      </div>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="mTrQty">Quantité à déplacer *</label>
        <input type="number" min="1" step="1" id="mTrQty" name="quantity" required placeholder="ex. 2" />
      </div>
      <div class="form-group">
        <label for="mTrReason">Commentaires / Motif</label>
        <input type="text" id="mTrReason" name="reason" placeholder="ex. Réorganisation magasin" />
      </div>
    </div>
  `;

  openFormModal({
    title: 'Transfert interne d\'emplacement',
    formId: 'modalMovementTransferForm',
    submitAction: 'movement-transfer-submit',
    submitLabel: 'Valider le transfert',
    fieldsHtml,
  });
}

async function handleMovementTransferSubmit(form) {
  const btn = document.getElementById('modalMovementTransferFormSubmitBtn');
  const errBox = document.getElementById('modalMovementTransferFormErrorBox');
  const errText = document.getElementById('modalMovementTransferFormErrorBoxText');

  const part_id = form.part_id.value;
  const from_location_id = form.from_location_id.value;
  const to_location_id = form.to_location_id.value;
  const quantity = parseInt(form.quantity.value, 10);
  const reason = (form.reason.value || '').trim() || null;

  btn.disabled = true;
  errBox.hidden = true;

  try {
    if (!part_id) throw new Error('Veuillez sélectionner une pièce.');
    if (!from_location_id) throw new Error('Veuillez sélectionner l\'emplacement d\'origine.');
    if (!to_location_id) throw new Error('Veuillez sélectionner l\'emplacement de destination.');
    if (from_location_id === to_location_id) throw new Error('L\'emplacement d\'origine et de destination doivent être différents.');
    if (isNaN(quantity) || quantity <= 0) throw new Error('La quantité doit être un entier strictement positif.');

    await api('/api/stock/movements', {
      method: 'POST',
      body: {
        movement_type: 'transfer',
        part_id,
        from_location_id,
        to_location_id,
        quantity,
        reason,
      },
    });

    notify('Transfert de stock effectué avec succès.', 'success');
    closeModal();

    if (state.currentPartDetail && state.currentPartDetail.id === part_id) {
      await loadPartDetail(part_id);
    }
    if (state.stockActiveTab === 'stock') await loadStockLevelsView();
    else if (state.stockActiveTab === 'movements') await loadStockMovementsListView(true);
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || 'Impossible d\'effectuer le transfert.';
  }
}

async function openMovementAdjustModal(prefill = {}) {
  const currentSiteId = state.selectedSiteId || (state.site ? state.site.id : '');
  let partsList = state.parts;
  let locationsList = state.locations;

  try {
    const siteParam = currentSiteId ? `?site_id=${encodeURIComponent(currentSiteId)}&limit=100` : '?limit=100';
    if (!partsList.length) {
      const pRes = await api(`/api/parts${siteParam}`);
      partsList = Array.isArray(pRes) ? pRes : (pRes && Array.isArray(pRes.items) ? pRes.items : []);
    }
    if (!locationsList.length) {
      const lRes = await api(`/api/locations${siteParam}`);
      locationsList = Array.isArray(lRes) ? lRes : (lRes && Array.isArray(lRes.items) ? lRes.items : []);
    }
  } catch (_) {}

  const targetPartId = prefill.partId || '';
  const targetLocId = prefill.locationId || '';

  const fieldsHtml = `
    <div class="form-group">
      <label for="mAdjPart">Pièce à ajuster *</label>
      <select id="mAdjPart" name="part_id" required>
        <option value="" ${!targetPartId ? 'selected disabled' : ''}>-- Choisir une pièce --</option>
        ${partsList.map(p => `
          <option value="${esc(p.id)}" ${p.id === targetPartId ? 'selected' : ''}>
            ${esc(p.reference)} — ${esc(p.name)}
          </option>
        `).join('')}
      </select>
    </div>
    <div class="form-group">
      <label for="mAdjLoc">Emplacement concerné *</label>
      <select id="mAdjLoc" name="location_id" required>
        <option value="" ${!targetLocId ? 'selected disabled' : ''}>-- Choisir l'emplacement --</option>
        ${locationsList.map(l => `<option value="${esc(l.id)}" ${l.id === targetLocId ? 'selected' : ''}>${esc(l.name)}</option>`).join('')}
      </select>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="mAdjKind">Nature de l'ajustement *</label>
        <select id="mAdjKind" name="adjustment_kind" required>
          <option value="inventaire" selected>Écart d'inventaire</option>
          <option value="rebut">Mise au rebut</option>
          <option value="perte">Perte constatée</option>
          <option value="casse">Casse accidentelle</option>
        </select>
      </div>
      <div class="form-group">
        <label for="mAdjDirection">Sens de la correction *</label>
        <select id="mAdjDirection" name="direction" required>
          <option value="sub">Retrait / Diminution (-)</option>
          <option value="add">Ajout / Surplus (+)</option>
        </select>
      </div>
    </div>
    <div class="form-group">
      <label for="mAdjQty">Quantité d'ajustement *</label>
      <input type="number" min="1" step="1" id="mAdjQty" name="quantity" required placeholder="ex. 1" />
      <span class="form-help">Valeur absolue de la variation. Le stock résultant ne peut pas être négatif.</span>
    </div>
    <div class="form-group">
      <label for="mAdjReason">Motif obligatoire de l'ajustement *</label>
      <input type="text" id="mAdjReason" name="reason" required placeholder="ex. Contrôle tournant des stocks, colis détérioré…" />
      <span class="form-help">Obligatoire pour la traçabilité et l'audit.</span>
    </div>
  `;

  openFormModal({
    title: 'Ajustement d\'inventaire / Rebut',
    formId: 'modalMovementAdjustForm',
    submitAction: 'movement-adjust-submit',
    submitLabel: 'Valider l\'ajustement',
    fieldsHtml,
  });
}

async function handleMovementAdjustSubmit(form) {
  const btn = document.getElementById('modalMovementAdjustFormSubmitBtn');
  const errBox = document.getElementById('modalMovementAdjustFormErrorBox');
  const errText = document.getElementById('modalMovementAdjustFormErrorBoxText');

  const part_id = form.part_id.value;
  const location_id = form.location_id.value;
  const adjustment_kind = form.adjustment_kind.value;
  const direction = form.direction.value;
  const qty = parseInt(form.quantity.value, 10);
  const reason = (form.reason.value || '').trim();

  btn.disabled = true;
  errBox.hidden = true;

  try {
    if (!part_id) throw new Error('Veuillez sélectionner une pièce.');
    if (!location_id) throw new Error('Veuillez sélectionner un emplacement.');
    if (isNaN(qty) || qty <= 0) throw new Error('La quantité doit être un nombre strictement positif.');
    if (!reason) throw new Error('Le motif de l\'ajustement est obligatoire.');

    const delta = direction === 'add' ? qty : -qty;

    await api('/api/stock/adjustments', {
      method: 'POST',
      body: {
        part_id,
        location_id,
        delta,
        adjustment_kind,
        reason,
      },
    });

    notify(`Ajustement de stock enregistré (${delta > 0 ? `+${delta}` : delta} unités).`, 'success');
    closeModal();

    if (state.currentPartDetail && state.currentPartDetail.id === part_id) {
      await loadPartDetail(part_id);
    }
    if (state.stockActiveTab === 'stock') await loadStockLevelsView();
    else if (state.stockActiveTab === 'movements') await loadStockMovementsListView(true);
    else if (state.stockActiveTab === 'alerts') await loadStockAlertsView();
    else if (state.stockActiveTab === 'catalog') await loadPartsListView(true);
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || 'Impossible d\'enregistrer l\'ajustement.';
  }
}

async function openAddInterventionPartModal(interventionId) {
  const inter = (state.currentInterventionDetail && state.currentInterventionDetail.id === interventionId)
    ? state.currentInterventionDetail
    : await api(`/api/interventions/${encodeURIComponent(interventionId)}`);

  const currentSiteId = inter.site_id || state.selectedSiteId || (state.site ? state.site.id : '');
  const siteParam = currentSiteId ? `?site_id=${encodeURIComponent(currentSiteId)}&limit=100` : '?limit=100';

  let parts = [];
  let recommendedParts = [];

  try {
    const [pRes, recRes] = await Promise.all([
      api(`/api/parts${siteParam}`),
      inter.equipment_id ? api(`/api/equipments/${encodeURIComponent(inter.equipment_id)}/parts`) : Promise.resolve([])
    ]);
    parts = Array.isArray(pRes) ? pRes : (pRes && Array.isArray(pRes.items) ? pRes.items : []);
    recommendedParts = (Array.isArray(recRes) ? recRes : []).map(adaptStock);
  } catch (_) {}

  let recommendedHtml = '';
  if (recommendedParts.length > 0) {
    recommendedHtml = `
      <div class="recommended-parts-block">
        <div class="hint font-bold">${icon('i-wrench')} Pièces recommandées pour cet équipement :</div>
        <div class="recommended-part-chips">
          ${recommendedParts.map(rp => `
            <button type="button" class="btn sm recommended-part-chip" data-action="quick-select-int-part" data-args="${jsArg(rp.part_id || rp.id)}">
              ${icon('i-basket')} <span>${esc(rp.reference)} — ${esc(rp.name)}</span>
            </button>
          `).join('')}
        </div>
      </div>
    `;
  }

  const fieldsHtml = `
    ${recommendedHtml}
    <div class="form-group">
      <label for="intPartId">Pièce détachée à consommer *</label>
      <select id="intPartId" name="part_id" required data-action="on-int-part-select" data-args="${jsArg(currentSiteId)}">
        <option value="" selected disabled>-- Choisir une pièce --</option>
        ${parts.map(p => `
          <option value="${esc(p.id)}" data-pmp="${esc(p.unit_price_cents)}" data-stock="${esc(p.total_stock)}">
            ${esc(p.reference)} — ${esc(p.name)} (${esc(p.total_stock)} en stock, PMP: ${formatCents(p.unit_price_cents)})
          </option>
        `).join('')}
      </select>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="intPartLoc">Emplacement de prélèvement *</label>
        <select id="intPartLoc" name="location_id" required data-action="on-int-part-loc-select">
          <option value="" selected disabled>-- Sélectionner d'abord une pièce --</option>
        </select>
      </div>
      <div class="form-group">
        <label for="intPartQty">Quantité consommée *</label>
        <input type="number" min="1" step="1" id="intPartQty" name="quantity" required value="1" data-action="on-int-part-qty-input" />
      </div>
    </div>
    <div id="intPartEstimBox" class="solution-report-box hint" hidden>
      ${icon('i-info')}
      <span>Valorisation estimée : <b id="intPartEstimTotal">0,00 €</b> (PMP unitaire : <span id="intPartEstimPmp">0,00 €</span> HT)</span>
    </div>
  `;

  openFormModal({
    title: 'Sortie de pièce sur intervention',
    formId: 'modalAddInterventionPartForm',
    submitAction: 'add-intervention-part-submit',
    submitArgs: interventionId,
    submitLabel: 'Confirmer la sortie',
    fieldsHtml,
  });
}

async function handleAddInterventionPartSubmit(form, interventionId) {
  const btn = document.getElementById('modalAddInterventionPartFormSubmitBtn');
  const errBox = document.getElementById('modalAddInterventionPartFormErrorBox');
  const errText = document.getElementById('modalAddInterventionPartFormErrorBoxText');

  const part_id = form.part_id.value;
  const location_id = form.location_id.value;
  const quantity = parseInt(form.quantity.value, 10);

  btn.disabled = true;
  errBox.hidden = true;

  try {
    if (!part_id) throw new Error('Veuillez sélectionner une pièce détachée.');
    if (!location_id) throw new Error('Veuillez sélectionner un emplacement de prélèvement.');
    if (isNaN(quantity) || quantity <= 0) throw new Error('La quantité doit être un entier supérieur à 0.');

    await api(`/api/interventions/${encodeURIComponent(interventionId)}/parts`, {
      method: 'POST',
      body: {
        part_id,
        location_id,
        quantity,
      },
    });

    notify('Pièce sortie avec succès sur l\'intervention.', 'success');
    closeModal();
    await loadInterventionDetail(interventionId);
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || 'Impossible de sortir la pièce.';
  }
}

async function openAddInterventionReservationModal(interventionId) {
  const inter = (state.currentInterventionDetail && state.currentInterventionDetail.id === interventionId)
    ? state.currentInterventionDetail
    : await api(`/api/interventions/${encodeURIComponent(interventionId)}`);

  const currentSiteId = inter.site_id || state.selectedSiteId || (state.site ? state.site.id : '');
  const siteParam = currentSiteId ? `?site_id=${encodeURIComponent(currentSiteId)}&limit=100` : '?limit=100';

  let parts = [];
  try {
    const pRes = await api(`/api/parts${siteParam}`);
    parts = Array.isArray(pRes) ? pRes : (pRes && Array.isArray(pRes.items) ? pRes.items : []);
  } catch (_) {}

  const fieldsHtml = `
    <div class="form-group">
      <label for="intResPartId">Pièce à préparer / réserver *</label>
      <select id="intResPartId" name="part_id" required data-action="on-int-part-select" data-args="${jsArg(currentSiteId)}">
        <option value="" selected disabled>-- Choisir une pièce --</option>
        ${parts.map(p => `
          <option value="${esc(p.id)}" data-pmp="${esc(p.unit_price_cents)}" data-stock="${esc(p.total_stock)}">
            ${esc(p.reference)} — ${esc(p.name)} (${esc(p.total_stock)} en stock)
          </option>
        `).join('')}
      </select>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="intPartLoc">Emplacement réservé *</label>
        <select id="intPartLoc" name="location_id" required>
          <option value="" selected disabled>-- Sélectionner d'abord une pièce --</option>
        </select>
      </div>
      <div class="form-group">
        <label for="intResQty">Quantité à réserver *</label>
        <input type="number" min="1" step="1" id="intResQty" name="quantity" required value="1" />
        <span class="form-help">Mise de côté pour cette intervention.</span>
      </div>
    </div>
  `;

  openFormModal({
    title: 'Préparer / Réserver une pièce',
    formId: 'modalAddInterventionReservationForm',
    submitAction: 'add-intervention-reservation-submit',
    submitArgs: interventionId,
    submitLabel: 'Valider la réservation',
    fieldsHtml,
  });
}

async function handleAddInterventionReservationSubmit(form, interventionId) {
  const btn = document.getElementById('modalAddInterventionReservationFormSubmitBtn');
  const errBox = document.getElementById('modalAddInterventionReservationFormErrorBox');
  const errText = document.getElementById('modalAddInterventionReservationFormErrorBoxText');

  const part_id = form.part_id.value;
  const location_id = form.location_id.value;
  const quantity = parseInt(form.quantity.value, 10);

  btn.disabled = true;
  errBox.hidden = true;

  try {
    if (!part_id) throw new Error('Veuillez sélectionner une pièce détachée.');
    if (!location_id) throw new Error('Veuillez sélectionner un emplacement.');
    if (isNaN(quantity) || quantity <= 0) throw new Error('La quantité doit être un entier supérieur à 0.');

    await api(`/api/interventions/${encodeURIComponent(interventionId)}/reservations`, {
      method: 'POST',
      body: {
        part_id,
        location_id,
        quantity,
      },
    });

    notify('Pièce réservée et mise de côté avec succès.', 'success');
    closeModal();
    await loadInterventionDetail(interventionId);
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || 'Impossible de réserver la pièce.';
  }
}

function openConsumeReservationModal(args) {
  const fieldsHtml = `
    <p>Vous vous apprêtez à consommer la pièce <b>${esc(args.partName)}</b> préparée pour cette intervention.</p>
    <div class="form-group">
      <label for="cResQty">Quantité consommée *</label>
      <input type="number" min="1" max="${esc(args.maxQuantity)}" step="1" id="cResQty" name="quantity" required value="${esc(args.maxQuantity)}" />
      <span class="form-help">Maximum préparé : ${esc(args.maxQuantity)} unité(s). Si vous consommez une quantité inférieure, le solde non utilisé sera automatiquement remis en rayon.</span>
    </div>
  `;

  openFormModal({
    title: 'Consommer une pièce préparée',
    formId: 'modalConsumeReservationForm',
    submitAction: 'consume-reservation-submit',
    submitArgs: args,
    submitLabel: 'Consommer sur l\'intervention',
    fieldsHtml,
  });
}

async function handleConsumeReservationSubmit(form, args) {
  const btn = document.getElementById('modalConsumeReservationFormSubmitBtn');
  const errBox = document.getElementById('modalConsumeReservationFormErrorBox');
  const errText = document.getElementById('modalConsumeReservationFormErrorBoxText');

  const quantity = parseInt(form.quantity.value, 10);
  btn.disabled = true;
  errBox.hidden = true;

  try {
    if (isNaN(quantity) || quantity <= 0) throw new Error('La quantité doit être un entier supérieur à 0.');
    if (quantity > args.maxQuantity) throw new Error(`La quantité ne peut pas excéder la quantité préparée (${args.maxQuantity}).`);

    await api(`/api/reservations/${encodeURIComponent(args.reservationId)}/consume`, {
      method: 'POST',
      body: { quantity },
    });

    notify('Pièce(s) consommée(s) sur l\'intervention avec succès.', 'success');
    closeModal();
    if (args.interventionId) {
      await loadInterventionDetail(args.interventionId);
    }
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || 'Impossible de consommer la réservation.';
  }
}

async function handleReleaseReservation(args) {
  const ok = await confirmDialog(
    'Remettre en rayon',
    'Voulez-vous libérer cette réservation ? La pièce préparée ne sera pas consommée et sera réintégrée dans le stock disponible de son emplacement.',
    'Remettre en rayon',
    false
  );
  if (!ok) return;

  try {
    await api(`/api/reservations/${encodeURIComponent(args.reservationId)}/release`, {
      method: 'POST',
    });
    notify('Pièce remise en rayon avec succès.', 'success');
    if (args.interventionId) {
      await loadInterventionDetail(args.interventionId);
    }
  } catch (err) {
    notify(err.message || 'Impossible de libérer la réservation.', 'error');
  }
}

async function handleDeleteInterventionPart(args) {
  const ok = await confirmDialog(
    'Annuler la sortie de pièce',
    'Êtes-vous sûr de vouloir annuler cette consommation ? La quantité sortie sera réintégrée dans le stock physique de son emplacement d\'origine.',
    'Annuler la sortie',
    true
  );
  if (!ok) return;

  try {
    await api(`/api/interventions/${encodeURIComponent(args.interventionId)}/parts/${encodeURIComponent(args.itemId)}`, {
      method: 'DELETE',
    });
    notify('Sortie de pièce annulée et stock réintégré.', 'success');
    await loadInterventionDetail(args.interventionId);
  } catch (err) {
    notify(err.message || 'Impossible d\'annuler la sortie de pièce.', 'error');
  }
}

async function openAddPartEquipmentModal(partId) {
  const part = state.currentPartDetail || state.parts.find(p => p.id === partId);
  const currentSiteId = part ? part.site_id : (state.selectedSiteId || (state.site ? state.site.id : ''));

  let equipments = [];
  try {
    const siteParam = currentSiteId ? `?site_id=${encodeURIComponent(currentSiteId)}&limit=100` : '?limit=100';
    const eqRes = await api(`/api/equipments${siteParam}`);
    equipments = Array.isArray(eqRes) ? eqRes : (eqRes && Array.isArray(eqRes.items) ? eqRes.items : []);
  } catch (_) {}

  const fieldsHtml = `
    <div class="form-group">
      <label for="peEquip">Équipement à associer *</label>
      <select id="peEquip" name="equipment_id" required>
        <option value="" selected disabled>-- Choisir un équipement --</option>
        ${equipments.map(e => `<option value="${esc(e.id)}">${esc(e.code)} — ${esc(e.name)}</option>`).join('')}
      </select>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="peQty">Quantité préconisée</label>
        <input type="number" min="1" step="1" id="peQty" name="quantity_recommended" value="1" />
        <span class="form-help">Nombre d'exemplaires installés sur cette machine.</span>
      </div>
      <div class="form-group">
        <label for="peNotes">Notes d'assemblage / Repère plan</label>
        <input type="text" id="peNotes" name="assembly_notes" placeholder="ex. Palier avant arbre primaire" />
      </div>
    </div>
  `;

  openFormModal({
    title: 'Associer à un équipement',
    formId: 'modalPartEquipmentForm',
    submitAction: 'add-part-equipment-submit',
    submitArgs: partId,
    submitLabel: 'Enregistrer l\'association',
    fieldsHtml,
  });
}

async function handleAddPartEquipmentSubmit(form, partId) {
  const btn = document.getElementById('modalPartEquipmentFormSubmitBtn');
  const errBox = document.getElementById('modalPartEquipmentFormErrorBox');
  const errText = document.getElementById('modalPartEquipmentFormErrorBoxText');

  const equipment_id = form.equipment_id.value;
  const quantity_recommended = parseInt(form.quantity_recommended.value, 10) || 1;
  const notes = (form.assembly_notes.value || '').trim() || null;

  btn.disabled = true;
  errBox.hidden = true;

  try {
    if (!equipment_id) throw new Error('Veuillez sélectionner un équipement.');

    await api(`/api/equipments/${encodeURIComponent(equipment_id)}/parts`, {
      method: 'POST',
      body: {
        part_id: partId,
        quantity_recommended,
        notes,
      },
    });

    notify('Équipement associé à la pièce avec succès.', 'success');
    closeModal();
    if (state.currentPartDetail && state.currentPartDetail.id === partId) {
      await loadPartDetail(partId);
    }
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || 'Impossible d\'associer l\'équipement.';
  }
}

async function handleDeletePartEquipment(partId, equipId) {
  const ok = await confirmDialog(
    'Dissocier l\'équipement',
    'Voulez-vous retirer cette pièce des composants préconisés de l\'équipement ?',
    'Dissocier',
    true
  );
  if (!ok) return;

  try {
    await api(`/api/equipments/${encodeURIComponent(equipId)}/parts/${encodeURIComponent(partId)}`, {
      method: 'DELETE',
    });
    notify('Équipement dissocié.', 'success');
    if (state.currentPartDetail && state.currentPartDetail.id === partId) {
      await loadPartDetail(partId);
    }
  } catch (err) {
    notify(err.message || 'Dissociation refusée.', 'error');
  }
}

async function openAddEquipmentPartModal(equipmentId) {
  const currentSiteId = state.selectedSiteId || (state.site ? state.site.id : '');
  let parts = [];
  try {
    const siteParam = currentSiteId ? `?site_id=${encodeURIComponent(currentSiteId)}&limit=100` : '?limit=100';
    const pRes = await api(`/api/parts${siteParam}`);
    parts = Array.isArray(pRes) ? pRes : (pRes && Array.isArray(pRes.items) ? pRes.items : []);
  } catch (_) {}

  const fieldsHtml = `
    <div class="form-group">
      <label for="epPartId">Pièce détachée à associer *</label>
      <select id="epPartId" name="part_id" required>
        <option value="" selected disabled>-- Choisir une pièce --</option>
        ${parts.map(p => `<option value="${esc(p.id)}">${esc(p.reference)} — ${esc(p.name)}</option>`).join('')}
      </select>
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="epQtyRec">Quantité préconisée</label>
        <input type="number" min="1" step="1" id="epQtyRec" name="quantity_recommended" value="1" />
      </div>
      <div class="form-group">
        <label for="epNotes">Notes d'assemblage / Repère</label>
        <input type="text" id="epNotes" name="notes" placeholder="ex. Roulement d'arbre de sortie" />
      </div>
    </div>
  `;

  openFormModal({
    title: 'Associer une pièce détachée',
    formId: 'modalEquipmentPartForm',
    submitAction: 'add-equipment-part-submit',
    submitArgs: equipmentId,
    submitLabel: 'Associer la pièce',
    fieldsHtml,
  });
}

async function handleAddEquipmentPartSubmit(form, equipmentId) {
  const btn = document.getElementById('modalEquipmentPartFormSubmitBtn');
  const errBox = document.getElementById('modalEquipmentPartFormErrorBox');
  const errText = document.getElementById('modalEquipmentPartFormErrorBoxText');

  const part_id = form.part_id.value;
  const quantity_recommended = parseInt(form.quantity_recommended.value, 10) || 1;
  const notes = (form.notes.value || '').trim() || null;

  btn.disabled = true;
  errBox.hidden = true;

  try {
    if (!part_id) throw new Error('Veuillez sélectionner une pièce détachée.');

    await api(`/api/equipments/${encodeURIComponent(equipmentId)}/parts`, {
      method: 'POST',
      body: {
        part_id,
        quantity_recommended,
        notes,
      },
    });

    notify('Pièce détachée associée à l\'équipement.', 'success');
    closeModal();
    openEquipmentDetailModal(equipmentId);
  } catch (err) {
    btn.disabled = false;
    errBox.hidden = false;
    errText.textContent = err.message || 'Impossible d\'associer la pièce.';
  }
}

async function handleDeleteEquipmentPart(args) {
  const ok = await confirmDialog(
    'Dissocier la pièce',
    'Voulez-vous retirer cette pièce des composants préconisés de cet équipement ?',
    'Dissocier',
    true
  );
  if (!ok) return;

  try {
    await api(`/api/equipments/${encodeURIComponent(args.equipmentId)}/parts/${encodeURIComponent(args.partId)}`, {
      method: 'DELETE',
    });
    notify('Pièce détachée dissociée de l\'équipement.', 'success');
    openEquipmentDetailModal(args.equipmentId);
  } catch (err) {
    notify(err.message || 'Impossible de dissocier la pièce.', 'error');
  }
}

async function updateInterventionPartLocations(partId, siteId) {
  const locSelect = document.getElementById('intPartLoc');
  if (!locSelect || !partId) return;

  locSelect.innerHTML = `<option value="" disabled selected>Chargement des emplacements…</option>`;

  try {
    const siteParam = siteId ? `&site_id=${encodeURIComponent(siteId)}` : '';
    const res = await api(`/api/stock?part_id=${encodeURIComponent(partId)}${siteParam}`);
    const items = Array.isArray(res) ? res : (res && Array.isArray(res.items) ? res.items : []);

    if (!items.length) {
      locSelect.innerHTML = `<option value="" disabled selected>Aucun stock disponible pour cette pièce</option>`;
      return;
    }

    locSelect.innerHTML = `<option value="" disabled selected>-- Choisir l'emplacement --</option>` +
      items.map(s => `
        <option value="${esc(s.location_id)}" data-available="${esc(s.available)}">
          ${esc(s.location ? s.location.name : 'Emplacement')} — Dispo : ${esc(s.available)} (Total : ${esc(s.quantity)})
        </option>
      `).join('');

    const availableItems = items.filter(s => s.available > 0);
    if (availableItems.length === 1) {
      locSelect.value = availableItems[0].location_id;
    }

    updateInterventionPartEstim();
  } catch (err) {
    locSelect.innerHTML = `<option value="" disabled selected>Erreur : ${esc(err.message)}</option>`;
  }
}

function updateInterventionPartEstim() {
  const partSelect = document.getElementById('intPartId');
  const qtyInput = document.getElementById('intPartQty');
  const estimBox = document.getElementById('intPartEstimBox');
  const totalEl = document.getElementById('intPartEstimTotal');
  const pmpEl = document.getElementById('intPartEstimPmp');

  if (!partSelect || !qtyInput || !estimBox) return;

  const opt = partSelect.selectedOptions[0];
  if (!opt || !opt.value) {
    estimBox.hidden = true;
    return;
  }

  const pmpCents = parseInt(opt.dataset.pmp, 10) || 0;
  const qty = parseInt(qtyInput.value, 10) || 0;

  if (pmpEl) pmpEl.textContent = formatCents(pmpCents);
  if (totalEl) totalEl.textContent = formatCents(pmpCents * qty);
  estimBox.hidden = false;
}

// ============================================================================
// 14b. MODULE MAINTENANCE PRÉVENTIVE ET COMPTEURS (PHASE 5)
// ============================================================================

/** Page principale du module Maintenance Préventive (Plannings / Vue planning). */
function maintenancePage() {
  const isPlans = state.maintenanceTab === 'plans';

  const subNavHtml = `
    <div class="sub-nav-tabs" role="tablist" aria-label="Sous-navigation maintenance préventive">
      <button type="button" class="sub-nav-tab ${isPlans ? 'active' : ''}" data-action="switch-maintenance-tab" data-args="${jsArg('plans')}">
        ${icon('i-cal')} <span>Plannings</span>
      </button>
      <button type="button" class="sub-nav-tab ${!isPlans ? 'active' : ''}" data-action="switch-maintenance-tab" data-args="${jsArg('schedule')}">
        ${icon('i-clock')} <span>Vue planning</span>
      </button>
    </div>
  `;

  return `
    <div class="page-head">
      ${icon('i-cal')}
      <h1>Maintenance préventive</h1>
    </div>
    ${subNavHtml}
    <div id="maintenanceTabContainer">
      <p class="hint">${icon('i-clock')} Chargement du module…</p>
    </div>
  `;
}

async function initMaintenancePageView() {
  if (!state.equipments.length) {
    try {
      const siteParam = state.selectedSiteId ? `?site_id=${encodeURIComponent(state.selectedSiteId)}&limit=100` : '?limit=100';
      const eRes = await api(`/api/equipments${siteParam}`);
      state.equipments = eRes && Array.isArray(eRes.items) ? eRes.items : [];
    } catch (_) {}
  }

  if (state.maintenanceTab === 'plans') {
    renderMaintenancePlansView();
    await loadMaintenancePlans(true);
  } else {
    renderMaintenanceScheduleView();
    await loadMaintenanceSchedule();
  }
}

/** Vue « Plannings » : filtre, tableau paginé et actions. */
function renderMaintenancePlansView() {
  const container = document.getElementById('maintenanceTabContainer');
  if (!container) return;

  const canManage = hasPermission('maintenance.manage');

  const extraFilters = `
    <select data-action="filter-maintenance-plans-frequency" aria-label="Filtrer par fréquence">
      <option value="">Toutes les fréquences</option>
      <option value="daily" ${state.maintenancePlansFrequencyFilter === 'daily' ? 'selected' : ''}>Journalière</option>
      <option value="weekly" ${state.maintenancePlansFrequencyFilter === 'weekly' ? 'selected' : ''}>Hebdomadaire</option>
      <option value="monthly" ${state.maintenancePlansFrequencyFilter === 'monthly' ? 'selected' : ''}>Mensuelle</option>
      <option value="quarterly" ${state.maintenancePlansFrequencyFilter === 'quarterly' ? 'selected' : ''}>Trimestrielle</option>
      <option value="yearly" ${state.maintenancePlansFrequencyFilter === 'yearly' ? 'selected' : ''}>Annuelle</option>
      <option value="interval_days" ${state.maintenancePlansFrequencyFilter === 'interval_days' ? 'selected' : ''}>Intervalle (jours)</option>
      <option value="counter" ${state.maintenancePlansFrequencyFilter === 'counter' ? 'selected' : ''}>Compteur</option>
    </select>
    <select data-action="filter-maintenance-plans-due" aria-label="Filtrer par échéance">
      <option value="all" ${state.maintenancePlansDueFilter === 'all' ? 'selected' : ''}>Toutes les échéances</option>
      <option value="overdue" ${state.maintenancePlansDueFilter === 'overdue' ? 'selected' : ''}>En retard</option>
      <option value="soon" ${state.maintenancePlansDueFilter === 'soon' ? 'selected' : ''}>Sous 30 jours</option>
    </select>
    <select data-action="filter-maintenance-plans-equipment" aria-label="Filtrer par équipement">
      <option value="">Tous les équipements</option>
      ${state.equipments.map(e => `<option value="${esc(e.id)}" ${state.maintenancePlansEquipmentFilter === e.id ? 'selected' : ''}>${esc(e.code)} - ${esc(e.name)}</option>`).join('')}
    </select>
  `;

  container.innerHTML = `
    ${renderToolbar({
      searchAction: 'filter-maintenance-plans-search',
      searchPlaceholder: 'Rechercher un planning (code, titre)…',
      searchValue: state.maintenancePlansSearchQuery,
      extraFiltersHtml: extraFilters,
      canCreate: canManage,
      secondaryActionsHtml: renderExportDropdown('maintenance_plans'),
      primaryAction: 'open-create-maintenance-plan',
      primaryLabel: 'Nouveau planning',
      primaryIcon: 'i-plus',
    })}
    <div id="maintenancePlansTableContainer">
      <p class="hint">${icon('i-clock')} Chargement des plannings…</p>
    </div>
  `;
}

async function loadMaintenancePlans(reset = true) {
  const container = document.getElementById('maintenancePlansTableContainer');
  if (!container) return;

  if (reset) {
    state.maintenancePlans = [];
    state.maintenancePlansNextCursor = null;
    container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement des plannings…</p>`;
  }

  state.maintenancePlansLoading = true;

  try {
    const params = new URLSearchParams();
    params.set('limit', '25');
    if (state.selectedSiteId) params.set('site_id', state.selectedSiteId);
    if (state.maintenancePlansSearchQuery) params.set('q', state.maintenancePlansSearchQuery.trim());
    if (state.maintenancePlansEquipmentFilter) params.set('equipment_id', state.maintenancePlansEquipmentFilter);
    if (state.maintenancePlansFrequencyFilter) params.set('frequency', state.maintenancePlansFrequencyFilter);
    if (state.maintenancePlansDueFilter && state.maintenancePlansDueFilter !== 'all') {
      params.set('due', state.maintenancePlansDueFilter);
    }
    if (state.maintenancePlansActiveFilter !== '') {
      params.set('active', state.maintenancePlansActiveFilter);
    }
    if (!reset && state.maintenancePlansNextCursor) {
      params.set('cursor', state.maintenancePlansNextCursor);
    }

    const res = await api(`/api/maintenance/plans?${params.toString()}`);
    const items = res && Array.isArray(res.items) ? res.items : [];
    state.maintenancePlansNextCursor = res ? res.next_cursor : null;

    if (reset) {
      state.maintenancePlans = items;
    } else {
      state.maintenancePlans = state.maintenancePlans.concat(items);
    }

    renderMaintenancePlansTable();
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur lors du chargement des plannings : ${esc(err.message)}</p>`;
  } finally {
    state.maintenancePlansLoading = false;
  }
}

function renderMaintenancePlansTable() {
  const container = document.getElementById('maintenancePlansTableContainer');
  if (!container) return;

  const canManage = hasPermission('maintenance.manage');
  const plans = state.maintenancePlans;

  if (!plans.length) {
    container.innerHTML = `<p class="hint">Aucun planning de maintenance trouvé.</p>`;
    return;
  }

  const rowsHtml = plans.map(p => {
    const hasOpenOcc = Boolean(p.open_occurrence);
    const equipLabel = p.equipment ? `${p.equipment.code} — ${p.equipment.name}` : '—';

    let openInterHtml = '—';
    if (hasOpenOcc && p.open_occurrence.intervention_id) {
      openInterHtml = `
        <a href="#/interventions/${esc(p.open_occurrence.intervention_id)}" class="link-action mono" title="Voir l'intervention">
          <b>${esc(p.open_occurrence.intervention_code || 'Voir fiche')}</b>
        </a>
      `;
    }

    const actions = [];
    actions.push(`
      <a href="#/maintenance-preventive/${esc(p.id)}" class="btn sm" title="Consulter la fiche du planning">
        ${icon('i-info')} <span>Consulter</span>
      </a>
    `);

    if (canManage) {
      if (!hasOpenOcc) {
        actions.push(`
          <button type="button" class="btn sm primary" data-action="generate-plan-intervention" data-args="${jsArg(p.id)}" title="Générer l'intervention maintenant">
            ${icon('i-wrench')} <span>Générer</span>
          </button>
          <button type="button" class="btn sm" data-action="open-skip-plan-modal" data-args="${jsArg(p.id)}" title="Reporter l'échéance avec motif">
            ${icon('i-clock')} <span>Reporter</span>
          </button>
        `);
      }
      actions.push(`
        <button type="button" class="btn sm" data-action="toggle-plan-active" data-args="${jsArg(p.id)}" title="${p.active ? 'Suspendre le planning' : 'Activer le planning'}">
          ${icon(p.active ? 'i-ban' : 'i-check')}
        </button>
      `);
    }

    return `
      <tr>
        <td>
          <a href="#/maintenance-preventive/${esc(p.id)}" class="link-action mono">
            <b>${esc(p.code)}</b>
          </a>
        </td>
        <td>
          <a href="#/maintenance-preventive/${esc(p.id)}" class="link-action">
            <b>${esc(p.title)}</b>
          </a>
        </td>
        <td>
          ${p.equipment ? `
            <button type="button" class="link-action" data-action="open-detail-equipment" data-args="${jsArg(p.equipment.id)}">
              ${esc(equipLabel)}
            </button>
          ` : '—'}
        </td>
        <td><span class="badge badge-freq">${esc(planFrequencyLabel(p))}</span></td>
        <td><b>${esc(formatPlanDue(p))}</b></td>
        <td>${planStateBadge(p.state)}</td>
        <td>${openInterHtml}</td>
        <td><div class="cell-actions">${actions.join('')}</div></td>
      </tr>
    `;
  }).join('');

  container.innerHTML = `
    <div class="table-container">
      <table class="data-table">
        <thead>
          <tr>
            <th>Code</th>
            <th>Titre</th>
            <th>Équipement</th>
            <th>Fréquence</th>
            <th>Prochaine échéance</th>
            <th>État</th>
            <th>Intervention ouverte</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          ${rowsHtml}
        </tbody>
      </table>
    </div>
    ${renderLoadMore(state.maintenancePlansNextCursor, 'load-more-maintenance-plans', state.maintenancePlansLoading)}
  `;
}

/** Vue « Planning » : calendrier mensuel et listes 30/60/90 jours. */
function renderMaintenanceScheduleView() {
  const container = document.getElementById('maintenanceTabContainer');
  if (!container) return;

  const range = state.maintenanceScheduleRange;
  const isMonth = range === 'month';

  const rangeButtonsHtml = `
    <div class="filter-tabs" role="tablist" aria-label="Plage de vue planning">
      <button type="button" class="filter-tab ${isMonth ? 'active' : ''}" data-action="switch-schedule-range" data-args="${jsArg('month')}">
        ${icon('i-cal')} <span>Vue mensuelle</span>
      </button>
      <button type="button" class="filter-tab ${range === '30' ? 'active' : ''}" data-action="switch-schedule-range" data-args="${jsArg('30')}">
        <span>30 jours</span>
      </button>
      <button type="button" class="filter-tab ${range === '60' ? 'active' : ''}" data-action="switch-schedule-range" data-args="${jsArg('60')}">
        <span>60 jours</span>
      </button>
      <button type="button" class="filter-tab ${range === '90' ? 'active' : ''}" data-action="switch-schedule-range" data-args="${jsArg('90')}">
        <span>90 jours</span>
      </button>
    </div>
  `;

  let navControlsHtml = '';
  if (isMonth) {
    const [yStr, mStr] = state.maintenanceScheduleMonth.split('-');
    const dt = new Date(parseInt(yStr, 10), parseInt(mStr, 10) - 1, 1);
    const monthName = dt.toLocaleDateString('fr-FR', { month: 'long', year: 'numeric' });
    const capitalizedMonth = monthName.charAt(0).toUpperCase() + monthName.slice(1);

    navControlsHtml = `
      <div class="schedule-nav-group">
        <button type="button" class="btn sm" data-action="schedule-prev-month" title="Mois précédent">
          ${icon('i-arrow-left')} <span>Précédent</span>
        </button>
        <span class="month-title-display">${esc(capitalizedMonth)}</span>
        <button type="button" class="btn sm" data-action="schedule-next-month" title="Mois suivant">
          <span>Suivant</span> ${icon('i-chevron-right')}
        </button>
        <button type="button" class="btn sm" data-action="schedule-today" title="Revenir au mois courant">
          <span>Aujourd'hui</span>
        </button>
      </div>
    `;
  }

  const equipFilterHtml = `
    <div class="schedule-nav-group">
      <select data-action="filter-schedule-equipment" aria-label="Filtrer par équipement">
        <option value="">Tous les équipements</option>
        ${state.equipments.map(e => `<option value="${esc(e.id)}" ${state.maintenancePlansEquipmentFilter === e.id ? 'selected' : ''}>${esc(e.code)} - ${esc(e.name)}</option>`).join('')}
      </select>
    </div>
  `;

  container.innerHTML = `
    ${rangeButtonsHtml}
    <div class="schedule-nav-bar">
      ${navControlsHtml}
      ${equipFilterHtml}
    </div>
    <div id="scheduleContentContainer">
      <p class="hint">${icon('i-clock')} Chargement de la vue planning…</p>
    </div>
  `;
}

async function loadMaintenanceSchedule() {
  const container = document.getElementById('scheduleContentContainer');
  if (!container) return;

  state.maintenanceScheduleLoading = true;
  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement des échéances…</p>`;

  const range = state.maintenanceScheduleRange;
  let fromStr = '';
  let toStr = '';

  if (range === 'month') {
    const [yStr, mStr] = state.maintenanceScheduleMonth.split('-');
    const y = parseInt(yStr, 10);
    const m = parseInt(mStr, 10);
    fromStr = `${yStr}-${mStr}-01`;
    const lastDay = new Date(y, m, 0).getDate();
    toStr = `${yStr}-${mStr}-${String(lastDay).padStart(2, '0')}`;
  } else {
    const numDays = parseInt(range, 10) || 30;
    const now = new Date();
    fromStr = localIso(now).slice(0, 10);
    const dTo = new Date();
    dTo.setDate(dTo.getDate() + numDays);
    toStr = localIso(dTo).slice(0, 10);
  }

  try {
    const params = new URLSearchParams();
    params.set('from', fromStr);
    params.set('to', toStr);
    if (state.selectedSiteId) params.set('site_id', state.selectedSiteId);
    if (state.maintenancePlansEquipmentFilter) params.set('equipment_id', state.maintenancePlansEquipmentFilter);

    const data = await api(`/api/maintenance/schedule?${params.toString()}`);
    state.maintenanceScheduleEvents = Array.isArray(data) ? data : [];

    if (range === 'month') {
      renderCalendarMonthlyView();
    } else {
      renderScheduleUpcomingListView();
    }
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur lors du chargement de la vue planning : ${esc(err.message)}</p>`;
  } finally {
    state.maintenanceScheduleLoading = false;
  }
}

/** Rendu du calendrier mensuel (grille responsive + agenda mobile). */
function renderCalendarMonthlyView() {
  const container = document.getElementById('scheduleContentContainer');
  if (!container) return;

  const [yStr, mStr] = state.maintenanceScheduleMonth.split('-');
  const year = parseInt(yStr, 10);
  const month = parseInt(mStr, 10); // 1-12

  const todayStr = localIso().slice(0, 10);
  const daysInMonth = new Date(year, month, 0).getDate();
  const firstDayOfWeek = new Date(year, month - 1, 1).getDay(); // 0 = Dim, 1 = Lun
  const offset = (firstDayOfWeek + 6) % 7; // 0 = Lun, 6 = Dim

  const eventsByDate = {};
  const counterEvents = [];

  for (const evt of state.maintenanceScheduleEvents) {
    if (evt.date) {
      if (!eventsByDate[evt.date]) eventsByDate[evt.date] = [];
      eventsByDate[evt.date].push(evt);
    } else {
      counterEvents.push(evt);
    }
  }

  // Construction de la grille mensuelle (semaines)
  const daysOfWeekLabels = ['Lun', 'Mar', 'Mer', 'Jeu', 'Ven', 'Sam', 'Dim'];
  let cellsHtml = '';
  let dayCount = 1;
  const totalSlots = Math.ceil((offset + daysInMonth) / 7) * 7;

  // Jours du mois précédent pour combler l'offset
  const prevMonthLastDay = new Date(year, month - 1, 0).getDate();

  for (let slot = 0; slot < totalSlots; slot++) {
    if (slot % 7 === 0) cellsHtml += '<tr>';

    if (slot < offset) {
      const prevDay = prevMonthLastDay - (offset - slot - 1);
      cellsHtml += `<td class="calendar-cell outside"><span class="calendar-day-num">${prevDay}</span></td>`;
    } else if (dayCount <= daysInMonth) {
      const curDateStr = `${year}-${String(month).padStart(2, '0')}-${String(dayCount).padStart(2, '0')}`;
      const isToday = curDateStr === todayStr;
      const dayEvts = eventsByDate[curDateStr] || [];

      const evtsHtml = dayEvts.map(evt => {
        const link = evt.intervention_id
          ? `#/interventions/${esc(evt.intervention_id)}`
          : `#/maintenance-preventive/${esc(evt.plan.id)}`;
        return `
          <a href="${link}" class="calendar-event-item calendar-event-${esc(evt.status)}" title="${esc(evt.plan.code)} — ${esc(evt.plan.title)} (${esc(occurrenceStatusLabel(evt.status))})">
            <b>${esc(evt.plan.code)}</b> ${esc(evt.plan.title)}
          </a>
        `;
      }).join('');

      cellsHtml += `
        <td class="calendar-cell ${isToday ? 'today' : ''}">
          <div class="calendar-cell-top">
            <span class="calendar-day-num">${dayCount}</span>
          </div>
          <div class="calendar-events-box">${evtsHtml}</div>
        </td>
      `;
      dayCount++;
    } else {
      const nextDay = slot - (offset + daysInMonth) + 1;
      cellsHtml += `<td class="calendar-cell outside"><span class="calendar-day-num">${nextDay}</span></td>`;
    }

    if (slot % 7 === 6) cellsHtml += '</tr>';
  }

  // Agenda pour affichage mobile (< 680px)
  let agendaRowsHtml = '';
  for (let d = 1; d <= daysInMonth; d++) {
    const curDateStr = `${year}-${String(month).padStart(2, '0')}-${String(d).padStart(2, '0')}`;
    const dayEvts = eventsByDate[curDateStr] || [];
    const isToday = curDateStr === todayStr;

    if (dayEvts.length || isToday) {
      const dt = new Date(year, month - 1, d);
      const dateLabel = dt.toLocaleDateString('fr-FR', { weekday: 'short', day: 'numeric', month: 'short' });

      const evtsListHtml = dayEvts.length ? dayEvts.map(evt => {
        const link = evt.intervention_id
          ? `#/interventions/${esc(evt.intervention_id)}`
          : `#/maintenance-preventive/${esc(evt.plan.id)}`;
        return `
          <div class="agenda-event-row">
            <div>
              <a href="${link}" class="link-action mono"><b>${esc(evt.plan.code)}</b></a>
              <span>${esc(evt.plan.title)}</span>
              ${evt.equipment ? `<div class="hint">${esc(evt.equipment.code)} — ${esc(evt.equipment.name)}</div>` : ''}
            </div>
            ${occurrenceStatusBadge(evt.status)}
          </div>
        `;
      }).join('') : '<p class="hint">Aucune échéance prévue ce jour.</p>';

      agendaRowsHtml += `
        <div class="agenda-day-card ${isToday ? 'today' : ''}">
          <div class="agenda-day-header">
            <span>${esc(dateLabel)}</span>
            ${isToday ? '<span class="badge badge-state-ok">Aujourd\'hui</span>' : ''}
          </div>
          <div class="agenda-events-list">
            ${evtsListHtml}
          </div>
        </div>
      `;
    }
  }

  // Section des plannings de compteur
  const counterSectionHtml = renderCounterPlansSummary(counterEvents);

  container.innerHTML = `
    <!-- Vue tableau écran large -->
    <div class="calendar-wrap">
      <table class="calendar-table">
        <thead>
          <tr>
            ${daysOfWeekLabels.map(l => `<th>${l}</th>`).join('')}
          </tr>
        </thead>
        <tbody>
          ${cellsHtml}
        </tbody>
      </table>
    </div>

    <!-- Vue agenda adaptée aux écrans étroits et mobiles (375 px) -->
    <div class="calendar-mobile-agenda">
      ${agendaRowsHtml || '<p class="hint">Aucune échéance prévue pour ce mois.</p>'}
    </div>

    ${counterSectionHtml}
  `;
}

/** Rendu de la vue liste des échéances 30 / 60 / 90 jours avec retards en tête. */
function renderScheduleUpcomingListView() {
  const container = document.getElementById('scheduleContentContainer');
  if (!container) return;

  const todayStr = localIso().slice(0, 10);
  const events = state.maintenanceScheduleEvents;

  const overdueEvents = [];
  const upcomingEvents = [];
  const counterEvents = [];

  for (const evt of events) {
    if (!evt.date) {
      counterEvents.push(evt);
    } else {
      const isOverdue = evt.status === 'overdue' || (evt.date < todayStr && evt.status !== 'done' && evt.status !== 'skipped');
      if (isOverdue) {
        overdueEvents.push(evt);
      } else {
        upcomingEvents.push(evt);
      }
    }
  }

  // Tri chronologique des échéances à venir
  upcomingEvents.sort((a, b) => (a.date > b.date ? 1 : -1));
  overdueEvents.sort((a, b) => (a.date > b.date ? 1 : -1));

  // Bloc des retards
  let overdueHtml = '';
  if (overdueEvents.length) {
    overdueHtml = `
      <section class="schedule-overdue-box">
        <div class="schedule-overdue-title">
          ${icon('i-alert')}
          <span>Échéances en retard (${overdueEvents.length})</span>
        </div>
        <div class="schedule-items-list">
          ${overdueEvents.map(evt => renderScheduleItemCard(evt, true)).join('')}
        </div>
      </section>
    `;
  }

  // Liste des échéances à venir
  let upcomingHtml = '';
  if (upcomingEvents.length) {
    upcomingHtml = `
      <section class="card">
        <h2>${icon('i-cal')} Prochaines échéances planifiées (${upcomingEvents.length})</h2>
        <div class="schedule-items-list">
          ${upcomingEvents.map(evt => renderScheduleItemCard(evt, false)).join('')}
        </div>
      </section>
    `;
  } else {
    upcomingHtml = `
      <section class="card">
        <h2>${icon('i-cal')} Prochaines échéances planifiées</h2>
        <p class="hint">Aucune échéance calendaire sur la période sélectionnée.</p>
      </section>
    `;
  }

  // Plannings de compteur
  const counterSectionHtml = renderCounterPlansSummary(counterEvents);

  container.innerHTML = `
    <div class="schedule-list-container">
      ${overdueHtml}
      ${upcomingHtml}
      ${counterSectionHtml}
    </div>
  `;
}

function renderScheduleItemCard(evt, isOverdue) {
  const dt = new Date(`${evt.date}T00:00:00`);
  const dayNum = dt.getDate();
  const monthName = dt.toLocaleDateString('fr-FR', { month: 'short' });

  const link = evt.intervention_id
    ? `#/interventions/${esc(evt.intervention_id)}`
    : `#/maintenance-preventive/${esc(evt.plan.id)}`;

  return `
    <div class="schedule-item-card ${isOverdue ? 'is-overdue' : ''}">
      <div class="schedule-item-left">
        <div class="schedule-date-badge">
          <span class="day-num">${dayNum}</span>
          <span class="month-name">${esc(monthName)}</span>
        </div>
        <div class="schedule-item-info">
          <div class="schedule-item-title-row">
            <a href="${link}" class="link-action mono"><b>${esc(evt.plan.code)}</b></a>
            <a href="${link}" class="link-action"><b>${esc(evt.plan.title)}</b></a>
          </div>
          ${evt.equipment ? `
            <div class="schedule-item-equip">
              ${icon('i-gear')} <b>${esc(evt.equipment.code)}</b> — ${esc(evt.equipment.name)}
            </div>
          ` : ''}
        </div>
      </div>
      <div class="schedule-item-actions">
        ${occurrenceStatusBadge(evt.status)}
        <a href="${link}" class="btn sm">
          ${icon('i-info')} <span>${evt.intervention_id ? 'Intervention' : 'Planning'}</span>
        </a>
      </div>
    </div>
  `;
}

function renderCounterPlansSummary(counterEvents) {
  if (!counterEvents || !counterEvents.length) return '';

  return `
    <section class="schedule-counters-box detail-card-spaced">
      <div class="schedule-counters-title">
        ${icon('i-clock')}
        <span>Plannings asservis aux compteurs (${counterEvents.length})</span>
      </div>
      <div class="schedule-counters-grid">
        ${counterEvents.map(evt => {
          const rem = evt.remaining;
          let stateClass = 'ok';
          if (rem !== null && rem <= 0) stateClass = 'overdue';
          else if (evt.status === 'due_soon') stateClass = 'soon';

          return `
            <div class="counter-plan-card">
              <div class="counter-plan-header">
                <div>
                  <a href="#/maintenance-preventive/${esc(evt.plan.id)}" class="link-action mono"><b>${esc(evt.plan.code)}</b></a>
                  <div class="bold">${esc(evt.plan.title)}</div>
                </div>
                ${planStateBadge(evt.status)}
              </div>
              ${evt.equipment ? `<div class="hint">${esc(evt.equipment.code)} — ${esc(evt.equipment.name)}</div>` : ''}
              <div class="counter-plan-remaining ${stateClass}">
                ${rem !== null ? (rem <= 0 ? 'Seuil d\'échéance atteint' : `Reste ${rem} ${esc(evt.unit || '')}`) : 'Compteur non initialisé'}
              </div>
              <div>
                <a href="#/maintenance-preventive/${esc(evt.plan.id)}" class="btn sm">
                  ${icon('i-info')} <span>Consulter la fiche</span>
                </a>
              </div>
            </div>
          `;
        }).join('')}
      </div>
    </section>
  `;
}

/** Fiche détaillée d'un planning de maintenance préventive. */
function maintenancePlanDetailPage(id) {
  return `
    <div class="plan-detail">
      <div class="detail-nav-bar">
        <a href="#/maintenance-preventive" class="btn" title="Retour aux plannings">
          ${icon('i-arrow-left')} <span>Retour aux plannings</span>
        </a>
      </div>
      <div id="planDetailContainer">
        <p class="hint">${icon('i-clock')} Chargement du planning…</p>
      </div>
    </div>
  `;
}

async function initMaintenancePlanDetailPageView(id) {
  await loadMaintenancePlanDetail(id);
}

async function loadMaintenancePlanDetail(id) {
  const container = document.getElementById('planDetailContainer');
  if (!container) return;

  state.currentPlanLoading = true;
  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement du planning…</p>`;

  try {
    const [plan, occsRes] = await Promise.all([
      api(`/api/maintenance/plans/${encodeURIComponent(id)}`),
      api(`/api/maintenance/plans/${encodeURIComponent(id)}/occurrences?limit=25`).catch(() => ({ items: [] })),
    ]);

    state.currentPlanDetail = plan;
    state.currentPlanOccurrences = occsRes && Array.isArray(occsRes.items) ? occsRes.items : [];
    state.currentPlanOccurrencesNextCursor = occsRes ? occsRes.next_cursor : null;

    renderPlanDetailView(plan);
    loadAndRenderEntityDocuments('maintenance_plan', plan.id, 'planDocumentsContainer');
  } catch (err) {
    container.innerHTML = `
      <div class="card">
        <p class="hint state-ko">${icon('i-alert')} Erreur lors du chargement du planning : ${esc(err.message)}</p>
        <p><a href="#/maintenance-preventive" class="btn">${icon('i-arrow-left')} <span>Retour à la liste</span></a></p>
      </div>
    `;
  } finally {
    state.currentPlanLoading = false;
  }
}

function renderPlanDetailView(plan) {
  const container = document.getElementById('planDetailContainer');
  if (!container) return;

  const canManage = hasPermission('maintenance.manage');
  const hasOpenOcc = Boolean(plan.open_occurrence);

  const quickActions = [];
  if (canManage) {
    if (!hasOpenOcc) {
      quickActions.push(`
        <button type="button" class="btn primary" data-action="generate-plan-intervention" data-args="${jsArg(plan.id)}">
          ${icon('i-wrench')} <span>Générer l'intervention</span>
        </button>
        <button type="button" class="btn" data-action="open-skip-plan-modal" data-args="${jsArg(plan.id)}">
          ${icon('i-clock')} <span>Reporter l'échéance</span>
        </button>
      `);
    }
    quickActions.push(`
      <button type="button" class="btn" data-action="open-edit-maintenance-plan" data-args="${jsArg(plan.id)}">
        ${icon('i-edit')} <span>Modifier</span>
      </button>
      <button type="button" class="btn" data-action="toggle-plan-active" data-args="${jsArg(plan.id)}">
        ${icon(plan.active ? 'i-ban' : 'i-check')} <span>${plan.active ? 'Désactiver' : 'Activer'}</span>
      </button>
      <button type="button" class="btn danger" data-action="delete-maintenance-plan" data-args="${jsArg(plan.id)}">
        ${icon('i-trash')} <span>Supprimer</span>
      </button>
    `);
  }

  let bannerOpenHtml = '';
  if (hasOpenOcc) {
    bannerOpenHtml = `
      <div class="plan-banner-open">
        ${icon('i-wrench')}
        <div>
          <b>Une intervention issue de ce planning est en cours :</b>
          <a href="#/interventions/${esc(plan.open_occurrence.intervention_id)}" class="link-action">
            <b>${esc(plan.open_occurrence.intervention_code || 'Voir l\'intervention')}</b>
          </a>
          ${plan.open_occurrence.due_date ? ` (Échéance : <b>${esc(plan.open_occurrence.due_date)}</b>)` : ''}
          ${plan.open_occurrence.due_counter !== null && plan.open_occurrence.due_counter !== undefined ? ` (Seuil compteur : <b>${esc(plan.open_occurrence.due_counter)}</b>)` : ''}
        </div>
      </div>
    `;
  }

  let recurrenceDetailsHtml = '';
  if (plan.frequency === 'counter') {
    const cnt = plan.counter;
    const currentVal = cnt ? cnt.current_value : 0;
    const unit = cnt ? cnt.unit : '';
    const diff = (plan.next_due_counter || 0) - currentVal;
    recurrenceDetailsHtml = `
      <dt>Type de fréquence</dt><dd><span class="badge badge-freq">Compteur machine</span></dd>
      <dt>Compteur associé</dt><dd>${cnt ? `<b>${esc(cnt.name)}</b> (valeur actuelle : ${esc(currentVal)} ${esc(unit)})` : '—'}</dd>
      <dt>Intervalle compteur</dt><dd>Tous les <b>${esc(plan.counter_interval)} ${esc(unit)}</b></dd>
      <dt>Prochain seuil</dt><dd><b class="mono">${esc(plan.next_due_counter)} ${esc(unit)}</b> (${diff <= 0 ? '<span class="state-ko">Seuil franchi</span>' : `reste ${diff} ${esc(unit)}`})</dd>
      <dt>Avance de génération</dt><dd>${esc(plan.lead_counter)} ${esc(unit)}</dd>
    `;
  } else {
    recurrenceDetailsHtml = `
      <dt>Type de fréquence</dt><dd><span class="badge badge-freq">${esc(formatPlanFrequency(plan.frequency))}</span></dd>
      ${plan.frequency === 'interval_days' ? `<dt>Intervalle en jours</dt><dd>Tous les <b>${esc(plan.interval_days)} jours</b></dd>` : ''}
      <dt>Date d'ancrage initiale</dt><dd>${plan.anchor_date ? formatDate(plan.anchor_date) : '—'}</dd>
      <dt>Prochaine date d'échéance</dt><dd><b class="mono">${plan.next_due_date ? formatDate(plan.next_due_date) : '—'}</b></dd>
      <dt>Avance de génération</dt><dd>${esc(plan.lead_days)} jours avant échéance</dd>
    `;
  }

  // Occurrences passées
  const occs = state.currentPlanOccurrences;
  let occRowsHtml = occs.map(occ => {
    let interCell = '—';
    if (!occ.intervention && occ.intervention_id) occ.intervention = { id: occ.intervention_id, code: occ.intervention_code };
    if (occ.intervention) {
      interCell = `
        <a href="#/interventions/${esc(occ.intervention.id)}" class="link-action mono">
          <b>${esc(occ.intervention.code)}</b>
        </a>
      `;
    }
    const dueDisplay = occ.due_date ? formatDate(occ.due_date) : (occ.due_counter !== null && occ.due_counter !== undefined ? `Seuil : ${occ.due_counter}` : '—');
    return `
      <tr>
        <td><b>${dueDisplay}</b></td>
        <td>${occurrenceStatusBadge(occ.status)}</td>
        <td><span class="badge">${occ.generation === 'auto' ? 'Automatique' : 'Manuel'}</span></td>
        <td>${interCell}</td>
        <td>${occ.closed_at ? formatDate(occ.closed_at) : (occ.generated_at ? `Générée le ${formatDate(occ.generated_at)}` : '—')}</td>
        <td>${occ.skip_reason ? `<em>« ${esc(occ.skip_reason)} »</em>` : '—'}</td>
      </tr>
    `;
  }).join('');

  if (!occs.length) {
    occRowsHtml = `<tr><td colspan="6" class="empty-cell">Aucun historique d'occurrence enregistré pour le moment.</td></tr>`;
  }

  container.innerHTML = `
    <!-- CARTE D'EN-TÊTE -->
    <section class="detail-header-card">
      <div class="detail-header-top">
        <div class="detail-title-block">
          <div class="detail-code-row">
            <span class="detail-code-label mono">${esc(plan.code)}</span>
            <span class="detail-title-label">${esc(plan.title)}</span>
          </div>
          <div class="detail-badges-row">
            ${planStateBadge(plan.state)}
            <span class="badge ${plan.active ? 'badge-active-on' : 'badge-active-off'}">${plan.active ? 'Actif' : 'Inactif'}</span>
            ${interventionPriorityBadge(plan.priority)}
          </div>
        </div>
      </div>
      ${bannerOpenHtml}
      ${quickActions.length ? `<div class="quick-actions-bar">${quickActions.join('')}</div>` : ''}
    </section>

    <!-- GRILLE D'INFORMATIONS -->
    <div class="detail-grid">
      <!-- PARAMÈTRES GÉNÉRAUX -->
      <section class="card">
        <h2>${icon('i-info')} Paramètres généraux</h2>
        <dl class="detail-dl">
          <dt>Équipement associé</dt>
          <dd>
            ${plan.equipment ? `
              <button type="button" class="link-action" data-action="open-detail-equipment" data-args="${jsArg(plan.equipment.id)}">
                <b>${esc(plan.equipment.code)} — ${esc(plan.equipment.name)}</b>
              </button>
            ` : '—'}
          </dd>
          <dt>Priorité</dt><dd>${interventionPriorityBadge(plan.priority)}</dd>
          <dt>Technicien par défaut</dt><dd>${plan.default_technician ? `<b>${esc(plan.default_technician.display_name)}</b>` : '<span class="hint">Non assigné</span>'}</dd>
          <dt>Durée estimée</dt><dd><b class="mono">${formatDurationMinutes(plan.estimated_minutes)}</b></dd>
          <dt>Génération automatique</dt><dd>${plan.auto_generate ? '<span class="state-ok">Activée (moteur automatique)</span>' : '<span class="hint">Désactivée (génération manuelle)</span>'}</dd>
          <dt>Description</dt><dd>${esc(plan.description || 'Aucune description.')}</dd>
        </dl>
      </section>

      <!-- RÈGLE DE RÉCURRENCE ET ÉCHÉANCE -->
      <section class="card">
        <h2>${icon('i-cal')} Règle de récurrence & Prochaine échéance</h2>
        <dl class="detail-dl">
          ${recurrenceDetailsHtml}
        </dl>
      </section>
    </div>

    <!-- GAMME OPÉRATOIRE / INSTRUCTIONS -->
    <section class="card">
      <h2>${icon('i-doc')} Gamme opératoire & Consignes techniques</h2>
      ${plan.instructions ? `
        <div class="plan-instructions-box">${esc(plan.instructions)}</div>
      ` : `
        <p class="hint">Aucune consigne opératoire renseignée pour ce planning.</p>
      `}
    </section>

    <!-- HISTORIQUE DES OCCURRENCES -->
    <section class="card">
      <div class="detail-nav-bar">
        <h2>${icon('i-clock')} Historique des échéances traitées</h2>
      </div>
      <div class="table-container">
        <table class="data-table">
          <thead>
            <tr>
              <th>Échéance</th>
              <th>Statut</th>
              <th>Génération</th>
              <th>Intervention</th>
              <th>Date de traitement</th>
              <th>Motif de report</th>
            </tr>
          </thead>
          <tbody>
            ${occRowsHtml}
          </tbody>
        </table>
      </div>
      ${renderLoadMore(state.currentPlanOccurrencesNextCursor, 'load-more-plan-occurrences', state.currentPlanOccurrencesLoading)}
    </section>

    <!-- SECTION DOCUMENTS (PHASE 6) -->
    <div id="planDocumentsContainer"></div>
  `;
}

async function loadMorePlanOccurrences(planId) {
  const targetId = (state.currentPlanDetail && state.currentPlanDetail.id) || (typeof planId === 'string' && !planId.startsWith('cursor_') ? planId : null);
  if (!targetId || !state.currentPlanOccurrencesNextCursor || state.currentPlanOccurrencesLoading) return;
  state.currentPlanOccurrencesLoading = true;

  try {
    const res = await api(`/api/maintenance/plans/${encodeURIComponent(targetId)}/occurrences?limit=25&cursor=${encodeURIComponent(state.currentPlanOccurrencesNextCursor)}`);
    const newItems = res && Array.isArray(res.items) ? res.items : [];
    state.currentPlanOccurrencesNextCursor = res ? res.next_cursor : null;
    state.currentPlanOccurrences = state.currentPlanOccurrences.concat(newItems);
    renderPlanDetailView(state.currentPlanDetail);
  } catch (err) {
    notify(err.message || 'Erreur chargement occurrences.', 'error');
  } finally {
    state.currentPlanOccurrencesLoading = false;
  }
}

// --- Modales et formulaires pour les plannings ---

async function openCreateMaintenancePlanModal(preselected = {}) {
  const currentSiteId = state.selectedSiteId || (state.site ? state.site.id : '');

  let equips = state.equipments;
  if (!equips.length) {
    try {
      const siteParam = currentSiteId ? `?site_id=${encodeURIComponent(currentSiteId)}&limit=100` : '?limit=100';
      const eRes = await api(`/api/equipments${siteParam}`);
      equips = eRes && Array.isArray(eRes.items) ? eRes.items : [];
    } catch (_) {}
  }

  let techs = [];
  try {
    const siteParam = currentSiteId ? `?site_id=${encodeURIComponent(currentSiteId)}` : '';
    const uRes = await api(`/api/users${siteParam}`);
    const uList = Array.isArray(uRes) ? uRes : (uRes && Array.isArray(uRes.items) ? uRes.items : []);
    techs = uList.filter(u => u.active && ['technicien', 'responsable', 'admin'].includes(u.role));
  } catch (_) {}

  const targetEquipId = preselected.equipment_id || '';

  // Compteurs du premier équipement
  let counters = [];
  if (targetEquipId) {
    try {
      const cRes = await api(`/api/equipments/${encodeURIComponent(targetEquipId)}/counters`);
      counters = (Array.isArray(cRes) ? cRes : (cRes && Array.isArray(cRes.items) ? cRes.items : [])).filter(c => c.active);
    } catch (_) {}
  }

  const todayStr = localIso().slice(0, 10);

  const fieldsHtml = `
    <div class="form-row">
      <div class="form-group">
        <label for="fPlanCode">Code du planning *</label>
        <input type="text" id="fPlanCode" name="code" required placeholder="Ex. PLN-PRS-01" class="mono" />
      </div>
      <div class="form-group">
        <label for="fPlanPriority">Priorité *</label>
        <select id="fPlanPriority" name="priority" required>
          <option value="low">Basse</option>
          <option value="medium" selected>Moyenne</option>
          <option value="high">Haute</option>
          <option value="critical">Critique</option>
        </select>
      </div>
    </div>

    <div class="form-group">
      <label for="fPlanTitle">Titre de l'opération *</label>
      <input type="text" id="fPlanTitle" name="title" required placeholder="Ex. Graissage mensuel des paliers" />
    </div>

    <div class="form-group">
      <label for="fPlanEquipment">Équipement rattaché *</label>
      <select id="fPlanEquipment" name="equipment_id" required data-action="on-plan-form-equipment-change">
        <option value="">-- Sélectionner un équipement --</option>
        ${equips.map(e => `<option value="${esc(e.id)}" ${e.id === targetEquipId ? 'selected' : ''}>${esc(e.code)} — ${esc(e.name)}</option>`).join('')}
      </select>
    </div>

    <div class="form-group">
      <label for="fPlanFrequency">Fréquence de récurrence *</label>
      <select id="fPlanFrequency" name="frequency" required data-action="on-plan-form-frequency-change">
        <option value="daily">Journalière</option>
        <option value="weekly">Hebdomadaire</option>
        <option value="monthly" selected>Mensuelle</option>
        <option value="quarterly">Trimestrielle</option>
        <option value="yearly">Annuelle</option>
        <option value="interval_days">Intervalle en jours (calendrier fixe)</option>
        <option value="counter">Asservie à un compteur machine</option>
      </select>
    </div>

    <!-- Champs conditionnels Intervalle en jours -->
    <div id="planIntervalDaysGroup" class="form-group" hidden>
      <label for="fPlanIntervalDays">Intervalle en jours (1 à 3650) *</label>
      <input type="number" id="fPlanIntervalDays" name="interval_days" min="1" max="3650" placeholder="Ex. 45" />
    </div>

    <!-- Champs conditionnels Compteur -->
    <div id="planCounterGroup" class="form-grid" hidden>
      <div class="form-group">
        <label for="fPlanCounterSelect">Compteur de l'équipement *</label>
        <select id="fPlanCounterSelect" name="counter_id">
          <option value="">-- Choisir un compteur actif --</option>
          ${counters.map(c => `<option value="${esc(c.id)}">${esc(c.name)} (${esc(c.current_value)} ${esc(c.unit)})</option>`).join('')}
        </select>
      </div>
      <div class="form-row">
        <div class="form-group">
          <label for="fPlanCounterInterval">Pas de récurrence compteur *</label>
          <input type="number" id="fPlanCounterInterval" name="counter_interval" min="1" placeholder="Ex. 500" />
        </div>
        <div class="form-group">
          <label for="fPlanLeadCounter">Avance de génération (unités)</label>
          <input type="number" id="fPlanLeadCounter" name="lead_counter" min="0" value="0" />
        </div>
      </div>
    </div>

    <!-- Champs calendaires standard -->
    <div id="planCalendarGroup" class="form-row">
      <div class="form-group">
        <label for="fPlanAnchorDate">Date d'ancrage (première échéance) *</label>
        <input type="date" id="fPlanAnchorDate" name="anchor_date" value="${todayStr}" />
      </div>
      <div class="form-group">
        <label for="fPlanLeadDays">Avance de génération (jours)</label>
        <input type="number" id="fPlanLeadDays" name="lead_days" min="0" max="365" value="7" />
      </div>
    </div>

    <div class="form-row">
      <div class="form-group">
        <label for="fPlanTech">Technicien par défaut</label>
        <select id="fPlanTech" name="default_technician_id">
          <option value="">Aucun technicien pré-assigné</option>
          ${techs.map(t => `<option value="${esc(t.id)}">${esc(t.display_name)} (${esc(formatRole(t.role))})</option>`).join('')}
        </select>
      </div>
      <div class="form-group">
        <label for="fPlanDuration">Durée estimée (minutes)</label>
        <input type="number" id="fPlanDuration" name="estimated_minutes" min="1" placeholder="Ex. 60" />
      </div>
    </div>

    <div class="form-group">
      <label class="checkbox-label">
        <input type="checkbox" name="auto_generate" value="1" checked />
        <span>Génération automatique des interventions par la tâche de fond</span>
      </label>
    </div>

    <div class="form-group">
      <label for="fPlanInstructions">Gamme opératoire (instructions techniques réutilisées dans l'intervention)</label>
      <textarea id="fPlanInstructions" name="instructions" rows="4" placeholder="Étapes à suivre, consignes de sécurité, outillage requis…"></textarea>
    </div>

    <div class="form-group">
      <label for="fPlanDescription">Description générale</label>
      <textarea id="fPlanDescription" name="description" rows="2" placeholder="Objectif de maintenance, périodicité…"></textarea>
    </div>
  `;

  openFormModal({
    title: 'Nouveau planning de maintenance préventive',
    formId: 'modalCreatePlanForm',
    submitAction: 'create-maintenance-plan-submit',
    submitLabel: 'Créer le planning',
    fieldsHtml,
  });
}

async function handleCreateMaintenancePlanSubmit(form) {
  const errBox = document.getElementById('modalCreatePlanFormErrorBox');
  const errText = document.getElementById('modalCreatePlanFormErrorBoxText');
  const btn = document.getElementById('modalCreatePlanFormSubmitBtn');

  const code = (form.code.value || '').trim();
  const title = (form.title.value || '').trim();
  const equipmentId = form.equipment_id.value;
  const frequency = form.frequency.value;
  const priority = form.priority.value;

  if (!code || !title || !equipmentId || !frequency) {
    errText.textContent = 'Veuillez renseigner tous les champs obligatoires (*).';
    errBox.hidden = false;
    return;
  }

  const payload = {
    code,
    title,
    equipment_id: equipmentId,
    frequency,
    priority,
    auto_generate: form.auto_generate ? form.auto_generate.checked : true,
    description: (form.description.value || '').trim() || null,
    instructions: (form.instructions.value || '').trim() || null,
  };

  const durVal = (form.estimated_minutes.value || '').trim();
  if (durVal) payload.estimated_minutes = parseInt(durVal, 10);

  const techVal = form.default_technician_id ? form.default_technician_id.value : '';
  if (techVal) payload.default_technician_id = techVal;

  if (frequency === 'interval_days') {
    const iv = parseInt(form.interval_days.value, 10);
    if (isNaN(iv) || iv < 1 || iv > 3650) {
      errText.textContent = 'L\'intervalle en jours doit être compris entre 1 et 3650.';
      errBox.hidden = false;
      return;
    }
    payload.interval_days = iv;
    payload.anchor_date = form.anchor_date.value || null;
    payload.lead_days = parseInt(form.lead_days.value, 10) || 7;
  } else if (frequency === 'counter') {
    const cid = form.counter_id ? form.counter_id.value : '';
    if (!cid) {
      errText.textContent = 'Veuillez sélectionner un compteur actif de l\'équipement.';
      errBox.hidden = false;
      return;
    }
    const civ = parseInt(form.counter_interval.value, 10);
    if (isNaN(civ) || civ <= 0) {
      errText.textContent = 'Le pas de compteur doit être strictement positif.';
      errBox.hidden = false;
      return;
    }
    payload.counter_id = cid;
    payload.counter_interval = civ;
    payload.lead_counter = parseInt(form.lead_counter.value, 10) || 0;
  } else {
    payload.anchor_date = form.anchor_date.value || null;
    payload.lead_days = parseInt(form.lead_days.value, 10) || 7;
  }

  btn.disabled = true;
  errBox.hidden = true;

  try {
    const newPlan = await api('/api/maintenance/plans', {
      method: 'POST',
      body: payload,
    });
    closeModal();
    notify(`Planning « ${newPlan.code} » créé avec succès.`, 'success');
    if (currentRoute() === 'maintenance-preventive') {
      await loadMaintenancePlans(true);
    } else if (payload.equipment_id && state.currentEquipmentDetailId === payload.equipment_id) {
      openEquipmentDetailModal(payload.equipment_id);
    }
  } catch (err) {
    errText.textContent = err.message || 'Erreur lors de la création du planning.';
    errBox.hidden = false;
    btn.disabled = false;
  }
}

async function openEditMaintenancePlanModal(planId) {
  let plan = state.currentPlanDetail;
  if (!plan || plan.id !== planId) {
    try {
      plan = await api(`/api/maintenance/plans/${encodeURIComponent(planId)}`);
    } catch (err) {
      notify(err.message || 'Planning introuvable.', 'error');
      return;
    }
  }

  const hasOpenOcc = Boolean(plan.open_occurrence);

  let techs = [];
  try {
    const siteParam = plan.site_id ? `?site_id=${encodeURIComponent(plan.site_id)}` : '';
    const uRes = await api(`/api/users${siteParam}`);
    const uList = Array.isArray(uRes) ? uRes : (uRes && Array.isArray(uRes.items) ? uRes.items : []);
    techs = uList.filter(u => u.active && ['technicien', 'responsable', 'admin'].includes(u.role));
  } catch (_) {}

  const fieldsHtml = `
    ${hasOpenOcc ? `
      <div class="machine-state-alert">
        ${icon('i-alert')}
        <span>Une intervention est en cours : la fréquence et l'ancrage ne peuvent pas être modifiés.</span>
      </div>
    ` : ''}

    <div class="form-row">
      <div class="form-group">
        <label for="fEditPlanCode">Code du planning *</label>
        <input type="text" id="fEditPlanCode" name="code" required value="${esc(plan.code)}" class="mono" />
      </div>
      <div class="form-group">
        <label for="fEditPlanPriority">Priorité *</label>
        <select id="fEditPlanPriority" name="priority" required>
          <option value="low" ${plan.priority === 'low' ? 'selected' : ''}>Basse</option>
          <option value="medium" ${plan.priority === 'medium' ? 'selected' : ''}>Moyenne</option>
          <option value="high" ${plan.priority === 'high' ? 'selected' : ''}>Haute</option>
          <option value="critical" ${plan.priority === 'critical' ? 'selected' : ''}>Critique</option>
        </select>
      </div>
    </div>

    <div class="form-group">
      <label for="fEditPlanTitle">Titre de l'opération *</label>
      <input type="text" id="fEditPlanTitle" name="title" required value="${esc(plan.title)}" />
    </div>

    <div class="form-row">
      <div class="form-group">
        <label for="fEditPlanTech">Technicien par défaut</label>
        <select id="fEditPlanTech" name="default_technician_id">
          <option value="">Aucun technicien pré-assigné</option>
          ${techs.map(t => `<option value="${esc(t.id)}" ${t.id === plan.default_technician_id ? 'selected' : ''}>${esc(t.display_name)} (${esc(formatRole(t.role))})</option>`).join('')}
        </select>
      </div>
      <div class="form-group">
        <label for="fEditPlanDuration">Durée estimée (minutes)</label>
        <input type="number" id="fEditPlanDuration" name="estimated_minutes" min="1" value="${plan.estimated_minutes || ''}" placeholder="Ex. 60" />
      </div>
    </div>

    <div class="form-row">
      <div class="form-group">
        <label for="fEditPlanLeadDays">Avance calendaire (jours)</label>
        <input type="number" id="fEditPlanLeadDays" name="lead_days" min="0" max="365" value="${plan.lead_days !== undefined ? plan.lead_days : 7}" />
      </div>
      <div class="form-group">
        <label for="fEditPlanLeadCounter">Avance compteur (unités)</label>
        <input type="number" id="fEditPlanLeadCounter" name="lead_counter" min="0" value="${plan.lead_counter || 0}" />
      </div>
    </div>

    <div class="form-group">
      <label class="checkbox-label">
        <input type="checkbox" name="auto_generate" value="1" ${plan.auto_generate ? 'checked' : ''} />
        <span>Génération automatique des interventions par la tâche de fond</span>
      </label>
    </div>

    <div class="form-group">
      <label for="fEditPlanInstructions">Gamme opératoire (instructions techniques)</label>
      <textarea id="fEditPlanInstructions" name="instructions" rows="4">${esc(plan.instructions || '')}</textarea>
    </div>

    <div class="form-group">
      <label for="fEditPlanDescription">Description générale</label>
      <textarea id="fEditPlanDescription" name="description" rows="2">${esc(plan.description || '')}</textarea>
    </div>
  `;

  openFormModal({
    title: `Modifier le planning « ${plan.code} »`,
    formId: 'modalEditPlanForm',
    submitAction: 'edit-maintenance-plan-submit',
    submitArgs: plan.id,
    submitLabel: 'Enregistrer les modifications',
    fieldsHtml,
  });
}

async function handleEditMaintenancePlanSubmit(form) {
  const planId = form.dataset.args ? JSON.parse(form.dataset.args) : null;
  if (!planId) return;

  const errBox = document.getElementById('modalEditPlanFormErrorBox');
  const errText = document.getElementById('modalEditPlanFormErrorBoxText');
  const btn = document.getElementById('modalEditPlanFormSubmitBtn');

  const code = (form.code.value || '').trim();
  const title = (form.title.value || '').trim();

  if (!code || !title) {
    errText.textContent = 'Le code et le titre ne peuvent pas être vides.';
    errBox.hidden = false;
    return;
  }

  const payload = {
    code,
    title,
    priority: form.priority.value,
    auto_generate: form.auto_generate ? form.auto_generate.checked : true,
    description: (form.description.value || '').trim() || null,
    instructions: (form.instructions.value || '').trim() || null,
    lead_days: parseInt(form.lead_days.value, 10) || 0,
    lead_counter: parseInt(form.lead_counter.value, 10) || 0,
  };

  const durVal = (form.estimated_minutes.value || '').trim();
  payload.estimated_minutes = durVal ? parseInt(durVal, 10) : null;

  const techVal = form.default_technician_id ? form.default_technician_id.value : '';
  payload.default_technician_id = techVal || null;

  btn.disabled = true;
  errBox.hidden = true;

  try {
    const updated = await api(`/api/maintenance/plans/${encodeURIComponent(planId)}`, {
      method: 'PATCH',
      body: payload,
    });
    closeModal();
    notify('Planning mis à jour.', 'success');
    if (state.currentPlanDetail && state.currentPlanDetail.id === planId) {
      await loadMaintenancePlanDetail(planId);
    } else {
      await loadMaintenancePlans(true);
    }
  } catch (err) {
    errText.textContent = err.message || 'Erreur lors de la modification du planning.';
    errBox.hidden = false;
    btn.disabled = false;
  }
}

function openSkipPlanModal(planId) {
  const fieldsHtml = `
    <p class="hint">
      Le report d'échéance avance la prochaine date ou le prochain seuil de compteur selon la fréquence fixée, sans créer d'intervention.
    </p>
    <div class="form-group">
      <label for="fSkipReason">Motif du report *</label>
      <textarea id="fSkipReason" name="reason" rows="3" required placeholder="Justifiez le report de cette échéance de maintenance…"></textarea>
    </div>
  `;

  openFormModal({
    title: 'Reporter l\'échéance du planning',
    formId: 'modalSkipPlanForm',
    submitAction: 'skip-plan-submit',
    submitArgs: planId,
    submitLabel: 'Confirmer le report',
    fieldsHtml,
  });
}

async function handleSkipPlanSubmit(form) {
  const planId = form.dataset.args ? JSON.parse(form.dataset.args) : null;
  if (!planId) return;

  const errBox = document.getElementById('modalSkipPlanFormErrorBox');
  const errText = document.getElementById('modalSkipPlanFormErrorBoxText');
  const btn = document.getElementById('modalSkipPlanFormSubmitBtn');

  const reason = (form.reason.value || '').trim();
  if (!reason) {
    errText.textContent = 'Le motif de report est obligatoire.';
    errBox.hidden = false;
    return;
  }

  btn.disabled = true;
  errBox.hidden = true;

  try {
    const updated = await api(`/api/maintenance/plans/${encodeURIComponent(planId)}/skip`, {
      method: 'POST',
      body: { reason },
    });
    closeModal();
    notify('Échéance reportée avec succès.', 'success');
    if (state.currentPlanDetail && state.currentPlanDetail.id === planId) {
      await loadMaintenancePlanDetail(planId);
    } else {
      await loadMaintenancePlans(true);
    }
  } catch (err) {
    errText.textContent = err.message || 'Erreur lors du report de l\'échéance.';
    errBox.hidden = false;
    btn.disabled = false;
  }
}

async function generatePlanIntervention(planId) {
  const ok = await confirmDialog(
    'Générer l\'intervention préventive',
    'Voulez-vous générer immédiatement l\'intervention préventive sur l\'équipement associé ?',
    'Générer',
    false
  );
  if (!ok) return;

  try {
    const res = await api(`/api/maintenance/plans/${encodeURIComponent(planId)}/generate`, {
      method: 'POST',
    });
    notify(`Intervention « ${res.intervention.code} » générée.`, 'success');
    if (state.currentPlanDetail && state.currentPlanDetail.id === planId) {
      await loadMaintenancePlanDetail(planId);
    } else {
      await loadMaintenancePlans(true);
    }
  } catch (err) {
    notify(err.message || 'Génération refusée.', 'error');
  }
}

async function togglePlanActive(planId) {
  let plan = state.maintenancePlans.find(p => p.id === planId);
  if (!plan && state.currentPlanDetail && state.currentPlanDetail.id === planId) {
    plan = state.currentPlanDetail;
  }
  const newActive = plan ? !plan.active : false;

  try {
    await api(`/api/maintenance/plans/${encodeURIComponent(planId)}`, {
      method: 'PATCH',
      body: { active: newActive },
    });
    notify(newActive ? 'Planning activé.' : 'Planning suspendu.', 'info');
    if (state.currentPlanDetail && state.currentPlanDetail.id === planId) {
      await loadMaintenancePlanDetail(planId);
    } else {
      await loadMaintenancePlans(false);
    }
  } catch (err) {
    notify(err.message || 'Action impossible.', 'error');
  }
}

async function deleteMaintenancePlan(planId) {
  const ok = await confirmDialog(
    'Supprimer le planning',
    'Voulez-vous vraiment supprimer logiquement ce planning de maintenance préventive ?',
    'Supprimer',
    true
  );
  if (!ok) return;

  try {
    await api(`/api/maintenance/plans/${encodeURIComponent(planId)}`, {
      method: 'DELETE',
    });
    notify('Planning supprimé.', 'success');
    location.hash = '#/maintenance-preventive';
  } catch (err) {
    notify(err.message || 'Suppression refusée.', 'error');
  }
}

// --- Gestion des compteurs d'équipements (Section fiche équipement) ---

function openCreateCounterModal(equipmentId) {
  const fieldsHtml = `
    <div class="form-group">
      <label for="fCounterName">Nom du compteur *</label>
      <input type="text" id="fCounterName" name="name" required placeholder="Ex. Heures de fonctionnement, Cycles…" />
    </div>
    <div class="form-row">
      <div class="form-group">
        <label for="fCounterUnit">Unité de mesure *</label>
        <input type="text" id="fCounterUnit" name="unit" required placeholder="Ex. h, cycles, km…" />
      </div>
      <div class="form-group">
        <label for="fCounterInitVal">Valeur initiale</label>
        <input type="number" id="fCounterInitVal" name="initial_value" min="0" value="0" />
      </div>
    </div>
  `;

  openFormModal({
    title: 'Ajouter un compteur à l\'équipement',
    formId: 'modalCreateCounterForm',
    submitAction: 'create-counter-submit',
    submitArgs: equipmentId,
    submitLabel: 'Créer le compteur',
    fieldsHtml,
  });
}

async function handleCreateCounterSubmit(form) {
  const equipmentId = form.dataset.args ? JSON.parse(form.dataset.args) : null;
  if (!equipmentId) return;

  const errBox = document.getElementById('modalCreateCounterFormErrorBox');
  const errText = document.getElementById('modalCreateCounterFormErrorBoxText');
  const btn = document.getElementById('modalCreateCounterFormSubmitBtn');

  const name = (form.name.value || '').trim();
  const unit = (form.unit.value || '').trim();
  const initialValue = parseInt(form.initial_value.value, 10) || 0;

  if (!name || !unit) {
    errText.textContent = 'Le nom et l\'unité sont obligatoires.';
    errBox.hidden = false;
    return;
  }

  btn.disabled = true;
  errBox.hidden = true;

  try {
    await api(`/api/equipments/${encodeURIComponent(equipmentId)}/counters`, {
      method: 'POST',
      body: { name, unit, initial_value: initialValue },
    });
    closeModal();
    notify('Compteur créé avec succès.', 'success');
    openEquipmentDetailModal(equipmentId);
  } catch (err) {
    errText.textContent = err.message || 'Erreur lors de la création du compteur.';
    errBox.hidden = false;
    btn.disabled = false;
  }
}

async function openEditCounterModal(arg) {
  const counterId = typeof arg === 'object' && arg !== null ? arg.counterId : arg;
  const equipmentId = typeof arg === 'object' && arg !== null ? arg.equipmentId : state.currentEquipmentDetailId;

  const fieldsHtml = `
    <div class="form-group">
      <label for="fEditCounterName">Nom du compteur *</label>
      <input type="text" id="fEditCounterName" name="name" required placeholder="Ex. Heures moteur" />
    </div>
    <div class="form-group">
      <label for="fEditCounterUnit">Unité de mesure *</label>
      <input type="text" id="fEditCounterUnit" name="unit" required placeholder="Ex. h" />
    </div>
    <div class="form-group">
      <label class="checkbox-label">
        <input type="checkbox" id="fEditCounterActive" name="active" value="1" checked />
        <span>Compteur actif</span>
      </label>
      <span class="form-help">Désactiver un compteur suspend les plannings associés.</span>
    </div>
  `;

  openFormModal({
    title: 'Modifier le compteur',
    formId: 'modalEditCounterForm',
    submitAction: 'edit-counter-submit',
    submitArgs: { counterId, equipmentId },
    submitLabel: 'Enregistrer',
    fieldsHtml,
  });
}

async function handleEditCounterSubmit(form) {
  const args = form.dataset.args ? JSON.parse(form.dataset.args) : null;
  const counterId = typeof args === 'object' && args !== null ? args.counterId : args;
  const equipmentId = typeof args === 'object' && args !== null ? args.equipmentId : state.currentEquipmentDetailId;
  if (!counterId) return;

  const errBox = document.getElementById('modalEditCounterFormErrorBox');
  const errText = document.getElementById('modalEditCounterFormErrorBoxText');
  const btn = document.getElementById('modalEditCounterFormSubmitBtn');

  const name = (form.name.value || '').trim();
  const unit = (form.unit.value || '').trim();
  const active = form.active ? form.active.checked : true;

  if (!name || !unit) {
    errText.textContent = 'Le nom et l\'unité sont obligatoires.';
    errBox.hidden = false;
    return;
  }

  btn.disabled = true;
  errBox.hidden = true;

  try {
    const updated = await api(`/api/counters/${encodeURIComponent(counterId)}`, {
      method: 'PATCH',
      body: { name, unit, active },
    });
    closeModal();
    notify('Compteur mis à jour.', 'success');
    const targetEqId = updated.equipment_id || equipmentId;
    if (targetEqId) {
      openEquipmentDetailModal(targetEqId);
    }
  } catch (err) {
    errText.textContent = err.message || 'Erreur lors de la modification du compteur.';
    errBox.hidden = false;
    btn.disabled = false;
  }
}

async function deleteCounter(arg) {
  const counterId = typeof arg === 'object' && arg !== null ? arg.counterId : arg;
  const equipmentId = typeof arg === 'object' && arg !== null ? arg.equipmentId : state.currentEquipmentDetailId;
  if (!counterId) return;

  const ok = await confirmDialog(
    'Supprimer le compteur',
    'Voulez-vous supprimer ce compteur ? Cette action est refusée s\'il est utilisé par des plannings de maintenance.',
    'Supprimer',
    true
  );
  if (!ok) return;

  try {
    await api(`/api/counters/${encodeURIComponent(counterId)}`, {
      method: 'DELETE',
    });
    notify('Compteur supprimé.', 'success');
    closeModal();
    if (equipmentId) {
      openEquipmentDetailModal(equipmentId);
    }
  } catch (err) {
    notify(err.message || 'Suppression refusée.', 'error');
  }
}

function openRecordReadingModal(arg) {
  const counterId = typeof arg === 'object' && arg !== null ? arg.counterId : arg;
  const equipmentId = typeof arg === 'object' && arg !== null ? arg.equipmentId : state.currentEquipmentDetailId;
  const nowIso = localIso().slice(0, 16);

  const fieldsHtml = `
    <div class="form-group">
      <label for="fReadingValue">Nouvelle valeur du compteur *</label>
      <input type="number" id="fReadingValue" name="value" min="0" required placeholder="Valeur relevée" />
      <span class="form-help">La valeur ne peut pas être inférieure à la valeur précédente sans remise à zéro.</span>
    </div>
    <div class="form-group">
      <label for="fReadingDate">Date et heure du relevé</label>
      <input type="datetime-local" id="fReadingDate" name="read_at" value="${nowIso}" />
    </div>
    <div class="form-group">
      <label for="fReadingNote">Commentaire ou observation</label>
      <input type="text" id="fReadingNote" name="note" placeholder="Relevé de fin de poste, compteur horaire…" />
    </div>
  `;

  openFormModal({
    title: 'Saisir un relevé de compteur',
    formId: 'modalRecordReadingForm',
    submitAction: 'record-reading-submit',
    submitArgs: { counterId, equipmentId },
    submitLabel: 'Enregistrer le relevé',
    fieldsHtml,
  });
}

async function handleRecordReadingSubmit(form) {
  const args = form.dataset.args ? JSON.parse(form.dataset.args) : null;
  const counterId = typeof args === 'object' && args !== null ? args.counterId : args;
  const equipmentId = typeof args === 'object' && args !== null ? args.equipmentId : state.currentEquipmentDetailId;
  if (!counterId) return;

  const errBox = document.getElementById('modalRecordReadingFormErrorBox');
  const errText = document.getElementById('modalRecordReadingFormErrorBoxText');
  const btn = document.getElementById('modalRecordReadingFormSubmitBtn');

  const valStr = (form.value.value || '').trim();
  const val = parseInt(valStr, 10);
  if (isNaN(val) || val < 0) {
    errText.textContent = 'La valeur doit être un entier positif ou nul.';
    errBox.hidden = false;
    return;
  }

  const readAt = form.read_at.value ? new Date(form.read_at.value).toISOString() : new Date().toISOString();
  const note = (form.note.value || '').trim() || null;

  btn.disabled = true;
  errBox.hidden = true;

  try {
    await api(`/api/counters/${encodeURIComponent(counterId)}/readings`, {
      method: 'POST',
      body: { value: val, read_at: readAt, is_reset: false, note },
    });
    closeModal();
    notify('Relevé enregistré avec succès.', 'success');
    if (equipmentId) {
      openEquipmentDetailModal(equipmentId);
    }
  } catch (err) {
    errText.textContent = err.message || 'Erreur lors de l\'enregistrement du relevé.';
    errBox.hidden = false;
    btn.disabled = false;
  }
}

function openResetCounterModal(arg) {
  const counterId = typeof arg === 'object' && arg !== null ? arg.counterId : arg;
  const equipmentId = typeof arg === 'object' && arg !== null ? arg.equipmentId : state.currentEquipmentDetailId;

  const fieldsHtml = `
    <div class="machine-state-alert">
      ${icon('i-alert')}
      <span>
        La remise à zéro ramène le compteur à la nouvelle valeur saisie (ex: remplacement machine ou horamètre).
        La distance restante jusqu'à la prochaine échéance des plannings liés est intégralement conservée.
      </span>
    </div>
    <div class="form-group">
      <label for="fResetValue">Nouvelle valeur de départ *</label>
      <input type="number" id="fResetValue" name="value" min="0" value="0" required />
    </div>
    <div class="form-group">
      <label for="fResetNote">Motif de la remise à zéro *</label>
      <input type="text" id="fResetNote" name="note" required placeholder="Remplacement du compteur physique, réfection totale…" />
    </div>
  `;

  openFormModal({
    title: 'Remise à zéro du compteur',
    formId: 'modalResetCounterForm',
    submitAction: 'reset-counter-submit',
    submitArgs: { counterId, equipmentId },
    submitLabel: 'Confirmer la remise à zéro',
    fieldsHtml,
  });
}

async function handleResetCounterSubmit(form) {
  const args = form.dataset.args ? JSON.parse(form.dataset.args) : null;
  const counterId = typeof args === 'object' && args !== null ? args.counterId : args;
  const equipmentId = typeof args === 'object' && args !== null ? args.equipmentId : state.currentEquipmentDetailId;
  if (!counterId) return;

  const errBox = document.getElementById('modalResetCounterFormErrorBox');
  const errText = document.getElementById('modalResetCounterFormErrorBoxText');
  const btn = document.getElementById('modalResetCounterFormSubmitBtn');

  const val = parseInt(form.value.value, 10);
  const note = (form.note.value || '').trim();

  if (isNaN(val) || val < 0) {
    errText.textContent = 'La valeur doit être un entier positif ou nul.';
    errBox.hidden = false;
    return;
  }
  if (!note) {
    errText.textContent = 'Le motif de la remise à zéro est obligatoire.';
    errBox.hidden = false;
    return;
  }

  btn.disabled = true;
  errBox.hidden = true;

  try {
    await api(`/api/counters/${encodeURIComponent(counterId)}/readings`, {
      method: 'POST',
      body: { value: val, is_reset: true, note },
    });
    closeModal();
    notify('Compteur réinitialisé et plannings réalignés.', 'success');
    if (equipmentId) {
      openEquipmentDetailModal(equipmentId);
    }
  } catch (err) {
    errText.textContent = err.message || 'Erreur lors de la remise à zéro.';
    errBox.hidden = false;
    btn.disabled = false;
  }
}

async function openCounterHistoryModal(arg) {
  const counterId = typeof arg === 'object' && arg !== null ? arg.counterId : arg;
  const equipmentId = typeof arg === 'object' && arg !== null ? arg.equipmentId : state.currentEquipmentDetailId;

  openModal('Historique des relevés', `<p class="hint">${icon('i-clock')} Chargement de l'historique…</p>`);

  try {
    const res = await api(`/api/counters/${encodeURIComponent(counterId)}/readings?limit=50`);
    const items = res && Array.isArray(res.items) ? res.items : [];

    let rowsHtml = items.map(r => `
      <tr>
        <td><b>${formatDate(r.read_at)}</b></td>
        <td class="mono"><b>${esc(r.value)}</b></td>
        <td>${r.is_reset ? '<span class="badge badge-state-warn">Remise à zéro</span>' : '<span class="badge">Relevé normal</span>'}</td>
        <td>${esc(r.recorded_by ? r.recorded_by.display_name : '—')}</td>
        <td>${esc(r.note || '—')}</td>
      </tr>
    `).join('');

    if (!items.length) {
      rowsHtml = `<tr><td colspan="5" class="empty-cell">Aucun relevé enregistré pour ce compteur.</td></tr>`;
    }

    const bodyHtml = `
      <div class="table-container">
        <table class="data-table">
          <thead>
            <tr>
              <th>Date du relevé</th>
              <th>Valeur</th>
              <th>Type</th>
              <th>Enregistré par</th>
              <th>Note</th>
            </tr>
          </thead>
          <tbody>
            ${rowsHtml}
          </tbody>
        </table>
      </div>
    `;

    const closeBtn = equipmentId ?
      `<button type="button" class="btn primary" data-action="open-detail-equipment" data-args="${jsArg(equipmentId)}">Retour à l'équipement</button>` :
      `<button type="button" class="btn" data-action="close-modal">Fermer</button>`;
    openModal('Historique des relevés', bodyHtml, closeBtn);
  } catch (err) {
    openModal('Erreur', `<p class="hint state-ko">${esc(err.message)}</p>`, `<button type="button" class="btn" data-action="close-modal">Fermer</button>`);
  }
}

async function updateCounterSelectForEquipment(equipmentId) {
  const select = document.getElementById('fPlanCounterSelect');
  if (!select) return;

  if (!equipmentId) {
    select.innerHTML = '<option value="">-- Choisir un équipement d\'abord --</option>';
    return;
  }

  select.innerHTML = '<option value="">Chargement des compteurs…</option>';
  try {
    const cRes = await api(`/api/equipments/${encodeURIComponent(equipmentId)}/counters`);
    const counters = (Array.isArray(cRes) ? cRes : (cRes && Array.isArray(cRes.items) ? cRes.items : [])).filter(c => c.active);
    if (!counters.length) {
      select.innerHTML = '<option value="">Aucun compteur actif sur cet équipement</option>';
    } else {
      select.innerHTML = '<option value="">-- Choisir un compteur actif --</option>' +
        counters.map(c => `<option value="${esc(c.id)}">${esc(c.name)} (${esc(c.current_value)} ${esc(c.unit)})</option>`).join('');
    }
  } catch (_) {
    select.innerHTML = '<option value="">Erreur chargement compteurs</option>';
  }
}

// ============================================================================
// 14H. MODULE DOCUMENTS ET ÉTIQUETTES (PHASE 6)
// ============================================================================

const EXT_TO_TYPE_ID = {
  pdf: 'pdf',
  jpg: 'jpeg', jpeg: 'jpeg',
  png: 'png',
  webp: 'webp',
  docx: 'docx', doc: 'doc',
  xlsx: 'xlsx', xls: 'xls',
  pptx: 'pptx', ppt: 'ppt',
  txt: 'txt', csv: 'csv', rtf: 'rtf',
  odt: 'odt', ods: 'ods',
  dwg: 'dwg', dxf: 'dxf', step: 'step', stp: 'step', iges: 'iges', igs: 'iges',
  zip: 'zip'
};

function uploadFileWithProgress(url, formData, onProgress, method = 'POST') {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open(method, url, true);
    xhr.responseType = 'json';
    if (xhr.upload && onProgress) {
      xhr.upload.onprogress = (e) => {
        if (e.lengthComputable) {
          const percent = Math.round((e.loaded / e.total) * 100);
          onProgress(percent);
        }
      };
    }
    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 300) {
        resolve(xhr.response);
      } else {
        const errorMsg = (xhr.response && typeof xhr.response.message === 'string' && xhr.response.message) || `Erreur serveur (${xhr.status})`;
        const err = new Error(errorMsg);
        err.status = xhr.status;
        reject(err);
      }
    };
    xhr.onerror = () => reject(new Error('Erreur réseau lors de l\'envoi du fichier.'));
    xhr.send(formData);
  });
}

// Catégories de documents acceptées par le serveur (valeur stockée, libellé affiché).
const DOC_CATEGORIES = [
  ['notice', 'Notice / Manuel'],
  ['schéma', 'Plan / Schéma'],
  ['certificat', 'Certificat / Conformité'],
  ['photo', 'Photo'],
  ['rapport', 'Rapport'],
  ['gamme', 'Gamme / Mode opératoire'],
  ['autre', 'Autre'],
];

function docCategoryOptions(selected, emptyLabel) {
  const current = selected || '';
  return [['', emptyLabel], ...DOC_CATEGORIES]
    .map(([v, label]) => `<option value="${esc(v)}"${v === current ? ' selected' : ''}>${esc(label)}</option>`)
    .join('');
}

async function ensureDocumentSettings() {
  if (!state.documentSettings) {
    try {
      state.documentSettings = await api('/api/settings/documents');
    } catch (_) {
      state.documentSettings = {
        allowed_types: ['pdf', 'jpeg', 'png', 'webp', 'docx', 'doc', 'xlsx', 'xls', 'dwg', 'dxf', 'step'],
        max_size_mb: 50,
        catalog: []
      };
    }
  }
  return state.documentSettings;
}

function validateDocumentFile(file, settings) {
  if (!file) return 'Veuillez sélectionner un fichier à téléverser.';
  const nameParts = file.name.split('.');
  if (nameParts.length < 2) return 'Le fichier ne comporte pas d\'extension valide.';
  const ext = nameParts.pop().toLowerCase();
  const typeId = EXT_TO_TYPE_ID[ext];
  if (!typeId || !Array.isArray(settings.allowed_types) || !settings.allowed_types.includes(typeId)) {
    return `Le format « .${ext} » n'est pas autorisé par les paramètres du site.`;
  }
  const maxBytes = (settings.max_size_mb || 50) * 1024 * 1024;
  if (file.size > maxBytes) {
    return `La taille du fichier (${formatBytes(file.size)}) dépasse le plafond autorisé de ${settings.max_size_mb} Mo.`;
  }
  return null;
}

async function loadAndRenderEntityDocuments(entityType, entityId, containerId) {
  const container = document.getElementById(containerId);
  if (!container) return;

  if (!hasPermission('documents.view')) {
    container.innerHTML = '';
    return;
  }

  container.innerHTML = `
    <div class="detail-card detail-card-spaced documents-section">
      <p class="hint">${icon('i-clock')} Chargement des documents…</p>
    </div>
  `;

  try {
    const res = await api(`/api/documents?entity_type=${encodeURIComponent(entityType)}&entity_id=${encodeURIComponent(entityId)}`);
    const rawList = Array.isArray(res) ? res : (res && Array.isArray(res.items) ? res.items : []);
    const docs = rawList.map(adaptDocument);
    state.entityDocuments.set(`${entityType}:${entityId}`, docs);
    renderEntityDocumentsView(entityType, entityId, containerId, docs);
  } catch (err) {
    container.innerHTML = `
      <div class="detail-card detail-card-spaced documents-section">
        <p class="hint state-ko">${icon('i-alert')} Impossible de charger les documents : ${esc(err.message)}</p>
      </div>
    `;
  }
}

function renderEntityDocumentsView(entityType, entityId, containerId, docs) {
  const container = document.getElementById(containerId);
  if (!container) return;

  const canCreate = hasPermission('documents.create');
  const canDelete = hasPermission('documents.delete');

  const dropzoneHtml = canCreate ? `
    <div class="dropzone" id="dropzone-${esc(containerId)}" data-entity-type="${esc(entityType)}" data-entity-id="${esc(entityId)}" data-container-id="${esc(containerId)}">
      ${icon('i-upload')}
      <div class="dropzone-text">
        <span>Glissez-déposez un document ici, ou</span>
        <button type="button" class="btn sm" data-action="open-upload-document" data-args="${jsArg({ entityType, entityId, containerId })}">
          ${icon('i-plus')} <span>Parcourir</span>
        </button>
      </div>
    </div>
  ` : '';

  let docsHtml = '';
  if (!docs.length) {
    docsHtml = '<p class="hint">Aucun document joint pour le moment.</p>';
  } else {
    docsHtml = `
      <div class="documents-grid">
        ${docs.map(doc => {
          const v = doc.current_version || {};
          const isInline = isDocInline(doc);
          const author = v.uploaded_by ? v.uploaded_by.display_name : '—';
          return `
            <div class="document-card">
              <div class="document-card-top">
                <span class="document-card-icon">${icon(docFileIcon(v.mime_type || v.original_name))}</span>
                <div class="document-card-title-block">
                  <h4 class="document-card-title">${esc(doc.title)}</h4>
                  <div class="document-card-filename mono hint">${esc(v.original_name || 'Fichier')}</div>
                </div>
                <span class="badge ${doc.category ? 'badge-info' : ''}">${esc(formatDocCategory(doc.category))}</span>
              </div>
              ${doc.description ? `<p class="document-card-desc">${esc(doc.description)}</p>` : ''}
              <div class="document-card-meta hint">
                <span>Version <b>v${esc(v.version_no || 1)}</b></span>
                <span>•</span>
                <span>${esc(formatBytes(v.size_bytes))}</span>
                <span>•</span>
                <span>${esc(formatDate(v.uploaded_at))}</span>
                <span>par ${esc(author)}</span>
              </div>
              <div class="document-card-actions">
                ${isInline ? `
                  <a href="/api/documents/${esc(doc.id)}/download" target="_blank" rel="noopener noreferrer" class="btn sm" title="Ouvrir dans un nouvel onglet">
                    ${icon('i-eye')} <span>Ouvrir</span>
                  </a>
                ` : ''}
                <a href="/api/documents/${esc(doc.id)}/download?download=1" class="btn sm" title="Télécharger le fichier">
                  ${icon('i-download')} <span>Télécharger</span>
                </a>
                ${canCreate ? `
                  <button type="button" class="btn sm" data-action="open-new-version" data-args="${jsArg({ documentId: doc.id, entityType, entityId, containerId })}" title="Téléverser une nouvelle révision">
                    ${icon('i-upload')} <span>Nouvelle version</span>
                  </button>
                ` : ''}
                <button type="button" class="btn sm" data-action="open-document-history" data-args="${jsArg({ documentId: doc.id, entityType, entityId, containerId })}" title="Historique des versions">
                  ${icon('i-history')} <span>Versions (${esc(doc.versions_count || 1)})</span>
                </button>
                ${canCreate ? `
                  <button type="button" class="btn sm" data-action="open-edit-document" data-args="${jsArg({ documentId: doc.id, entityType, entityId, containerId })}" title="Modifier le titre ou la catégorie">
                    ${icon('i-edit')}
                  </button>
                ` : ''}
                ${canDelete ? `
                  <button type="button" class="btn sm danger" data-action="delete-document" data-args="${jsArg({ documentId: doc.id, entityType, entityId, containerId })}" title="Supprimer le document">
                    ${icon('i-trash')}
                  </button>
                ` : ''}
              </div>
            </div>
          `;
        }).join('')}
      </div>
    `;
  }

  container.innerHTML = `
    <div class="detail-card detail-card-spaced documents-section">
      <div class="detail-nav-bar">
        <h3>${icon('i-doc')} Documents joints (${docs.length})</h3>
        ${canCreate ? `
          <button type="button" class="btn sm primary" data-action="open-upload-document" data-args="${jsArg({ entityType, entityId, containerId })}">
            ${icon('i-plus')} <span>Ajouter un document</span>
          </button>
        ` : ''}
      </div>
      ${dropzoneHtml}
      ${docsHtml}
    </div>
  `;
}

function handleDropzoneDragOver(e) {
  const dz = e.target.closest('.dropzone');
  if (dz) {
    e.preventDefault();
    dz.classList.add('dragover');
  }
}

function handleDropzoneDragLeave(e) {
  const dz = e.target.closest('.dropzone');
  if (dz) {
    dz.classList.remove('dragover');
  }
}

function handleDropzoneDrop(e) {
  const dz = e.target.closest('.dropzone');
  if (dz) {
    e.preventDefault();
    dz.classList.remove('dragover');
    const entityType = dz.dataset.entityType;
    const entityId = dz.dataset.entityId;
    const containerId = dz.dataset.containerId;
    if (e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files.length > 0) {
      openUploadDocModal({ entityType, entityId, containerId, preselectedFile: e.dataTransfer.files[0] });
    }
  }
}

async function openUploadDocModal({ entityType, entityId, containerId, preselectedFile = null }) {
  await ensureDocumentSettings();
  const maxMb = state.documentSettings ? state.documentSettings.max_size_mb : 50;

  const bodyHtml = `
    <form id="docUploadForm" class="modal-form">
      <input type="hidden" name="entity_type" value="${esc(entityType)}" />
      <input type="hidden" name="entity_id" value="${esc(entityId)}" />
      <input type="hidden" name="container_id" value="${esc(containerId || '')}" />

      <div class="form-group">
        <label for="docFileInput">Fichier à joindre *</label>
        <input type="file" id="docFileInput" name="file" required />
        <span class="form-help">Formats autorisés selon réglages. Taille max : <b>${esc(maxMb)} Mo</b>.</span>
      </div>

      <div class="form-group">
        <label for="docTitleInput">Titre du document *</label>
        <input type="text" id="docTitleInput" name="title" required placeholder="ex. Schéma électrique armoire principale" />
        <span class="form-help">Intitulé clair pour la recherche transversale.</span>
      </div>

      <div class="form-row">
        <div class="form-group">
          <label for="docCategorySelect">Catégorie</label>
          <select id="docCategorySelect" name="category">
            ${docCategoryOptions('', 'Sans catégorie')}
          </select>
        </div>
      </div>

      <div class="form-group">
        <label for="docDescInput">Description ou notes (optionnel)</label>
        <textarea id="docDescInput" name="description" rows="2" placeholder="Observations, version papier, indices de révision…"></textarea>
      </div>

      <div id="docUploadProgressContainer" class="upload-progress-container" hidden>
        <div class="upload-progress-label">
          <span>Téléversement en cours…</span>
          <span id="docUploadProgressPercent" class="mono font-bold">0%</span>
        </div>
        <div class="upload-progress-bar">
          <div id="docUploadProgressFill" class="upload-progress-fill"></div>
        </div>
      </div>

      <div id="docUploadError" class="form-error" hidden></div>
    </form>
  `;

  const footerHtml = `
    <button type="button" class="btn" data-action="close-modal">Annuler</button>
    <button type="submit" form="docUploadForm" class="btn primary" id="btnSubmitDocUpload">
      ${icon('i-upload')} <span>Téléverser</span>
    </button>
  `;

  openModal('Ajouter un document', bodyHtml, footerHtml);

  if (preselectedFile) {
    const fileInput = document.getElementById('docFileInput');
    if (fileInput) {
      const dt = new DataTransfer();
      dt.items.add(preselectedFile);
      fileInput.files = dt.files;
      const titleInput = document.getElementById('docTitleInput');
      if (titleInput && !titleInput.value) {
        titleInput.value = preselectedFile.name.replace(/\.[^/.]+$/, '');
      }
    }
  }
}

async function submitDocUpload(form) {
  const errBox = document.getElementById('docUploadError');
  const progBox = document.getElementById('docUploadProgressContainer');
  const progFill = document.getElementById('docUploadProgressFill');
  const progText = document.getElementById('docUploadProgressPercent');
  const submitBtn = document.getElementById('btnSubmitDocUpload');

  if (errBox) errBox.hidden = true;

  const fileInput = form.file;
  const file = fileInput && fileInput.files ? fileInput.files[0] : null;
  const docSettings = await ensureDocumentSettings();

  const valErr = validateDocumentFile(file, docSettings);
  if (valErr) {
    if (errBox) {
      errBox.textContent = valErr;
      errBox.hidden = false;
    }
    return;
  }

  // Seuls les champs attendus par le serveur : il refuse tout champ multipart inconnu.
  const formData = new FormData();
  formData.append('entity_type', form.entity_type.value);
  formData.append('entity_id', form.entity_id.value);
  formData.append('title', form.title.value.trim());
  if (form.category.value) formData.append('category', form.category.value);
  if (form.description.value.trim()) formData.append('description', form.description.value.trim());
  formData.append('file', file);
  const entityType = form.entity_type.value;
  const entityId = form.entity_id.value;
  const containerId = form.container_id.value;

  if (progBox) progBox.hidden = false;
  if (submitBtn) submitBtn.disabled = true;

  try {
    await uploadFileWithProgress('/api/documents', formData, (percent) => {
      if (progFill) progFill.style.width = `${percent}%`;
      if (progText) progText.textContent = `${percent}%`;
    });

    closeModal();
    notify('Document ajouté avec succès.', 'success');

    if (containerId) {
      await loadAndRenderEntityDocuments(entityType, entityId, containerId);
    }
    if (currentRoute() === 'documents') {
      await loadDocumentsList(true);
    }
  } catch (err) {
    if (progBox) progBox.hidden = true;
    if (submitBtn) submitBtn.disabled = false;
    if (errBox) {
      errBox.textContent = err.message || 'Erreur lors du téléversement.';
      errBox.hidden = false;
    }
  }
}

async function openNewVersionModal({ documentId, entityType, entityId, containerId }) {
  await ensureDocumentSettings();
  const maxMb = state.documentSettings ? state.documentSettings.max_size_mb : 50;

  const bodyHtml = `
    <form id="docVersionForm" class="modal-form">
      <input type="hidden" name="document_id" value="${esc(documentId)}" />
      <input type="hidden" name="entity_type" value="${esc(entityType || '')}" />
      <input type="hidden" name="entity_id" value="${esc(entityId || '')}" />
      <input type="hidden" name="container_id" value="${esc(containerId || '')}" />

      <div class="form-group">
        <label for="versionFileInput">Nouveau fichier de révision *</label>
        <input type="file" id="versionFileInput" name="file" required />
        <span class="form-help">Formats autorisés selon réglages. Max : <b>${esc(maxMb)} Mo</b>.</span>
      </div>

      <div class="form-group">
        <label for="versionCommentInput">Commentaire de révision (optionnel)</label>
        <input type="text" id="versionCommentInput" name="comment" placeholder="ex. Mise à jour plan de câblage 2026" />
      </div>

      <div id="versionUploadProgressContainer" class="upload-progress-container" hidden>
        <div class="upload-progress-label">
          <span>Envoi de la nouvelle version…</span>
          <span id="versionUploadProgressPercent" class="mono font-bold">0%</span>
        </div>
        <div class="upload-progress-bar">
          <div id="versionUploadProgressFill" class="upload-progress-fill"></div>
        </div>
      </div>

      <div id="versionUploadError" class="form-error" hidden></div>
    </form>
  `;

  const footerHtml = `
    <button type="button" class="btn" data-action="close-modal">Annuler</button>
    <button type="submit" form="docVersionForm" class="btn primary" id="btnSubmitDocVersion">
      ${icon('i-upload')} <span>Enregistrer la nouvelle version</span>
    </button>
  `;

  openModal('Nouvelle version du document', bodyHtml, footerHtml);
}

async function submitNewVersion(form) {
  const errBox = document.getElementById('versionUploadError');
  const progBox = document.getElementById('versionUploadProgressContainer');
  const progFill = document.getElementById('versionUploadProgressFill');
  const progText = document.getElementById('versionUploadProgressPercent');
  const submitBtn = document.getElementById('btnSubmitDocVersion');

  if (errBox) errBox.hidden = true;

  const fileInput = form.file;
  const file = fileInput && fileInput.files ? fileInput.files[0] : null;
  const docSettings = await ensureDocumentSettings();

  const valErr = validateDocumentFile(file, docSettings);
  if (valErr) {
    if (errBox) {
      errBox.textContent = valErr;
      errBox.hidden = false;
    }
    return;
  }

  const formData = new FormData();
  formData.append('file', file);
  if (form.comment && form.comment.value.trim()) {
    formData.append('comment', form.comment.value.trim());
  }

  const documentId = form.document_id.value;
  const entityType = form.entity_type.value;
  const entityId = form.entity_id.value;
  const containerId = form.container_id.value;

  if (progBox) progBox.hidden = false;
  if (submitBtn) submitBtn.disabled = true;

  try {
    await uploadFileWithProgress(`/api/documents/${encodeURIComponent(documentId)}/versions`, formData, (percent) => {
      if (progFill) progFill.style.width = `${percent}%`;
      if (progText) progText.textContent = `${percent}%`;
    });

    closeModal();
    notify('Nouvelle version enregistrée avec succès.', 'success');

    if (containerId) {
      await loadAndRenderEntityDocuments(entityType, entityId, containerId);
    }
    if (currentRoute() === 'documents') {
      await loadDocumentsList(true);
    }
  } catch (err) {
    if (progBox) progBox.hidden = true;
    if (submitBtn) submitBtn.disabled = false;
    if (errBox) {
      if (err.status === 409) {
        errBox.textContent = 'Ce fichier est strictement identique à la version actuelle du document (doublon rejeté).';
      } else {
        errBox.textContent = err.message || 'Erreur lors de l\'envoi de la révision.';
      }
      errBox.hidden = false;
    }
  }
}

async function openDocumentHistoryModal({ documentId, entityType, entityId, containerId }) {
  openModal('Historique des versions', `<p class="hint">${icon('i-clock')} Chargement des versions…</p>`);

  try {
    const [doc, versionsRes] = await Promise.all([
      api(`/api/documents/${encodeURIComponent(documentId)}`),
      api(`/api/documents/${encodeURIComponent(documentId)}/versions`),
    ]);

    const versions = Array.isArray(versionsRes) ? versionsRes : (versionsRes && Array.isArray(versionsRes.items) ? versionsRes.items : []);
    const canCreate = hasPermission('documents.create');
    const curVerNo = doc.current_version ? doc.current_version.version_no : (versions[0] ? versions[0].version_no : 1);

    const rows = versions.map(v => {
      const isCur = v.version_no === curVerNo;
      const author = v.uploaded_by ? v.uploaded_by.display_name : '—';
      return `
        <tr>
          <td>
            <b class="mono">v${esc(v.version_no)}</b>
            ${isCur ? ' <span class="badge badge-active-on">Actuelle</span>' : ''}
          </td>
          <td>
            <div class="mono font-bold">${esc(v.original_name)}</div>
            ${v.comment ? `<div class="hint">« ${esc(v.comment)} »</div>` : ''}
          </td>
          <td class="mono">${esc(formatBytes(v.size_bytes))}</td>
          <td>
            <div>${esc(author)}</div>
            <div class="hint">${esc(formatDate(v.uploaded_at))}</div>
          </td>
          <td>
            <div class="cell-actions">
              <a href="/api/documents/${esc(documentId)}/download?version=${esc(v.version_no)}&download=1" class="btn sm" title="Télécharger cette version">
                ${icon('i-download')} <span>Télécharger</span>
              </a>
              ${canCreate && !isCur ? `
                <button type="button" class="btn sm primary" data-action="restore-doc-version" data-args="${jsArg({ documentId, versionNo: v.version_no, entityType, entityId, containerId })}" title="Restaurer cette version en créant une nouvelle révision">
                  ${icon('i-refresh')} <span>Restaurer</span>
                </button>
              ` : ''}
            </div>
          </td>
        </tr>
      `;
    }).join('');

    const bodyHtml = `
      <div class="modal-intro">
        <h3>${esc(doc.title)}</h3>
        <p class="hint">Consultez et téléchargez les anciennes versions de ce document, ou restaurez une révision antérieure.</p>
      </div>
      <div class="table-container">
        <table class="data-table">
          <thead>
            <tr>
              <th>Version</th>
              <th>Fichier original & Commentaire</th>
              <th>Taille</th>
              <th>Auteur & Date</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            ${rows.length ? rows : '<tr><td colspan="5" class="empty-cell">Aucune version répertoriée.</td></tr>'}
          </tbody>
        </table>
      </div>
    `;

    const footerHtml = `<button type="button" class="btn" data-action="close-modal">Fermer</button>`;
    openModal(`Versions : ${doc.title}`, bodyHtml, footerHtml);
  } catch (err) {
    openModal('Erreur', `<p class="hint state-ko">${esc(err.message)}</p>`, `<button type="button" class="btn" data-action="close-modal">Fermer</button>`);
  }
}

async function restoreDocumentVersion({ documentId, versionNo, entityType, entityId, containerId }) {
  const ok = await confirmDialog(
    'Restaurer une version',
    `Voulez-vous restaurer la version v${versionNo} ? Cela créera une nouvelle version active basée sur cette archive.`,
    'Restaurer'
  );
  if (!ok) return;

  try {
    await api(`/api/documents/${encodeURIComponent(documentId)}/versions/${encodeURIComponent(versionNo)}/restore`, { method: 'POST' });
    notify(`Version v${versionNo} restaurée avec succès.`, 'success');
    closeModal();
    if (containerId) {
      await loadAndRenderEntityDocuments(entityType, entityId, containerId);
    }
    if (currentRoute() === 'documents') {
      await loadDocumentsList(true);
    }
  } catch (err) {
    notify(err.message || 'Erreur lors de la restauration.', 'error');
  }
}

async function openEditDocumentModal({ documentId, entityType, entityId, containerId }) {
  openModal('Modifier le document', `<p class="hint">${icon('i-clock')} Chargement du document…</p>`);

  try {
    const doc = await api(`/api/documents/${encodeURIComponent(documentId)}`);

    const bodyHtml = `
      <form id="editDocForm" class="modal-form">
        <input type="hidden" name="document_id" value="${esc(documentId)}" />
        <input type="hidden" name="entity_type" value="${esc(entityType || '')}" />
        <input type="hidden" name="entity_id" value="${esc(entityId || '')}" />
        <input type="hidden" name="container_id" value="${esc(containerId || '')}" />

        <div class="form-group">
          <label for="editDocTitleInput">Titre du document *</label>
          <input type="text" id="editDocTitleInput" name="title" required value="${esc(doc.title)}" />
        </div>

        <div class="form-group">
          <label for="editDocCategorySelect">Catégorie</label>
          <select id="editDocCategorySelect" name="category">
            ${docCategoryOptions(doc.category, 'Sans catégorie')}
          </select>
        </div>

        <div class="form-group">
          <label for="editDocDescInput">Description ou notes</label>
          <textarea id="editDocDescInput" name="description" rows="3">${esc(doc.description || '')}</textarea>
        </div>

        <div id="editDocError" class="form-error" hidden></div>
      </form>
    `;

    const footerHtml = `
      <button type="button" class="btn" data-action="close-modal">Annuler</button>
      <button type="submit" form="editDocForm" class="btn primary">
        ${icon('i-check')} <span>Enregistrer</span>
      </button>
    `;

    openModal('Modifier le document', bodyHtml, footerHtml);
  } catch (err) {
    openModal('Erreur', `<p class="hint state-ko">${esc(err.message)}</p>`, `<button type="button" class="btn" data-action="close-modal">Fermer</button>`);
  }
}

async function submitEditDocument(form) {
  const errBox = document.getElementById('editDocError');
  if (errBox) errBox.hidden = true;

  const documentId = form.document_id.value;
  const entityType = form.entity_type.value;
  const entityId = form.entity_id.value;
  const containerId = form.container_id.value;

  const payload = {
    title: form.title.value.trim(),
    category: form.category.value || null,
    description: form.description.value.trim() || null
  };

  try {
    await api(`/api/documents/${encodeURIComponent(documentId)}`, {
      method: 'PATCH',
      body: payload
    });

    closeModal();
    notify('Document mis à jour avec succès.', 'success');

    if (containerId) {
      await loadAndRenderEntityDocuments(entityType, entityId, containerId);
    }
    if (currentRoute() === 'documents') {
      await loadDocumentsList(true);
    }
  } catch (err) {
    if (errBox) {
      errBox.textContent = err.message || 'Erreur lors de la modification.';
      errBox.hidden = false;
    }
  }
}

async function deleteDocument({ documentId, entityType, entityId, containerId }) {
  const ok = await confirmDialog(
    'Supprimer le document',
    'Voulez-vous supprimer ce document ? L\'ensemble de ses versions sera archivé.',
    'Supprimer',
    true
  );
  if (!ok) return;

  try {
    await api(`/api/documents/${encodeURIComponent(documentId)}`, { method: 'DELETE' });
    notify('Document supprimé avec succès.', 'success');

    if (containerId) {
      await loadAndRenderEntityDocuments(entityType, entityId, containerId);
    }
    if (currentRoute() === 'documents') {
      await loadDocumentsList(true);
    }
  } catch (err) {
    notify(err.message || 'Erreur lors de la suppression.', 'error');
  }
}

function documentsPage() {
  return `
    <div class="page-head">
      <div class="page-head-title">
        ${icon('i-doc')}
        <div>
          <h1>Documents et pièces jointes</h1>
          <p class="page-subtitle">Vue transversale des manuels, schémas, procédures et rapports de maintenance.</p>
        </div>
      </div>
    </div>

    <div class="toolbar">
      <div class="toolbar-left">
        <div class="search-box">
          ${icon('i-search')}
          <input type="search" id="documentsSearchInput" placeholder="Rechercher par titre ou nom de fichier…" value="${esc(state.documentsSearchQuery)}" autocomplete="off" />
        </div>
        <div class="filter-select-wrap">
          <label for="documentsEntityTypeSelect" class="sr-only">Type d'entité</label>
          <select id="documentsEntityTypeSelect" data-action="filter-documents-entity-type">
            <option value="" ${!state.documentsEntityTypeFilter ? 'selected' : ''}>Toutes les entités</option>
            <option value="equipment" ${state.documentsEntityTypeFilter === 'equipment' ? 'selected' : ''}>Équipements</option>
            <option value="intervention" ${state.documentsEntityTypeFilter === 'intervention' ? 'selected' : ''}>Interventions</option>
            <option value="part" ${state.documentsEntityTypeFilter === 'part' ? 'selected' : ''}>Pièces détachées</option>
            <option value="maintenance_plan" ${state.documentsEntityTypeFilter === 'maintenance_plan' ? 'selected' : ''}>Plannings préventifs</option>
          </select>
        </div>
        <div class="filter-select-wrap">
          <label for="documentsCategorySelect" class="sr-only">Catégorie</label>
          <select id="documentsCategorySelect" data-action="filter-documents-category">
            ${docCategoryOptions(state.documentsCategoryFilter, 'Toutes les catégories')}
          </select>
        </div>
      </div>
      <div class="toolbar-right">
        <span id="documentsCountInfo" class="hint mono"></span>
      </div>
    </div>

    <div id="documentsListContainer">
      <p class="hint">${icon('i-clock')} Chargement des documents…</p>
    </div>
  `;
}

async function initDocumentsPageView() {
  await loadDocumentsList(true);
}

async function loadDocumentsList(reset = true) {
  const container = document.getElementById('documentsListContainer');
  if (!container) return;

  if (reset) {
    state.documents = [];
    state.documentsNextCursor = null;
    container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement des documents…</p>`;
  }

  state.documentsLoading = true;

  try {
    const params = new URLSearchParams();
    if (state.documentsSearchQuery) params.set('q', state.documentsSearchQuery);
    if (state.documentsEntityTypeFilter) params.set('entity_type', state.documentsEntityTypeFilter);
    if (state.documentsCategoryFilter) params.set('category', state.documentsCategoryFilter);
    if (state.selectedSiteId) params.set('site_id', state.selectedSiteId);
    params.set('limit', '25');
    if (!reset && state.documentsNextCursor) {
      params.set('cursor', state.documentsNextCursor);
    }

    const res = await api(`/api/documents/search?${params.toString()}`);
    const rawList = Array.isArray(res) ? res : (res && Array.isArray(res.items) ? res.items : []);
    const items = rawList.map(adaptDocument);
    const nextCursor = res && res.next_cursor ? res.next_cursor : null;

    if (reset) {
      state.documents = items;
    } else {
      state.documents = state.documents.concat(items);
    }
    state.documentsNextCursor = nextCursor;
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur lors du chargement des documents : ${esc(err.message)}</p>`;
    state.documentsLoading = false;
    return;
  }

  state.documentsLoading = false;
  renderDocumentsListView();
}

function renderDocumentsListView() {
  const container = document.getElementById('documentsListContainer');
  if (!container) return;

  const countInfo = document.getElementById('documentsCountInfo');
  if (countInfo) {
    const len = state.documents.length;
    countInfo.textContent = `${len} document${len > 1 ? 's' : ''}`;
  }

  if (!state.documents.length) {
    container.innerHTML = `
      <div class="card">
        <p class="hint">Aucun document ne correspond aux filtres appliqués.</p>
      </div>
    `;
    return;
  }

  const canCreate = hasPermission('documents.create');
  const canDelete = hasPermission('documents.delete');

  const rows = state.documents.map(doc => {
    const v = doc.current_version || {};
    const ent = doc.entity || {};
    const isInline = isDocInline(doc);
    const author = v.uploaded_by ? v.uploaded_by.display_name : '—';

    let entityLinkHtml = '—';
    if (ent.type === 'equipment') {
      entityLinkHtml = `
        <span class="badge badge-subtle">Équipement</span>
        <button type="button" class="link-action font-bold mono" data-action="open-detail-equipment" data-args="${jsArg(ent.id)}">
          ${esc(ent.code || ent.label || 'Équipement')}
        </button>
      `;
    } else if (ent.type === 'intervention') {
      entityLinkHtml = `
        <span class="badge badge-subtle">Intervention</span>
        <a href="#/interventions/${esc(ent.id)}" class="link-action font-bold mono">
          ${esc(ent.code || ent.label || 'Intervention')}
        </a>
      `;
    } else if (ent.type === 'part') {
      entityLinkHtml = `
        <span class="badge badge-subtle">Pièce</span>
        <a href="#/pieces/${esc(ent.id)}" class="link-action font-bold mono">
          ${esc(ent.code || ent.label || 'Pièce')}
        </a>
      `;
    } else if (ent.type === 'maintenance_plan') {
      entityLinkHtml = `
        <span class="badge badge-subtle">Planning</span>
        <a href="#/maintenance-preventive/${esc(ent.id)}" class="link-action font-bold mono">
          ${esc(ent.code || ent.label || 'Planning')}
        </a>
      `;
    }

    return `
      <tr>
        <td>
          <div class="cell-doc-title">
            <span class="doc-icon">${icon(docFileIcon(v.mime_type || v.original_name))}</span>
            <div>
              <b>${esc(doc.title)}</b>
              <div class="hint mono">${esc(v.original_name || '')}</div>
            </div>
          </div>
        </td>
        <td>
          <div class="cell-entity">${entityLinkHtml}</div>
        </td>
        <td><span class="badge ${doc.category ? 'badge-info' : ''}">${esc(formatDocCategory(doc.category))}</span></td>
        <td>
          <span class="badge font-bold">v${esc(v.version_no || 1)}</span>
          <span class="hint mono">${esc(formatBytes(v.size_bytes))}</span>
        </td>
        <td>
          <div>${esc(author)}</div>
          <div class="hint">${esc(formatDate(v.uploaded_at))}</div>
        </td>
        <td>
          <div class="cell-actions">
            ${isInline ? `
              <a href="/api/documents/${esc(doc.id)}/download" target="_blank" rel="noopener noreferrer" class="btn sm" title="Ouvrir dans un nouvel onglet">
                ${icon('i-eye')} <span>Ouvrir</span>
              </a>
            ` : ''}
            <a href="/api/documents/${esc(doc.id)}/download?download=1" class="btn sm" title="Télécharger">
              ${icon('i-download')}
            </a>
            ${canCreate ? `
              <button type="button" class="btn sm" data-action="open-new-version" data-args="${jsArg({ documentId: doc.id, entityType: ent.type, entityId: ent.id })}" title="Nouvelle version">
                ${icon('i-upload')}
              </button>
            ` : ''}
            <button type="button" class="btn sm" data-action="open-document-history" data-args="${jsArg({ documentId: doc.id, entityType: ent.type, entityId: ent.id })}" title="Historique">
              ${icon('i-history')}
            </button>
            ${canCreate ? `
              <button type="button" class="btn sm" data-action="open-edit-document" data-args="${jsArg({ documentId: doc.id, entityType: ent.type, entityId: ent.id })}" title="Modifier">
                ${icon('i-edit')}
              </button>
            ` : ''}
            ${canDelete ? `
              <button type="button" class="btn sm danger" data-action="delete-document" data-args="${jsArg({ documentId: doc.id, entityType: ent.type, entityId: ent.id })}" title="Supprimer">
                ${icon('i-trash')}
              </button>
            ` : ''}
          </div>
        </td>
      </tr>
    `;
  }).join('');

  container.innerHTML = `
    <div class="table-container">
      <table class="data-table">
        <thead>
          <tr>
            <th>Document</th>
            <th>Entité liée</th>
            <th>Catégorie</th>
            <th>Version & Taille</th>
            <th>Auteur & Date</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          ${rows}
        </tbody>
      </table>
    </div>
    ${renderLoadMore(state.documentsNextCursor, 'load-more-documents', state.documentsLoading)}
  `;
}

async function loadMoreDocuments() {
  if (!state.documentsNextCursor || state.documentsLoading) return;
  await loadDocumentsList(false);
}

function labelsPrintPage() {
  return `
    <div class="labels-page-wrap">
      <div class="labels-toolbar no-print">
        <div class="labels-toolbar-left">
          <a href="#/equipements" class="btn">
            ${icon('i-arrow-left')} <span>Retour aux équipements</span>
          </a>
          <h2>${icon('i-qrcode')} Impression d'étiquettes QR Code</h2>
        </div>
        <div class="labels-toolbar-right">
          <div class="labels-offset-ctrl" id="labelsOffsetBox">
            <label for="labelsOffsetInput">Commencer à la position n° :</label>
            <input type="number" id="labelsOffsetInput" min="1" max="100" value="1" data-action="change-labels-offset" />
          </div>
          <button type="button" class="btn primary" data-action="trigger-print">
            ${icon('i-print')} <span>Imprimer</span>
          </button>
          ${hasPermission('settings.edit') ? `
            <a href="#/parametres" class="btn" title="Modifier le format d'étiquette">
              ${icon('i-gear')} <span>Format</span>
            </a>
          ` : ''}
        </div>
      </div>

      <div id="labelsPrintContainer">
        <p class="hint">${icon('i-clock')} Chargement du format et des équipements…</p>
      </div>
    </div>
  `;
}

async function initLabelsPrintPageView() {
  const container = document.getElementById('labelsPrintContainer');
  if (!container) return;

  const { query } = currentRouteInfo();
  let ids = [];
  if (query) {
    const params = new URLSearchParams(query);
    const idsParam = params.get('ids');
    if (idsParam) {
      ids = idsParam.split(',').map(s => s.trim()).filter(Boolean);
    }
  }

  if (!ids.length && state.selectedEquipmentIds.size > 0) {
    ids = Array.from(state.selectedEquipmentIds);
  }

  if (!ids.length) {
    container.innerHTML = `
      <div class="card">
        <p class="hint">Aucun équipement sélectionné pour l'impression.</p>
        <p><a href="#/equipements" class="btn primary">${icon('i-gear')} <span>Sélectionner des équipements</span></a></p>
      </div>
    `;
    return;
  }

  try {
    const [fmt, ...equipments] = await Promise.all([
      api('/api/settings/labels').catch(() => ({
        mode: 'sheet', page: 'A4', columns: 3, rows: 8, margin_top_mm: 10, margin_left_mm: 7, spacing_h_mm: 2, spacing_v_mm: 0,
        width_mm: 64, height_mm: 34, qr_size_mm: 20, show_code: true, show_name: true, show_site: false, show_line: false
      })),
      ...ids.map(id => api(`/api/equipments/${encodeURIComponent(id)}`).catch(() => null))
    ]);

    state.labelSettings = labelFromServer(fmt);
    state.currentPrintEquipments = equipments.filter(Boolean);
    state.currentPrintOffset = 1;

    renderLabelsSheet();
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur : ${esc(err.message)}</p>`;
  }
}

function renderLabelsSheet() {
  const container = document.getElementById('labelsPrintContainer');
  if (!container) return;

  const fmt = state.labelSettings || {};
  const equips = state.currentPrintEquipments || [];
  const offset = Math.max(1, parseInt(state.currentPrintOffset || 1, 10));

  const offsetBox = document.getElementById('labelsOffsetBox');
  if (offsetBox) {
    offsetBox.hidden = fmt.mode !== 'sheet';
  }

  if (fmt.mode === 'single') {
    container.innerHTML = `
      <div class="labels-single-container">
        ${equips.map(eq => renderStickerHtml(eq, fmt)).join('')}
      </div>
    `;
    const singleWrap = container.querySelector('.labels-single-container');
    if (singleWrap) {
      singleWrap.style.setProperty('--label-w', `${fmt.width_mm || 64}mm`);
      singleWrap.style.setProperty('--label-h', `${fmt.height_mm || 34}mm`);
    }
    container.querySelectorAll('.label-sticker').forEach(stk => {
      stk.style.setProperty('--qr-size', `${fmt.qr_size_mm || 20}mm`);
    });
    return;
  }

  const cols = fmt.columns || 3;
  const rows = fmt.rows || 8;
  const stickersPerPage = cols * rows;

  const cells = [];
  for (let i = 1; i < offset; i++) {
    cells.push('<div class="label-sticker sticker-empty"></div>');
  }
  for (const eq of equips) {
    cells.push(renderStickerHtml(eq, fmt));
  }

  const pagesHtml = [];
  for (let i = 0; i < cells.length; i += stickersPerPage) {
    const pageCells = cells.slice(i, i + stickersPerPage);
    pagesHtml.push(`
      <div class="label-sheet-page label-page-${esc(fmt.page || 'A4').toLowerCase()}">
        <div class="label-sheet-grid">
          ${pageCells.join('')}
        </div>
      </div>
    `);
  }

  container.innerHTML = pagesHtml.join('');

  container.querySelectorAll('.label-sheet-page').forEach(page => {
    page.style.setProperty('--sheet-cols', cols);
    page.style.setProperty('--sheet-rows', rows);
    page.style.setProperty('--margin-top', `${fmt.margin_top_mm || 10}mm`);
    page.style.setProperty('--margin-left', `${fmt.margin_left_mm || 7}mm`);
    page.style.setProperty('--spacing-h', `${fmt.spacing_h_mm || 2}mm`);
    page.style.setProperty('--spacing-v', `${fmt.spacing_v_mm || 0}mm`);
    page.style.setProperty('--sticker-w', `${fmt.width_mm || 64}mm`);
    page.style.setProperty('--sticker-h', `${fmt.height_mm || 34}mm`);
  });

  container.querySelectorAll('.label-sticker').forEach(stk => {
    stk.style.setProperty('--qr-size', `${fmt.qr_size_mm || 20}mm`);
  });
}

function renderStickerHtml(eq, fmt) {
  const lineLabel = eq.production_line ? `${eq.production_line.code}` : '';
  const siteLabel = eq.site ? `${eq.site.name}` : (state.site ? state.site.name : '');
  const qrSize = fmt.qr_size_mm || 20;

  return `
    <div class="label-sticker">
      <div class="sticker-qr">
        <img src="/api/equipments/${esc(eq.id)}/qr.svg" alt="QR Code" width="${qrSize * 4}" height="${qrSize * 4}" />
      </div>
      <div class="sticker-info">
        ${fmt.show_code ? `<div class="sticker-code mono">${esc(eq.code)}</div>` : ''}
        ${fmt.show_name ? `<div class="sticker-name">${esc(eq.name)}</div>` : ''}
        ${fmt.show_line && lineLabel ? `<div class="sticker-line hint mono">${esc(lineLabel)}</div>` : ''}
        ${fmt.show_site && siteLabel ? `<div class="sticker-site hint">${esc(siteLabel)}</div>` : ''}
      </div>
    </div>
  `;
}

function changeLabelsOffset(val) {
  const num = parseInt(val, 10);
  if (!isNaN(num) && num >= 1) {
    state.currentPrintOffset = num;
    renderLabelsSheet();
  }
}

// ============================================================================
// 14I. MODULE PARAMÈTRES (PHASE 6)
// ============================================================================

function settingsPage() {
  if (!hasPermission('settings.view')) {
    return accessDeniedPage();
  }

  const tab = state.settingsTab || 'general';

  return `
    <div class="page-head">
      <div class="page-head-title">
        ${icon('i-cog')}
        <div>
          <h1>Paramètres du système</h1>
          <p class="page-subtitle">Configuration globale, sécurité, serveur e-mail, alertes, stockage et dispositions.</p>
        </div>
      </div>
    </div>

    <div class="tabs-nav settings-tabs-nav">
      <button type="button" class="tab-btn ${tab === 'general' ? 'on' : ''}" data-action="switch-settings-tab" data-args="general">
        ${icon('i-cog')} <span>Général</span>
      </button>
      <button type="button" class="tab-btn ${tab === 'security' ? 'on' : ''}" data-action="switch-settings-tab" data-args="security">
        ${icon('i-shield')} <span>Sécurité</span>
      </button>
      <button type="button" class="tab-btn ${tab === 'stock' ? 'on' : ''}" data-action="switch-settings-tab" data-args="stock">
        ${icon('i-box')} <span>Stock</span>
      </button>
      <button type="button" class="tab-btn ${tab === 'smtp' ? 'on' : ''}" data-action="switch-settings-tab" data-args="smtp">
        ${icon('i-mail')} <span>E-mail & SMTP</span>
      </button>
      <button type="button" class="tab-btn ${tab === 'alerts' ? 'on' : ''}" data-action="switch-settings-tab" data-args="alerts">
        ${icon('i-bell')} <span>Alertes par défaut</span>
      </button>
      <button type="button" class="tab-btn ${tab === 'audit' ? 'on' : ''}" data-action="switch-settings-tab" data-args="audit">
        ${icon('i-audit')} <span>Audit</span>
      </button>
      <button type="button" class="tab-btn ${tab === 'storage' ? 'on' : ''}" data-action="switch-settings-tab" data-args="storage">
        ${icon('i-database')} <span>Stockage</span>
      </button>
      <button type="button" class="tab-btn ${tab === 'documents' ? 'on' : ''}" data-action="switch-settings-tab" data-args="documents">
        ${icon('i-folder')} <span>Types de fichiers</span>
      </button>
      <button type="button" class="tab-btn ${tab === 'labels' ? 'on' : ''}" data-action="switch-settings-tab" data-args="labels">
        ${icon('i-qrcode')} <span>Étiquettes</span>
      </button>
      <button type="button" class="tab-btn ${tab === 'dashboard' ? 'on' : ''}" data-action="switch-settings-tab" data-args="dashboard">
        ${icon('i-dash')} <span>Tableau de bord</span>
      </button>
    </div>

    <div id="settingsTabContent">
      <p class="hint">${icon('i-clock')} Chargement des paramètres…</p>
    </div>
  `;
}

async function initSettingsPageView() {
  const tab = state.settingsTab || 'general';
  await switchSettingsTab(tab);
}

async function switchSettingsTab(tab) {
  state.settingsTab = tab;

  document.querySelectorAll('.settings-tabs-nav .tab-btn').forEach(btn => {
    btn.classList.toggle('on', btn.dataset.args === tab);
  });

  const container = document.getElementById('settingsTabContent');
  if (!container) return;

  if (tab === 'general') {
    await loadSettingsGeneral();
  } else if (tab === 'security') {
    await loadSettingsSecurity();
  } else if (tab === 'stock') {
    await loadSettingsStock();
  } else if (tab === 'smtp') {
    await loadSettingsSmtp();
  } else if (tab === 'alerts') {
    await loadSettingsAlerts();
  } else if (tab === 'audit') {
    await loadSettingsAudit();
  } else if (tab === 'storage') {
    await loadSettingsStorage();
  } else if (tab === 'documents') {
    await loadSettingsDocuments();
  } else if (tab === 'labels') {
    await loadSettingsLabels();
  } else if (tab === 'dashboard') {
    await loadSettingsDashboardDefaults(state.settingsDashboardRole || 'responsable');
  }
}

/** Stockage renvoyé par le serveur (kind, label, is_active) vers les noms utilisés par l'écran. */
function adaptStorage(b) {
  return {
    ...b,
    driver: b.kind || b.driver,
    name: b.label || b.name || '',
    active: b.is_active !== undefined ? !!b.is_active : !!b.active,
  };
}

async function loadSettingsStorage() {
  const container = document.getElementById('settingsTabContent');
  if (!container) return;

  state.storageLoading = true;
  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement des stockages…</p>`;

  try {
    const list = await api('/api/settings/storage');
    state.storageBackends = (Array.isArray(list) ? list : (list && Array.isArray(list.items) ? list.items : [])).map(adaptStorage);
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur lors du chargement des stockages : ${esc(err.message)}</p>`;
    state.storageLoading = false;
    return;
  }

  state.storageLoading = false;
  renderSettingsStorageView();
}

function renderSettingsStorageView() {
  const container = document.getElementById('settingsTabContent');
  if (!container) return;

  const canEdit = hasPermission('settings.edit');

  const rows = state.storageBackends.map(b => {
    const isFolder = b.driver === 'folder';
    const driverLabel = isFolder ? 'Dossier local / UNC' : 'Stockage objet S3';
    const cfg = b.config || {};
    const pathInfo = isFolder
      ? (cfg.path ? cfg.path : 'Dossier par défaut du serveur')
      : `${cfg.bucket || ''} (${cfg.endpoint || ''})`;

    return `
      <tr>
        <td>
          <b>${esc(b.name)}</b>
          ${b.active ? ' <span class="badge badge-active-on">Actif</span>' : '<span class="badge badge-active-off">Inactif</span>'}
        </td>
        <td>
          <span class="badge ${isFolder ? 'badge-info' : 'badge-subtle'}">${esc(driverLabel)}</span>
        </td>
        <td class="mono hint">${esc(pathInfo)}</td>
        <td>
          <div class="cell-actions">
            <button type="button" class="btn sm" data-action="test-storage" data-args="${jsArg(b.id)}" title="Vérifier l'accessibilité en écriture et lecture">
              ${icon('i-check')} <span>Tester la connexion</span>
            </button>
            ${canEdit && !b.active ? `
              <button type="button" class="btn sm primary" data-action="activate-storage" data-args="${jsArg(b.id)}" title="Définir comme stockage actif pour les nouveaux fichiers">
                ${icon('i-check')} <span>Activer</span>
              </button>
            ` : ''}
            ${canEdit ? `
              <button type="button" class="btn sm" data-action="open-edit-storage" data-args="${jsArg(b.id)}" title="Modifier">
                ${icon('i-edit')}
              </button>
              <button type="button" class="btn sm danger" data-action="delete-storage" data-args="${jsArg(b.id)}" title="Supprimer">
                ${icon('i-trash')}
              </button>
            ` : ''}
          </div>
        </td>
      </tr>
    `;
  }).join('');

  container.innerHTML = `
    <div class="card">
      <div class="detail-nav-bar">
        <div>
          <h2>${icon('i-database')} Emplacements de stockage des documents</h2>
          <p class="hint">Apex Maint prend en charge le stockage sur système de fichiers local / partage réseau UNC, ou sur serveur S3 (AWS, MinIO, Scaleway, Ceph).</p>
        </div>
        ${canEdit ? `
          <button type="button" class="btn primary" data-action="open-create-storage">
            ${icon('i-plus')} <span>Ajouter un stockage</span>
          </button>
        ` : ''}
      </div>

      <div class="table-container">
        <table class="data-table">
          <thead>
            <tr>
              <th>Nom du stockage</th>
              <th>Pilote</th>
              <th>Chemin / Bucket</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            ${rows.length ? rows : '<tr><td colspan="4" class="empty-cell">Aucun stockage configuré.</td></tr>'}
          </tbody>
        </table>
      </div>
    </div>
  `;
}

async function testStorageConnection(storageId) {
  try {
    const res = await api(`/api/settings/storage/${encodeURIComponent(storageId)}/test`, { method: 'POST' });
    if (res && res.ok) {
      notify('Test de connexion réussi : écriture et lecture validées.', 'success');
    } else {
      notify(`Échec du test de connexion : ${(res && res.message) || 'Inaccessible'}`, 'error');
    }
  } catch (err) {
    notify(`Échec du test de connexion : ${err.message}`, 'error');
  }
}

async function activateStorage(storageId) {
  try {
    await api(`/api/settings/storage/${encodeURIComponent(storageId)}/activate`, { method: 'POST' });
    notify('Stockage activé avec succès pour les prochains téléversements.', 'success');
    await loadSettingsStorage();
  } catch (err) {
    notify(`Erreur lors de l'activation : ${err.message}`, 'error');
  }
}

async function deleteStorage(storageId) {
  const ok = await confirmDialog(
    'Supprimer le stockage',
    'Voulez-vous supprimer ce stockage ? S\'il contient des documents référencés, la suppression sera refusée.',
    'Supprimer',
    true
  );
  if (!ok) return;

  try {
    await api(`/api/settings/storage/${encodeURIComponent(storageId)}`, { method: 'DELETE' });
    notify('Stockage supprimé avec succès.', 'success');
    await loadSettingsStorage();
  } catch (err) {
    notify(`Suppression impossible : ${err.message}`, 'error');
  }
}

function openCreateStorageModal() {
  const bodyHtml = `
    <form id="createStorageForm" class="modal-form">
      <div class="form-group">
        <label for="storageNameInput">Nom du stockage *</label>
        <input type="text" id="storageNameInput" name="name" required placeholder="ex. Serveur NAS Ateliers ou S3 MinIO" />
      </div>

      <div class="form-group">
        <label for="storageDriverSelect">Pilote de stockage *</label>
        <select id="storageDriverSelect" name="driver" data-action="toggle-storage-driver-fields">
          <option value="folder" selected>Dossier local ou partage UNC (folder)</option>
          <option value="s3">Stockage objet compatible S3 (s3)</option>
        </select>
      </div>

      <!-- CHAMPS PILOTE FOLDER -->
      <div id="storageFolderFields" class="storage-driver-section">
        <div class="form-group">
          <label for="storageFolderPath">Chemin absolu du dossier</label>
          <input type="text" id="storageFolderPath" name="path" placeholder="ex. D:\\GMAO\\fichiers ou \\\\nas\\partage\\gmao (laisser vide pour le dossier par défaut)" />
          <span class="form-help">Si vide, le dossier par défaut configuré au démarrage du serveur sera utilisé.</span>
        </div>
      </div>

      <!-- CHAMPS PILOTE S3 -->
      <div id="storageS3Fields" class="storage-driver-section" hidden>
        <div class="form-row">
          <div class="form-group">
            <label for="s3Endpoint">Point de terminaison (Endpoint) *</label>
            <input type="text" id="s3Endpoint" name="endpoint" placeholder="ex. https://s3.eu-west-1.amazonaws.com ou http://minio:9000" />
          </div>
          <div class="form-group">
            <label for="s3Region">Région</label>
            <input type="text" id="s3Region" name="region" placeholder="ex. eu-west-1 ou us-east-1" value="eu-west-1" />
          </div>
        </div>
        <div class="form-row">
          <div class="form-group">
            <label for="s3Bucket">Nom du Bucket *</label>
            <input type="text" id="s3Bucket" name="bucket" placeholder="ex. apex-maint-documents" />
          </div>
          <div class="form-group">
            <label for="s3Prefix">Préfixe de dossier (optionnel)</label>
            <input type="text" id="s3Prefix" name="prefix" placeholder="ex. production/" />
          </div>
        </div>
        <div class="form-row">
          <div class="form-group">
            <label for="s3AccessKey">Clé d'accès (Access Key ID) *</label>
            <input type="text" id="s3AccessKey" name="access_key_id" autocomplete="off" />
          </div>
          <div class="form-group">
            <label for="s3SecretKey">Clé secrète (Secret Access Key) *</label>
            <input type="password" id="s3SecretKey" name="secret" autocomplete="new-password" />
            <span class="form-help">Chiffrée de manière sécurisée en AES-256-GCM.</span>
          </div>
        </div>
        <div class="form-group">
          <label class="checkbox-label">
            <input type="checkbox" id="s3PathStyle" name="path_style" />
            <span>Activer le mode Path Style (recommandé pour MinIO ou Ceph)</span>
          </label>
        </div>
      </div>

      <div id="createStorageError" class="form-error" hidden></div>
    </form>
  `;

  const footerHtml = `
    <button type="button" class="btn" data-action="close-modal">Annuler</button>
    <button type="submit" form="createStorageForm" class="btn primary">
      ${icon('i-check')} <span>Créer le stockage</span>
    </button>
  `;

  openModal('Nouveau stockage de documents', bodyHtml, footerHtml);
}

async function submitCreateStorage(form) {
  const errBox = document.getElementById('createStorageError');
  if (errBox) errBox.hidden = true;

  const driver = form.driver.value;
  const name = form.name.value.trim();

  let config = {};
  let secret = null;

  if (driver === 'folder') {
    config = { path: form.path ? form.path.value.trim() : '' };
  } else if (driver === 's3') {
    config = {
      endpoint: form.endpoint.value.trim(),
      region: form.region.value.trim() || 'eu-west-1',
      bucket: form.bucket.value.trim(),
      access_key_id: form.access_key_id.value.trim(),
      prefix: form.prefix ? form.prefix.value.trim() : '',
      path_style: form.path_style ? form.path_style.checked : false
    };
    secret = form.secret.value;
    if (!config.endpoint || !config.bucket || !config.access_key_id || !secret) {
      if (errBox) {
        errBox.textContent = 'Tous les champs S3 obligatoires doivent être renseignés.';
        errBox.hidden = false;
      }
      return;
    }
  }

  const payload = { label: name, kind: driver, config };
  if (secret) payload.secret = secret;

  try {
    await api('/api/settings/storage', {
      method: 'POST',
      body: payload
    });

    closeModal();
    notify('Stockage ajouté avec succès.', 'success');
    await loadSettingsStorage();
  } catch (err) {
    if (errBox) {
      errBox.textContent = err.message || 'Erreur lors de la création du stockage.';
      errBox.hidden = false;
    }
  }
}

async function openEditStorageModal(storageId) {
  openModal('Modifier le stockage', `<p class="hint">${icon('i-clock')} Chargement du stockage…</p>`);

  try {
    const list = await api('/api/settings/storage');
    const items = Array.isArray(list) ? list : (list && Array.isArray(list.items) ? list.items : []);
    const found = items.find(item => item.id === storageId);
    if (!found) throw new Error('Stockage introuvable.');
    const b = adaptStorage(found);

    const isFolder = b.driver === 'folder';
    const cfg = b.config || {};

    const bodyHtml = `
      <form id="editStorageForm" class="modal-form">
        <input type="hidden" name="storage_id" value="${esc(b.id)}" />
        <input type="hidden" name="driver" value="${esc(b.driver)}" />

        <div class="form-group">
          <label for="editStorageName">Nom du stockage *</label>
          <input type="text" id="editStorageName" name="name" required value="${esc(b.name)}" />
        </div>

        ${isFolder ? `
          <div class="form-group">
            <label for="editStoragePath">Chemin du dossier</label>
            <input type="text" id="editStoragePath" name="path" value="${esc(cfg.path || '')}" placeholder="Laisser vide pour le dossier par défaut" />
          </div>
        ` : `
          <div class="form-row">
            <div class="form-group">
              <label for="editS3Endpoint">Point de terminaison (Endpoint) *</label>
              <input type="text" id="editS3Endpoint" name="endpoint" required value="${esc(cfg.endpoint || '')}" />
            </div>
            <div class="form-group">
              <label for="editS3Region">Région</label>
              <input type="text" id="editS3Region" name="region" value="${esc(cfg.region || 'eu-west-1')}" />
            </div>
          </div>
          <div class="form-row">
            <div class="form-group">
              <label for="editS3Bucket">Nom du Bucket *</label>
              <input type="text" id="editS3Bucket" name="bucket" required value="${esc(cfg.bucket || '')}" />
            </div>
            <div class="form-group">
              <label for="editS3Prefix">Préfixe</label>
              <input type="text" id="editS3Prefix" name="prefix" value="${esc(cfg.prefix || '')}" />
            </div>
          </div>
          <div class="form-row">
            <div class="form-group">
              <label for="editS3AccessKey">Clé d'accès *</label>
              <input type="text" id="editS3AccessKey" name="access_key_id" required value="${esc(cfg.access_key_id || '')}" />
            </div>
            <div class="form-group">
              <label for="editS3Secret">Nouvelle clé secrète (optionnel)</label>
              <input type="password" id="editS3Secret" name="secret" placeholder="${b.has_secret ? 'Laisser vide pour conserver le secret existant' : 'Saisir le secret'}" />
            </div>
          </div>
          <div class="form-group">
            <label class="checkbox-label">
              <input type="checkbox" id="editS3PathStyle" name="path_style" ${cfg.path_style ? 'checked' : ''} />
              <span>Mode Path Style (MinIO / Ceph)</span>
            </label>
          </div>
        `}

        <div id="editStorageError" class="form-error" hidden></div>
      </form>
    `;

    const footerHtml = `
      <button type="button" class="btn" data-action="close-modal">Annuler</button>
      <button type="submit" form="editStorageForm" class="btn primary">
        ${icon('i-check')} <span>Enregistrer les modifications</span>
      </button>
    `;

    openModal(`Modifier « ${b.name} »`, bodyHtml, footerHtml);
  } catch (err) {
    openModal('Erreur', `<p class="hint state-ko">${esc(err.message)}</p>`, `<button type="button" class="btn" data-action="close-modal">Fermer</button>`);
  }
}

async function submitEditStorage(form) {
  const errBox = document.getElementById('editStorageError');
  if (errBox) errBox.hidden = true;

  const storageId = form.storage_id.value;
  const driver = form.driver.value;
  const name = form.name.value.trim();

  let config = {};
  let secret = null;

  if (driver === 'folder') {
    config = { path: form.path ? form.path.value.trim() : '' };
  } else if (driver === 's3') {
    config = {
      endpoint: form.endpoint.value.trim(),
      region: form.region.value.trim() || 'eu-west-1',
      bucket: form.bucket.value.trim(),
      access_key_id: form.access_key_id.value.trim(),
      prefix: form.prefix ? form.prefix.value.trim() : '',
      path_style: form.path_style ? form.path_style.checked : false
    };
    if (form.secret && form.secret.value) {
      secret = form.secret.value;
    }
  }

  const payload = { label: name, config };
  if (secret) payload.secret = secret;

  try {
    await api(`/api/settings/storage/${encodeURIComponent(storageId)}`, {
      method: 'PATCH',
      body: payload
    });

    closeModal();
    notify('Stockage mis à jour avec succès.', 'success');
    await loadSettingsStorage();
  } catch (err) {
    if (errBox) {
      errBox.textContent = err.message || 'Erreur lors de la modification.';
      errBox.hidden = false;
    }
  }
}

async function loadSettingsDocuments() {
  const container = document.getElementById('settingsTabContent');
  if (!container) return;

  state.documentSettingsLoading = true;
  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement des types de fichiers…</p>`;

  try {
    state.documentSettings = await api('/api/settings/documents');
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur : ${esc(err.message)}</p>`;
    state.documentSettingsLoading = false;
    return;
  }

  state.documentSettingsLoading = false;
  renderSettingsDocumentsView();
}

function renderSettingsDocumentsView() {
  const container = document.getElementById('settingsTabContent');
  if (!container) return;

  const docSettings = state.documentSettings || {};
  const allowed = Array.isArray(docSettings.allowed_types) ? docSettings.allowed_types : [];
  const catalog = Array.isArray(docSettings.catalog) ? docSettings.catalog : [];
  const maxSize = docSettings.max_size_mb || 50;
  const canEdit = hasPermission('settings.edit');

  const catalogItemsHtml = catalog.map(cat => {
    const isChecked = allowed.includes(cat.id);
    const exts = Array.isArray(cat.extensions) ? cat.extensions.join(', ') : cat.extensions;
    return `
      <label class="catalog-item-card">
        <input type="checkbox" name="allowed_types" value="${esc(cat.id)}" ${isChecked ? 'checked' : ''} ${!canEdit ? 'disabled' : ''} />
        <div class="catalog-item-info">
          <div class="catalog-item-title">${esc(cat.label)}</div>
          <div class="catalog-item-ext mono hint">${esc(exts)}</div>
          <div class="catalog-item-disp hint">${cat.disposition === 'inline' ? 'Consultable dans le navigateur' : 'Téléchargement direct'}</div>
        </div>
      </label>
    `;
  }).join('');

  container.innerHTML = `
    <div class="card">
      <form id="settingsDocumentsForm">
        <div class="detail-nav-bar">
          <div>
            <h2>${icon('i-folder')} Formats de fichiers et quotas de téléversement</h2>
            <p class="hint">Activez les types de documents acceptés par l'application et fixez la limite maximale de taille autorisée.</p>
          </div>
          ${canEdit ? `
            <button type="submit" class="btn primary">
              ${icon('i-check')} <span>Enregistrer les réglages</span>
            </button>
          ` : ''}
        </div>

        <div class="form-row form-row-tight">
          <div class="form-group max-size-group">
            <label for="docMaxSizeInput">Taille maximale par fichier (en Mo) *</label>
            <div class="input-with-unit">
              <input type="number" id="docMaxSizeInput" name="max_size_mb" min="1" max="200" required value="${esc(maxSize)}" ${!canEdit ? 'disabled' : ''} />
              <span class="unit-addon">Mo</span>
            </div>
            <span class="form-help">Valeur comprise entre 1 et 200 Mo (par défaut 50 Mo).</span>
          </div>
        </div>

        <div class="catalog-section">
          <h3>Catalogue des types de fichiers pris en charge (${catalog.length})</h3>
          <div class="catalog-grid">
            ${catalogItemsHtml}
          </div>
        </div>

        <div id="settingsDocumentsError" class="form-error" hidden></div>
      </form>
    </div>
  `;
}

async function submitSettingsDocuments(form) {
  const errBox = document.getElementById('settingsDocumentsError');
  if (errBox) errBox.hidden = true;

  const max_size_mb = parseInt(form.max_size_mb.value, 10);
  if (isNaN(max_size_mb) || max_size_mb < 1 || max_size_mb > 200) {
    if (errBox) {
      errBox.textContent = 'La taille maximale doit être un nombre entier entre 1 et 200 Mo.';
      errBox.hidden = false;
    }
    return;
  }

  const allowed_types = Array.from(form.querySelectorAll('input[name="allowed_types"]:checked')).map(cb => cb.value);
  if (!allowed_types.length) {
    if (errBox) {
      errBox.textContent = 'Vous devez autoriser au moins un type de fichier.';
      errBox.hidden = false;
    }
    return;
  }

  try {
    const res = await api('/api/settings/documents', {
      method: 'PUT',
      body: { allowed_types, max_size_mb }
    });
    state.documentSettings = res;
    notify('Paramètres de documents enregistrés avec succès.', 'success');
  } catch (err) {
    if (errBox) {
      errBox.textContent = err.message || 'Erreur lors de la sauvegarde.';
      errBox.hidden = false;
    }
  }
}

async function loadSettingsLabels() {
  const container = document.getElementById('settingsTabContent');
  if (!container) return;

  state.labelSettingsLoading = true;
  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement des paramètres d'étiquettes…</p>`;

  try {
    state.labelSettings = labelFromServer(await api('/api/settings/labels'));
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur : ${esc(err.message)}</p>`;
    state.labelSettingsLoading = false;
    return;
  }

  state.labelSettingsLoading = false;
  renderSettingsLabelsView();
}

function renderSettingsLabelsView() {
  const container = document.getElementById('settingsTabContent');
  if (!container) return;

  const fmt = state.labelSettings || {
    mode: 'sheet', page: 'A4', columns: 3, rows: 8, margin_top_mm: 10, margin_left_mm: 7, spacing_h_mm: 2, spacing_v_mm: 0,
    width_mm: 64, height_mm: 34, qr_size_mm: 20, show_code: true, show_name: true, show_site: false, show_line: false
  };

  const isSheet = fmt.mode === 'sheet';
  const canEdit = hasPermission('settings.edit');

  container.innerHTML = `
    <div class="card">
      <div class="detail-nav-bar">
        <div>
          <h2>${icon('i-qrcode')} Format d'impression des étiquettes d'équipement</h2>
          <p class="hint">Configurez les dimensions des étiquettes pour planche adhésive ou rouleau d'imprimante thermique.</p>
        </div>
        ${canEdit ? `
          <button type="submit" form="settingsLabelsForm" class="btn primary">
            ${icon('i-check')} <span>Enregistrer le format</span>
          </button>
        ` : ''}
      </div>

      <div class="settings-labels-layout">
        <!-- FORMULAIRE -->
        <form id="settingsLabelsForm" class="settings-labels-form">
          <div class="form-group">
            <label for="labelModeSelect">Mode d'impression *</label>
            <select id="labelModeSelect" name="mode" ${!canEdit ? 'disabled' : ''}>
              <option value="sheet" ${isSheet ? 'selected' : ''}>Planche d'étiquettes (A4 / Letter)</option>
              <option value="single" ${!isSheet ? 'selected' : ''}>Étiquette unitaire (imprimante thermique)</option>
            </select>
          </div>

          <!-- REGLAGES PLANCHE -->
          <div id="sheetSpecificFields" ${!isSheet ? 'hidden' : ''}>
            <div class="form-row">
              <div class="form-group">
                <label for="labelPageSelect">Format de page</label>
                <select id="labelPageSelect" name="page" ${!canEdit ? 'disabled' : ''}>
                  <option value="A4" ${fmt.page === 'A4' ? 'selected' : ''}>A4 (210 × 297 mm)</option>
                  <option value="Letter" ${fmt.page === 'Letter' ? 'selected' : ''}>Letter (216 × 279 mm)</option>
                </select>
              </div>
              <div class="form-group">
                <label for="labelColsInput">Colonnes par page</label>
                <input type="number" id="labelColsInput" name="columns" min="1" max="10" value="${esc(fmt.columns || 3)}" ${!canEdit ? 'disabled' : ''} />
              </div>
              <div class="form-group">
                <label for="labelRowsInput">Lignes par page</label>
                <input type="number" id="labelRowsInput" name="rows" min="1" max="25" value="${esc(fmt.rows || 8)}" ${!canEdit ? 'disabled' : ''} />
              </div>
            </div>

            <div class="form-row">
              <div class="form-group">
                <label for="labelMarginTop">Marge haute (mm)</label>
                <input type="number" id="labelMarginTop" name="margin_top_mm" min="0" max="100" value="${esc(fmt.margin_top_mm || 10)}" ${!canEdit ? 'disabled' : ''} />
              </div>
              <div class="form-group">
                <label for="labelMarginLeft">Marge gauche (mm)</label>
                <input type="number" id="labelMarginLeft" name="margin_left_mm" min="0" max="100" value="${esc(fmt.margin_left_mm || 7)}" ${!canEdit ? 'disabled' : ''} />
              </div>
              <div class="form-group">
                <label for="labelSpacingH">Espacement horizontal (mm)</label>
                <input type="number" id="labelSpacingH" name="spacing_h_mm" min="0" max="50" value="${esc(fmt.spacing_h_mm || 2)}" ${!canEdit ? 'disabled' : ''} />
              </div>
              <div class="form-group">
                <label for="labelSpacingV">Espacement vertical (mm)</label>
                <input type="number" id="labelSpacingV" name="spacing_v_mm" min="0" max="50" value="${esc(fmt.spacing_v_mm || 0)}" ${!canEdit ? 'disabled' : ''} />
              </div>
            </div>
          </div>

          <!-- DIMENSIONS DE L'ÉTIQUETTE -->
          <h3>Dimensions de chaque étiquette</h3>
          ${isSheet ? '<p class="hint">En mode planche, la taille de chaque étiquette est calculée à partir du format de page, du nombre de colonnes et de lignes, des marges et des espacements.</p>' : ''}
          <div class="form-row">
            <div class="form-group">
              <label for="labelWidthMm">Largeur étiquette (mm) *</label>
              <input type="number" id="labelWidthMm" name="width_mm" min="10" max="500" step="0.1" required value="${esc(fmt.width_mm || 64)}" ${!canEdit ? 'disabled' : ''} ${isSheet ? 'readonly' : ''} />
            </div>
            <div class="form-group">
              <label for="labelHeightMm">Hauteur étiquette (mm) *</label>
              <input type="number" id="labelHeightMm" name="height_mm" min="10" max="500" step="0.1" required value="${esc(fmt.height_mm || 34)}" ${!canEdit ? 'disabled' : ''} ${isSheet ? 'readonly' : ''} />
            </div>
            <div class="form-group">
              <label for="labelQrSizeMm">Taille du QR Code (mm) *</label>
              <input type="number" id="labelQrSizeMm" name="qr_size_mm" min="10" max="100" step="0.1" required value="${esc(fmt.qr_size_mm || 20)}" ${!canEdit ? 'disabled' : ''} />
            </div>
          </div>

          <!-- ÉLÉMENTS AFFICHÉS -->
          <h3>Éléments textuels imprimés</h3>
          <div class="checkbox-grid">
            <label class="checkbox-label">
              <input type="checkbox" name="show_code" ${fmt.show_code ? 'checked' : ''} ${!canEdit ? 'disabled' : ''} />
              <span>Code équipement (ex. EXT-101)</span>
            </label>
            <label class="checkbox-label">
              <input type="checkbox" name="show_name" ${fmt.show_name ? 'checked' : ''} ${!canEdit ? 'disabled' : ''} />
              <span>Désignation de l'équipement</span>
            </label>
            <label class="checkbox-label">
              <input type="checkbox" name="show_site" ${fmt.show_site ? 'checked' : ''} ${!canEdit ? 'disabled' : ''} />
              <span>Nom du site</span>
            </label>
            <label class="checkbox-label">
              <input type="checkbox" name="show_line" ${fmt.show_line ? 'checked' : ''} ${!canEdit ? 'disabled' : ''} />
              <span>Ligne de production rattachée</span>
            </label>
          </div>

          <div id="settingsLabelsError" class="form-error" hidden></div>
        </form>

        <!-- APERÇU EN DIRECT -->
        <div class="settings-labels-preview">
          <h3>Aperçu en direct</h3>
          <p class="hint">Rendu réel d'une étiquette selon les paramètres saisis :</p>
          <div class="preview-sticker-container">
            <div id="labelLivePreviewSticker" class="label-sticker label-preview-sticker">
              <div class="sticker-qr">
                <svg width="80" height="80" viewBox="0 0 100 100" fill="currentColor">
                  <rect width="100" height="100" fill="var(--bg-surface)" />
                  <rect x="10" y="10" width="30" height="30" fill="currentColor" />
                  <rect x="60" y="10" width="30" height="30" fill="currentColor" />
                  <rect x="10" y="60" width="30" height="30" fill="currentColor" />
                  <rect x="20" y="20" width="10" height="10" fill="var(--bg-surface)" />
                  <rect x="70" y="20" width="10" height="10" fill="var(--bg-surface)" />
                  <rect x="20" y="70" width="10" height="10" fill="var(--bg-surface)" />
                  <rect x="45" y="45" width="10" height="10" fill="currentColor" />
                </svg>
              </div>
              <div class="sticker-info">
                <div class="sticker-code mono font-bold">EXT-101</div>
                <div class="sticker-name">Extrudeuse principale</div>
                <div class="sticker-line hint mono">LIGNE-A</div>
                <div class="sticker-site hint">Usine Principale</div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  `;

  updateLabelsPreviewLive();
}

function updateLabelsPreviewLive() {
  const form = document.getElementById('settingsLabelsForm');
  const preview = document.getElementById('labelLivePreviewSticker');
  if (!form || !preview) return;

  const mode = form.mode ? form.mode.value : 'sheet';
  const sheetFields = document.getElementById('sheetSpecificFields');
  if (sheetFields) {
    sheetFields.hidden = mode !== 'sheet';
  }

  const showCode = form.show_code ? form.show_code.checked : true;
  const showName = form.show_name ? form.show_name.checked : true;
  const showSite = form.show_site ? form.show_site.checked : false;
  const showLine = form.show_line ? form.show_line.checked : false;
  const qrSize = Math.max(10, Math.min(60, parseInt(form.qr_size_mm ? form.qr_size_mm.value : 20, 10)));
  const widthMm = Math.max(20, Math.min(200, parseInt(form.width_mm ? form.width_mm.value : 64, 10)));
  const heightMm = Math.max(15, Math.min(200, parseInt(form.height_mm ? form.height_mm.value : 34, 10)));

  preview.style.setProperty('--qr-size', `${qrSize}mm`);
  preview.style.setProperty('--label-w', `${widthMm}mm`);
  preview.style.setProperty('--label-h', `${heightMm}mm`);

  const codeEl = preview.querySelector('.sticker-code');
  const nameEl = preview.querySelector('.sticker-name');
  const siteEl = preview.querySelector('.sticker-site');
  const lineEl = preview.querySelector('.sticker-line');
  const qrSvg = preview.querySelector('.sticker-qr svg');

  if (codeEl) codeEl.hidden = !showCode;
  if (nameEl) nameEl.hidden = !showName;
  if (siteEl) siteEl.hidden = !showSite;
  if (lineEl) lineEl.hidden = !showLine;
  if (qrSvg) {
    qrSvg.setAttribute('width', qrSize * 4);
    qrSvg.setAttribute('height', qrSize * 4);
  }
}

// Format d'étiquette : le serveur imbrique la planche (sheet) et l'étiquette unitaire (single) ;
// l'écran travaille sur un objet à plat. En planche, la case est calculée (marges symétriques).
const LABEL_PAGES_MM = { A4: [210, 297], Letter: [215.9, 279.4] };

function labelFromServer(s) {
  if (!s || !s.sheet) return s;
  const sh = s.sheet;
  const single = s.single || { width_mm: 70, height_mm: 37 };
  const [pw, ph] = LABEL_PAGES_MM[sh.page] || LABEL_PAGES_MM.A4;
  const round = (n) => Math.round(n * 10) / 10;
  const cellW = (pw - 2 * sh.margin_left_mm - (sh.columns - 1) * sh.gap_horizontal_mm) / sh.columns;
  const cellH = (ph - 2 * sh.margin_top_mm - (sh.rows - 1) * sh.gap_vertical_mm) / sh.rows;
  const isSingle = s.mode === 'single';
  return {
    mode: s.mode,
    page: sh.page,
    columns: sh.columns,
    rows: sh.rows,
    margin_top_mm: sh.margin_top_mm,
    margin_left_mm: sh.margin_left_mm,
    spacing_h_mm: sh.gap_horizontal_mm,
    spacing_v_mm: sh.gap_vertical_mm,
    width_mm: isSingle ? single.width_mm : round(cellW),
    height_mm: isSingle ? single.height_mm : round(cellH),
    qr_size_mm: s.qr_size_mm,
    show_code: !!s.show_code,
    show_name: !!s.show_name,
    show_site: !!s.show_site,
    show_line: !!s.show_production_line,
    single,
  };
}

function labelToServer(f, previous) {
  const keptSingle = (previous && previous.single) || { width_mm: 70, height_mm: 37 };
  return {
    mode: f.mode,
    sheet: {
      page: f.page,
      columns: f.columns,
      rows: f.rows,
      margin_top_mm: f.margin_top_mm,
      margin_left_mm: f.margin_left_mm,
      gap_horizontal_mm: f.spacing_h_mm,
      gap_vertical_mm: f.spacing_v_mm,
    },
    single: f.mode === 'single' ? { width_mm: f.width_mm, height_mm: f.height_mm } : keptSingle,
    qr_size_mm: f.qr_size_mm,
    show_code: f.show_code,
    show_name: f.show_name,
    show_site: f.show_site,
    show_production_line: f.show_line,
  };
}

async function submitSettingsLabels(form) {
  const errBox = document.getElementById('settingsLabelsError');
  if (errBox) errBox.hidden = true;

  const mode = form.mode.value;
  const page = form.page ? form.page.value : 'A4';
  const columns = form.columns ? parseInt(form.columns.value, 10) : 3;
  const rows = form.rows ? parseInt(form.rows.value, 10) : 8;
  const margin_top_mm = form.margin_top_mm ? parseFloat(form.margin_top_mm.value.replace(',', '.')) : 10;
  const margin_left_mm = form.margin_left_mm ? parseFloat(form.margin_left_mm.value.replace(',', '.')) : 7;
  const spacing_h_mm = form.spacing_h_mm ? parseFloat(form.spacing_h_mm.value.replace(',', '.')) : 2;
  const spacing_v_mm = form.spacing_v_mm ? parseFloat(form.spacing_v_mm.value.replace(',', '.')) : 0;
  const width_mm = parseFloat(form.width_mm.value.replace(',', '.'));
  const height_mm = parseFloat(form.height_mm.value.replace(',', '.'));
  const qr_size_mm = parseFloat(form.qr_size_mm.value.replace(',', '.'));

  const show_code = form.show_code.checked;
  const show_name = form.show_name.checked;
  const show_site = form.show_site.checked;
  const show_line = form.show_line.checked;

  const payload = {
    mode,
    page,
    columns,
    rows,
    margin_top_mm,
    margin_left_mm,
    spacing_h_mm,
    spacing_v_mm,
    width_mm,
    height_mm,
    qr_size_mm,
    show_code,
    show_name,
    show_site,
    show_line
  };

  try {
    const res = await api('/api/settings/labels', {
      method: 'PUT',
      body: labelToServer(payload, state.labelSettings)
    });
    state.labelSettings = labelFromServer(res);
    notify('Format d\'étiquette enregistré avec succès.', 'success');
  } catch (err) {
    if (errBox) {
      errBox.textContent = err.message || 'Erreur lors de la sauvegarde du format.';
      errBox.hidden = false;
    }
  }
}

// ============================================================================
// 14J. MODULE PARAMÈTRES — SECTIONS PHASE 7
// ============================================================================

async function loadSettingsGeneral() {
  const container = document.getElementById('settingsTabContent');
  if (!container) return;

  const canEdit = hasPermission('settings.edit');
  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement des paramètres généraux…</p>`;

  try {
    const g = await api('/api/settings/general');
    state.settingsGeneral = g;

    container.innerHTML = `
      <div class="card settings-card">
        <h2>${icon('i-cog')} Identité de l'application</h2>
        <p class="hint form-help">Ces paramètres personnalisent l'en-tête de l'application, l'écran de connexion et les liens dans les e-mails.</p>

        <form class="form-grid" id="settingsGeneralForm" data-action="settings-general-submit">
          <div id="settingsGeneralErrorBox" class="form-error" hidden>
            ${icon('i-alert')}
            <span id="settingsGeneralErrorText"></span>
          </div>

          <div class="form-row">
            <div class="form-group">
              <label for="setAppName">Nom de l'application *</label>
              <input type="text" id="setAppName" name="app_name" value="${esc(g.app_name || '')}" required ${!canEdit ? 'disabled' : ''} />
            </div>
            <div class="form-group">
              <label for="setAppSubtitle">Sous-titre de l'application</label>
              <input type="text" id="setAppSubtitle" name="app_subtitle" value="${esc(g.app_subtitle || '')}" ${!canEdit ? 'disabled' : ''} />
            </div>
          </div>

          <div class="form-group">
            <label for="setPublicUrl">Adresse publique de l'IHM (URL de base)</label>
            <input type="url" id="setPublicUrl" name="public_base_url" value="${esc(g.public_base_url || '')}" placeholder="https://gmao.usine.lan" ${!canEdit ? 'disabled' : ''} />
            <span class="form-help">Utilisée pour forger les liens cliquables dans les notifications e-mail et réinitialisations de mot de passe.</span>
          </div>

          ${canEdit ? `
            <button type="submit" class="btn primary" id="settingsGeneralSubmitBtn">
              <span>Enregistrer les paramètres généraux</span>
            </button>
          ` : '<p class="hint">Vous disposez uniquement d\'un droit de consultation sur ces paramètres.</p>'}
        </form>
      </div>
    `;
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur : ${esc(err.message)}</p>`;
  }
}

async function handleSettingsGeneralSubmit(form) {
  const errorBox = document.getElementById('settingsGeneralErrorBox');
  const errorText = document.getElementById('settingsGeneralErrorText');
  const btn = document.getElementById('settingsGeneralSubmitBtn');

  const app_name = form.app_name.value.trim();
  const app_subtitle = form.app_subtitle.value.trim();
  const public_base_url = form.public_base_url.value.trim();

  if (btn) btn.disabled = true;
  if (errorBox) errorBox.hidden = true;

  try {
    const res = await api('/api/settings/general', {
      method: 'PUT',
      body: { app_name, app_subtitle, public_base_url },
    });
    state.settingsGeneral = res;
    if (state.branding) {
      state.branding.app_name = res.app_name;
      state.branding.app_subtitle = res.app_subtitle;
    }
    applyBranding();
    notify('Paramètres généraux enregistrés avec succès.', 'success');
  } catch (err) {
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = err.message || 'Impossible d\'enregistrer les paramètres généraux.';
    }
  } finally {
    if (btn) btn.disabled = false;
  }
}

async function loadSettingsSecurity() {
  const container = document.getElementById('settingsTabContent');
  if (!container) return;

  const canEdit = hasPermission('settings.edit');
  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement des paramètres de sécurité…</p>`;

  try {
    const s = await api('/api/settings/security');
    state.settingsSecurity = s;

    container.innerHTML = `
      <div class="card settings-card">
        <h2>${icon('i-shield')} Sécurité & Sessions utilisateurs</h2>
        <p class="hint form-help">Définissez la durée maximale d'inactivité avant déconnexion automatique des utilisateurs.</p>

        <form class="form-grid" id="settingsSecurityForm" data-action="settings-security-submit">
          <div id="settingsSecurityErrorBox" class="form-error" hidden>
            ${icon('i-alert')}
            <span id="settingsSecurityErrorText"></span>
          </div>

          <div class="form-group">
            <label for="setSessionTimeout">Délai d'inactivité avant expiration de session (en minutes) *</label>
            <input type="number" id="setSessionTimeout" name="session_timeout_min" min="15" max="10080" value="${esc(s.session_timeout_min || 480)}" required ${!canEdit ? 'disabled' : ''} />
            <span class="form-help">Entre 15 et 10 080 minutes (7 jours). S'applique aux nouvelles sessions et aux sessions existantes lors de leur prochaine action.</span>
          </div>

          ${canEdit ? `
            <button type="submit" class="btn primary" id="settingsSecuritySubmitBtn">
              <span>Enregistrer la sécurité</span>
            </button>
          ` : '<p class="hint">Consultation seule.</p>'}
        </form>
      </div>
    `;
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur : ${esc(err.message)}</p>`;
  }
}

async function handleSettingsSecuritySubmit(form) {
  const errorBox = document.getElementById('settingsSecurityErrorBox');
  const errorText = document.getElementById('settingsSecurityErrorText');
  const btn = document.getElementById('settingsSecuritySubmitBtn');

  const session_timeout_min = parseInt(form.session_timeout_min.value, 10);
  if (isNaN(session_timeout_min) || session_timeout_min < 15 || session_timeout_min > 10080) {
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = 'Le délai de session doit être compris entre 15 et 10 080 minutes.';
    }
    return;
  }

  if (btn) btn.disabled = true;
  if (errorBox) errorBox.hidden = true;

  try {
    const res = await api('/api/settings/security', {
      method: 'PUT',
      body: { session_timeout_min },
    });
    state.settingsSecurity = res;
    notify('Délai de session enregistré avec succès.', 'success');
  } catch (err) {
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = err.message || 'Impossible d\'enregistrer le délai de session.';
    }
  } finally {
    if (btn) btn.disabled = false;
  }
}

async function loadSettingsStock() {
  const container = document.getElementById('settingsTabContent');
  if (!container) return;

  const canEdit = hasPermission('settings.edit');
  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement des paramètres de stock…</p>`;

  try {
    const s = await api('/api/settings/stock');
    state.settingsStock = s;

    container.innerHTML = `
      <div class="card settings-card">
        <h2>${icon('i-box')} Paramètres globaux du magasin</h2>
        <p class="hint form-help">Règles et seuils appliqués par défaut à la gestion des pièces de rechange.</p>

        <form class="form-grid" id="settingsStockForm" data-action="settings-stock-submit">
          <div id="settingsStockErrorBox" class="form-error" hidden>
            ${icon('i-alert')}
            <span id="settingsStockErrorText"></span>
          </div>

          <div class="form-group">
            <label for="setStockAlertThresh">Seuil d'alerte global par défaut *</label>
            <input type="number" id="setStockAlertThresh" name="stock_alert_threshold" min="0" max="100000" value="${esc(s.stock_alert_threshold || 5)}" required ${!canEdit ? 'disabled' : ''} />
            <span class="form-help">Seuil utilisé lorsqu'une pièce n'a pas de seuil d'alerte spécifique défini.</span>
          </div>

          ${canEdit ? `
            <button type="submit" class="btn primary" id="settingsStockSubmitBtn">
              <span>Enregistrer les paramètres de stock</span>
            </button>
          ` : '<p class="hint">Consultation seule.</p>'}
        </form>
      </div>
    `;
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur : ${esc(err.message)}</p>`;
  }
}

async function handleSettingsStockSubmit(form) {
  const errorBox = document.getElementById('settingsStockErrorBox');
  const errorText = document.getElementById('settingsStockErrorText');
  const btn = document.getElementById('settingsStockSubmitBtn');

  const stock_alert_threshold = parseInt(form.stock_alert_threshold.value, 10);
  if (isNaN(stock_alert_threshold) || stock_alert_threshold < 0) {
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = 'Le seuil doit être un entier positif ou nul.';
    }
    return;
  }

  if (btn) btn.disabled = true;
  if (errorBox) errorBox.hidden = true;

  try {
    const res = await api('/api/settings/stock', {
      method: 'PUT',
      body: { stock_alert_threshold },
    });
    state.settingsStock = res;
    notify('Paramètres de stock enregistrés avec succès.', 'success');
  } catch (err) {
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = err.message || 'Impossible d\'enregistrer les paramètres de stock.';
    }
  } finally {
    if (btn) btn.disabled = false;
  }
}

async function loadSettingsSmtp() {
  const container = document.getElementById('settingsTabContent');
  if (!container) return;

  const canEdit = hasPermission('settings.edit');
  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement des paramètres SMTP et de la boîte d'envoi…</p>`;

  try {
    const [smtp, outboxRes] = await Promise.all([
      api('/api/settings/smtp'),
      api('/api/settings/smtp/outbox?limit=50'),
    ]);
    state.settingsSmtp = smtp;
    state.settingsOutbox = (outboxRes && Array.isArray(outboxRes.items)) ? outboxRes.items : [];

    const outboxRows = state.settingsOutbox.map(it => {
      let statusBadge = `<span class="badge badge-status-${esc(it.status)}">${esc(it.status)}</span>`;
      if (it.status === 'sent') statusBadge = '<span class="badge badge-state-ok">Envoyé</span>';
      else if (it.status === 'failed') statusBadge = '<span class="badge badge-state-ko">Échec</span>';
      else if (it.status === 'pending') statusBadge = '<span class="badge badge-state-warn">En attente</span>';

      return `
        <tr>
          <td class="mono font-bold">${esc(formatDate(it.created_at))}</td>
          <td>${esc(it.to_address)}</td>
          <td><span class="badge">${esc(NOTIF_EVENT_LABELS[it.event] || it.event)}</span></td>
          <td>${statusBadge}</td>
          <td class="mono">${it.attempts || 0}</td>
          <td class="hint text-danger">${esc(it.last_error || '—')}</td>
        </tr>
      `;
    }).join('');

    container.innerHTML = `
      <div class="card settings-card">
        <div class="card-head-between">
          <h2>${icon('i-mail')} Configuration du serveur sortant (SMTP)</h2>
          ${canEdit ? `
            <button type="button" class="btn" data-action="test-smtp-settings">
              ${icon('i-mail')} <span>Tester l'envoi SMTP</span>
            </button>
          ` : ''}
        </div>
        <p class="hint form-help">Permet l'envoi des alertes de stock, des rappels de préventif et de la réinitialisation de mot de passe.</p>

        <form class="form-grid" id="settingsSmtpForm" data-action="settings-smtp-submit">
          <div id="settingsSmtpErrorBox" class="form-error" hidden>
            ${icon('i-alert')}
            <span id="settingsSmtpErrorText"></span>
          </div>

          <label class="checkbox-label mb-2">
            <input type="checkbox" name="enabled" ${smtp.enabled ? 'checked' : ''} ${!canEdit ? 'disabled' : ''} />
            <span><b>Activer les notifications par e-mail (serveur SMTP actif)</b></span>
          </label>

          <div class="form-row">
            <div class="form-group">
              <label for="smtpHost">Serveur d'envoi SMTP *</label>
              <input type="text" id="smtpHost" name="host" value="${esc(smtp.host || '')}" placeholder="smtp.example.com" ${!canEdit ? 'disabled' : ''} />
            </div>
            <div class="form-group">
              <label for="smtpPort">Port SMTP *</label>
              <input type="number" id="smtpPort" name="port" value="${esc(smtp.port || 587)}" min="1" max="65535" ${!canEdit ? 'disabled' : ''} />
            </div>
            <div class="form-group">
              <label for="smtpSecurity">Sécurité de transport</label>
              <select id="smtpSecurity" name="security" ${!canEdit ? 'disabled' : ''}>
                <option value="starttls" ${smtp.security === 'starttls' ? 'selected' : ''}>STARTTLS (recommandé port 587)</option>
                <option value="tls" ${smtp.security === 'tls' ? 'selected' : ''}>TLS / SSL (port 465)</option>
                <option value="none" ${smtp.security === 'none' ? 'selected' : ''}>Aucune (non sécurisé)</option>
              </select>
            </div>
          </div>

          <div class="form-row">
            <div class="form-group">
              <label for="smtpUser">Identifiant SMTP</label>
              <input type="text" id="smtpUser" name="username" value="${esc(smtp.username || '')}" autocomplete="off" ${!canEdit ? 'disabled' : ''} />
            </div>
            <div class="form-group">
              <label for="smtpPass">Mot de passe SMTP</label>
              <input type="password" id="smtpPass" name="password" placeholder="${smtp.has_password ? '•••••••• (inchangé)' : 'Saisir un mot de passe'}" autocomplete="new-password" ${!canEdit ? 'disabled' : ''} />
              <span class="form-help">Champ en écriture seule. Laissez vide pour conserver le mot de passe actuel.</span>
            </div>
          </div>

          <div class="form-row">
            <div class="form-group">
              <label for="smtpFromName">Nom de l'expéditeur affiché</label>
              <input type="text" id="smtpFromName" name="from_name" value="${esc(smtp.from_name || 'Apex Maint')}" ${!canEdit ? 'disabled' : ''} />
            </div>
            <div class="form-group">
              <label for="smtpFromAddr">Adresse e-mail d'expédition *</label>
              <input type="email" id="smtpFromAddr" name="from_address" value="${esc(smtp.from_address || '')}" placeholder="noreply@usine.lan" ${!canEdit ? 'disabled' : ''} />
            </div>
          </div>

          ${canEdit ? `
            <button type="submit" class="btn primary" id="settingsSmtpSubmitBtn">
              <span>Enregistrer la configuration SMTP</span>
            </button>
          ` : '<p class="hint">Consultation seule.</p>'}
        </form>
      </div>

      <div class="card settings-card mt-4">
        <h2>${icon('i-mail')} Boîte d'envoi & Historique des e-mails (Outbox)</h2>
        <div class="table-container">
          <table class="data-table">
            <thead>
              <tr>
                <th>Date</th>
                <th>Destinataire</th>
                <th>Événement</th>
                <th>Statut</th>
                <th>Tentatives</th>
                <th>Dernière erreur</th>
              </tr>
            </thead>
            <tbody>
              ${outboxRows || '<tr><td colspan="6" class="empty-cell">Aucun e-mail dans la boîte d\'envoi.</td></tr>'}
            </tbody>
          </table>
        </div>
      </div>
    `;
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur : ${esc(err.message)}</p>`;
  }
}

async function handleSettingsSmtpSubmit(form) {
  const errorBox = document.getElementById('settingsSmtpErrorBox');
  const errorText = document.getElementById('settingsSmtpErrorText');
  const btn = document.getElementById('settingsSmtpSubmitBtn');

  const enabled = form.enabled.checked;
  const host = form.host.value.trim();
  const port = parseInt(form.port.value, 10);
  const security = form.security.value;
  const username = form.username.value.trim();
  const password = form.password.value;
  const from_name = form.from_name.value.trim();
  const from_address = form.from_address.value.trim();

  const payload = { enabled, host, port, security, username, from_name, from_address };
  if (password) {
    payload.password = password;
  }

  if (btn) btn.disabled = true;
  if (errorBox) errorBox.hidden = true;

  try {
    const res = await api('/api/settings/smtp', {
      method: 'PUT',
      body: payload,
    });
    state.settingsSmtp = res;
    notify('Configuration SMTP enregistrée avec succès.', 'success');
    await loadSettingsSmtp();
  } catch (err) {
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = err.message || 'Impossible d\'enregistrer la configuration SMTP.';
    }
  } finally {
    if (btn) btn.disabled = false;
  }
}

async function handleTestSmtp() {
  try {
    notify('Envoi de l\'e-mail de test en cours…', 'info');
    await api('/api/settings/smtp/test', { method: 'POST' });
    notify('E-mail de test envoyé à votre adresse avec succès.', 'success');
    await loadSettingsSmtp();
  } catch (err) {
    notify(`Échec de l'envoi de test : ${err.message}`, 'error');
  }
}

async function loadSettingsAlerts() {
  const container = document.getElementById('settingsTabContent');
  if (!container) return;

  const canEdit = hasPermission('settings.edit');
  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement des préférences d'alertes…</p>`;

  try {
    const al = await api('/api/settings/alerts');
    state.settingsAlerts = al;

    const roles = ['admin', 'responsable', 'technicien', 'production'];
    const events = [
      { key: 'stock_low', label: 'Stock bas' },
      { key: 'maintenance_generated', label: 'Nouveaux préventifs' },
      { key: 'maintenance_overdue', label: 'Maintenances en retard' },
      { key: 'intervention_critical', label: 'Interventions critiques' },
      { key: 'intervention_assigned', label: 'Interventions assignées' },
    ];

    const defaults = al.defaults || {};

    const matrixRows = roles.map(r => {
      const roleDefs = defaults[r] || {};
      const cells = events.map(ev => {
        // Sans réglage, le serveur abonne tous les rôles sauf admin : on affiche la valeur appliquée.
        const checked = typeof roleDefs[ev.key] === 'boolean' ? roleDefs[ev.key] : r !== 'admin';
        return `
          <td class="text-center">
            <input type="checkbox" name="def_${r}_${ev.key}" ${checked ? 'checked' : ''} ${!canEdit ? 'disabled' : ''} />
          </td>
        `;
      }).join('');

      return `
        <tr>
          <td><span class="${roleBadgeClass(r)} font-bold">${esc(formatRole(r))}</span></td>
          ${cells}
        </tr>
      `;
    }).join('');

    container.innerHTML = `
      <div class="card settings-card">
        <h2>${icon('i-bell')} Alertes & Abonnements par défaut des rôles</h2>
        <p class="hint form-help">Définissez l'heure d'envoi du récapitulatif quotidien et les abonnements pré-activés pour chaque profil d'utilisateur.</p>

        <form class="form-grid" id="settingsAlertsForm" data-action="settings-alerts-submit">
          <div id="settingsAlertsErrorBox" class="form-error" hidden>
            ${icon('i-alert')}
            <span id="settingsAlertsErrorText"></span>
          </div>

          <div class="form-group mb-4">
            <label for="setDigestHour">Heure d'envoi du récapitulatif quotidien des préventifs en retard (0 à 23h) *</label>
            <input type="number" id="setDigestHour" name="overdue_digest_hour" min="0" max="23" value="${esc(al.overdue_digest_hour !== undefined ? al.overdue_digest_hour : 7)}" required ${!canEdit ? 'disabled' : ''} />
            <span class="form-help">Heure locale du serveur à laquelle le bilan des maintenances en retard est envoyé.</span>
          </div>

          <h3>Matrice des abonnements par défaut</h3>
          <div class="table-container mb-4">
            <table class="data-table">
              <thead>
                <tr>
                  <th>Rôle</th>
                  ${events.map(ev => `<th>${esc(ev.label)}</th>`).join('')}
                </tr>
              </thead>
              <tbody>
                ${matrixRows}
              </tbody>
            </table>
          </div>

          ${canEdit ? `
            <button type="submit" class="btn primary" id="settingsAlertsSubmitBtn">
              <span>Enregistrer les préférences d'alertes</span>
            </button>
          ` : '<p class="hint">Consultation seule.</p>'}
        </form>
      </div>
    `;
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur : ${esc(err.message)}</p>`;
  }
}

async function handleSettingsAlertsSubmit(form) {
  const errorBox = document.getElementById('settingsAlertsErrorBox');
  const errorText = document.getElementById('settingsAlertsErrorText');
  const btn = document.getElementById('settingsAlertsSubmitBtn');

  const overdue_digest_hour = parseInt(form.overdue_digest_hour.value, 10);
  if (isNaN(overdue_digest_hour) || overdue_digest_hour < 0 || overdue_digest_hour > 23) {
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = 'L\'heure doit être comprise entre 0 et 23.';
    }
    return;
  }

  const roles = ['admin', 'responsable', 'technicien', 'production'];
  const events = ['stock_low', 'maintenance_generated', 'maintenance_overdue', 'intervention_critical', 'intervention_assigned'];

  const defaults = {};
  roles.forEach(r => {
    defaults[r] = {};
    events.forEach(ev => {
      const input = form.elements[`def_${r}_${ev}`];
      defaults[r][ev] = input ? Boolean(input.checked) : false;
    });
  });

  if (btn) btn.disabled = true;
  if (errorBox) errorBox.hidden = true;

  try {
    const res = await api('/api/settings/alerts', {
      method: 'PUT',
      body: { overdue_digest_hour, defaults },
    });
    state.settingsAlerts = res;
    notify('Préférences d\'alertes enregistrées avec succès.', 'success');
  } catch (err) {
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = err.message || 'Impossible d\'enregistrer les préférences d\'alertes.';
    }
  } finally {
    if (btn) btn.disabled = false;
  }
}

async function loadSettingsAudit() {
  const container = document.getElementById('settingsTabContent');
  if (!container) return;

  const canEdit = hasPermission('settings.edit');
  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement des paramètres d'audit…</p>`;

  try {
    const a = await api('/api/settings/audit');
    state.settingsAudit = a;

    container.innerHTML = `
      <div class="card settings-card">
        <h2>${icon('i-audit')} Rétention du journal d'audit</h2>
        <p class="hint form-help">Durée légale de conservation des traces d'activité et des modifications avant purge quotidienne automatique.</p>

        <form class="form-grid" id="settingsAuditForm" data-action="settings-audit-submit">
          <div id="settingsAuditErrorBox" class="form-error" hidden>
            ${icon('i-alert')}
            <span id="settingsAuditErrorText"></span>
          </div>

          <div class="form-group">
            <label for="setRetentionDays">Durée de conservation (en jours) *</label>
            <input type="number" id="setRetentionDays" name="retention_days" min="365" max="3650" value="${esc(a.retention_days || 1095)}" required ${!canEdit ? 'disabled' : ''} />
            <span class="form-help">Minimum 365 jours (1 an), valeur par défaut 1 095 jours (3 ans).</span>
          </div>

          ${canEdit ? `
            <button type="submit" class="btn primary" id="settingsAuditSubmitBtn">
              <span>Enregistrer la durée de conservation</span>
            </button>
          ` : '<p class="hint">Consultation seule.</p>'}
        </form>
      </div>
    `;
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur : ${esc(err.message)}</p>`;
  }
}

async function handleSettingsAuditSubmit(form) {
  const errorBox = document.getElementById('settingsAuditErrorBox');
  const errorText = document.getElementById('settingsAuditErrorText');
  const btn = document.getElementById('settingsAuditSubmitBtn');

  const retention_days = parseInt(form.retention_days.value, 10);
  if (isNaN(retention_days) || retention_days < 365) {
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = 'La durée de conservation doit être d\'au moins 365 jours.';
    }
    return;
  }

  if (btn) btn.disabled = true;
  if (errorBox) errorBox.hidden = true;

  try {
    const res = await api('/api/settings/audit', {
      method: 'PUT',
      body: { retention_days },
    });
    state.settingsAudit = res;
    notify('Durée de conservation de l\'audit enregistrée avec succès.', 'success');
  } catch (err) {
    if (errorBox) {
      errorBox.hidden = false;
      errorText.textContent = err.message || 'Impossible d\'enregistrer la rétention d\'audit.';
    }
  } finally {
    if (btn) btn.disabled = false;
  }
}

async function loadSettingsDashboardDefaults(role = 'responsable') {
  const container = document.getElementById('settingsTabContent');
  if (!container) return;

  state.settingsDashboardRole = role;
  const canEdit = hasPermission('dashboard.manage_defaults');
  container.innerHTML = `<p class="hint">${icon('i-clock')} Chargement de la disposition par défaut du rôle…</p>`;

  try {
    const layout = await api(`/api/dashboard/defaults/${encodeURIComponent(role)}`);
    state.settingsDashboardRoleLayout = layout;

    const roles = ['admin', 'responsable', 'technicien', 'production'];
    const widgets = (layout && Array.isArray(layout.widgets)) ? layout.widgets : [];

    const widgetRows = widgets.map((w, idx) => `
      <tr>
        <td class="mono font-bold">${idx + 1}</td>
        <td><b>${esc(DASHBOARD_TITLES[w.widget] || w.widget)}</b></td>
        <td class="mono">${w.w} colonnes</td>
        <td>
          ${canEdit ? `
            <div class="cell-actions">
              <button type="button" class="btn sm" data-action="role-widget-up" data-args="${idx}" ${idx === 0 ? 'disabled' : ''}>▲</button>
              <button type="button" class="btn sm" data-action="role-widget-down" data-args="${idx}" ${idx === widgets.length - 1 ? 'disabled' : ''}>▼</button>
              <button type="button" class="btn sm danger" data-action="role-widget-remove" data-args="${idx}">${icon('i-trash')}</button>
            </div>
          ` : '<span class="hint">—</span>'}
        </td>
      </tr>
    `).join('');

    container.innerHTML = `
      <div class="card settings-card">
        <div class="card-head-between">
          <h2>${icon('i-dash')} Dispositions par défaut des tableaux de bord</h2>
          ${canEdit ? `
            <button type="button" class="btn primary" data-action="save-role-dashboard-defaults">
              ${icon('i-check')} <span>Enregistrer pour ce rôle</span>
            </button>
          ` : ''}
        </div>
        <p class="hint form-help">Modifiez la disposition des widgets assignée par défaut aux utilisateurs de chaque rôle.</p>

        <div class="tabs-nav mb-4">
          ${roles.map(r => `
            <button type="button" class="tab-btn ${r === role ? 'on' : ''}" data-action="switch-role-dashboard-tab" data-args="${esc(r)}">
              <span>${esc(formatRole(r))}</span>
            </button>
          `).join('')}
        </div>

        <div class="table-container mb-4">
          <table class="data-table">
            <thead>
              <tr>
                <th>Ordre</th>
                <th>Widget</th>
                <th>Largeur</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              ${widgetRows || '<tr><td colspan="4" class="empty-cell">Aucun widget configuré pour ce rôle.</td></tr>'}
            </tbody>
          </table>
        </div>

        ${canEdit ? `
          <button type="button" class="btn secondary" data-action="open-add-role-widget-modal">
            ${icon('i-plus')} <span>Ajouter un widget au rôle</span>
          </button>
        ` : ''}
      </div>
    `;
  } catch (err) {
    container.innerHTML = `<p class="hint state-ko">${icon('i-alert')} Erreur : ${esc(err.message)}</p>`;
  }
}

async function saveRoleDashboardDefaults() {
  const role = state.settingsDashboardRole || 'responsable';
  const layout = state.settingsDashboardRoleLayout;
  if (!layout) return;

  try {
    await api(`/api/dashboard/defaults/${encodeURIComponent(role)}`, {
      method: 'PUT',
      body: { widgets: layout.widgets || [] },
    });
    notify(`Disposition par défaut pour le rôle « ${formatRole(role)} » enregistrée.`, 'success');
  } catch (err) {
    notify(`Erreur : ${err.message}`, 'error');
  }
}

function handleRoleDefaultMove(idx, dir) {
  const widgets = (state.settingsDashboardRoleLayout && state.settingsDashboardRoleLayout.widgets) || [];
  const target = dir === 'up' ? idx - 1 : idx + 1;
  if (target < 0 || target >= widgets.length) return;

  const tmp = widgets[idx];
  widgets[idx] = widgets[target];
  widgets[target] = tmp;

  loadSettingsDashboardDefaults(state.settingsDashboardRole);
}

function handleRoleDefaultRemove(idx) {
  const widgets = (state.settingsDashboardRoleLayout && state.settingsDashboardRoleLayout.widgets) || [];
  widgets.splice(idx, 1);
  loadSettingsDashboardDefaults(state.settingsDashboardRole);
}

async function openAddRoleWidgetModal() {
  try {
    if (!state.dashboardCatalog.length) {
      const cat = await api('/api/dashboard/catalog');
      state.dashboardCatalog = Array.isArray(cat) ? cat : [];
    }

    const cards = state.dashboardCatalog.map(it => `
      <div class="widget-catalog-card">
        <div>
          <h4>${esc(it.title || it.widget)}</h4>
          <p class="hint">${esc(it.widget)}</p>
        </div>
        <button type="button" class="btn primary sm" data-action="add-widget-to-role-layout" data-args="${jsArg(it.widget)}">
          ${icon('i-plus')} <span>Ajouter</span>
        </button>
      </div>
    `).join('');

    openModal('Ajouter un widget par défaut', `<div class="widget-catalog-grid">${cards}</div>`, '<button type="button" class="btn" data-action="close-modal">Fermer</button>');
  } catch (err) {
    notify(`Erreur : ${err.message}`, 'error');
  }
}

function addWidgetToRoleLayout(widgetKey) {
  if (!state.settingsDashboardRoleLayout) {
    state.settingsDashboardRoleLayout = { widgets: [] };
  }
  const widgets = state.settingsDashboardRoleLayout.widgets || [];
  widgets.push({
    id: `rw_${Date.now()}`,
    widget: widgetKey,
    x: 0,
    y: widgets.length,
    w: 6,
    h: 3,
    params: { period: '30d' },
  });
  closeModal();
  loadSettingsDashboardDefaults(state.settingsDashboardRole);
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
  // Clic à l'extérieur pour fermer le dropdown utilisateur et les menus d'export
  const userDropdown = document.getElementById('userDropdown');
  const userBtn = document.getElementById('userBtn');
  if (userDropdown && !userDropdown.hidden && !userDropdown.contains(e.target) && !userBtn.contains(e.target)) {
    closeUserDropdown();
  }
  if (!e.target.closest('.dropdown-wrap')) {
    document.querySelectorAll('.dropdown-menu').forEach(m => { m.hidden = true; });
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

    // Phase 4 - Magasin & Pièces
    case 'switch-stock-tab':
      state.stockActiveTab = args;
      render();
      break;
    case 'open-create-part':
      openCreatePartModal();
      break;
    case 'open-detail-part':
      location.hash = `#/pieces/${encodeURIComponent(args)}`;
      break;
    case 'open-edit-part':
      openEditPartModal(args);
      break;
    case 'delete-part':
      handleDeletePart(args);
      break;
    case 'load-more-parts':
      loadPartsListView(false);
      break;
    case 'open-movement-in':
      openMovementInModal(args || {});
      break;
    case 'open-movement-transfer':
      openMovementTransferModal(args || {});
      break;
    case 'open-movement-adjust':
      openMovementAdjustModal(args || {});
      break;
    case 'load-more-stock-movements':
      loadStockMovementsListView(false);
      break;
    case 'open-add-intervention-part':
      openAddInterventionPartModal(args);
      break;
    case 'delete-intervention-part':
      handleDeleteInterventionPart(args);
      break;
    case 'open-add-intervention-reservation':
      openAddInterventionReservationModal(args);
      break;
    case 'open-consume-reservation':
      openConsumeReservationModal(args);
      break;
    case 'release-reservation':
      handleReleaseReservation(args);
      break;
    case 'open-add-part-equipment':
      openAddPartEquipmentModal(args);
      break;
    case 'delete-part-equipment':
      handleDeletePartEquipment(args.partId, args.equipmentId);
      break;
    case 'open-add-equipment-part':
      openAddEquipmentPartModal(args);
      break;
    case 'delete-equipment-part':
      handleDeleteEquipmentPart(args);
      break;
    case 'quick-select-int-part': {
      const partSelect = document.getElementById('intPartId') || document.getElementById('intResPartId');
      if (partSelect) {
        partSelect.value = args;
        const siteId = partSelect.dataset.args ? JSON.parse(partSelect.dataset.args) : '';
        updateInterventionPartLocations(args, siteId);
      }
      break;
    }

    // Phase 5 — Maintenance préventive et compteurs
    case 'switch-maintenance-tab':
      state.maintenanceTab = args || 'plans';
      render();
      break;
    case 'switch-schedule-range':
      state.maintenanceScheduleRange = String(args || 'month');
      renderMaintenanceScheduleView();
      loadMaintenanceSchedule();
      break;
    case 'schedule-prev-month': {
      const cur = state.maintenanceScheduleMonth || localIso().slice(0, 7);
      const [y, m] = cur.split('-').map(Number);
      const prev = new Date(Date.UTC(y, m - 2, 1));
      state.maintenanceScheduleMonth = prev.toISOString().slice(0, 7);
      renderMaintenanceScheduleView();
      loadMaintenanceSchedule();
      break;
    }
    case 'schedule-next-month': {
      const cur = state.maintenanceScheduleMonth || localIso().slice(0, 7);
      const [y, m] = cur.split('-').map(Number);
      const next = new Date(Date.UTC(y, m, 1));
      state.maintenanceScheduleMonth = next.toISOString().slice(0, 7);
      renderMaintenanceScheduleView();
      loadMaintenanceSchedule();
      break;
    }
    case 'schedule-today':
      state.maintenanceScheduleMonth = localIso().slice(0, 7);
      renderMaintenanceScheduleView();
      loadMaintenanceSchedule();
      break;
    case 'open-create-maintenance-plan':
      openCreateMaintenancePlanModal(args || {});
      break;
    case 'open-edit-maintenance-plan':
      openEditMaintenancePlanModal(args);
      break;
    case 'open-skip-plan-modal':
      openSkipPlanModal(args);
      break;
    case 'generate-plan-intervention':
      generatePlanIntervention(args);
      break;
    case 'toggle-plan-active':
      togglePlanActive(args);
      break;
    case 'delete-maintenance-plan':
      deleteMaintenancePlan(args);
      break;
    case 'load-more-maintenance-plans':
      loadMaintenancePlans(false);
      break;
    case 'load-more-plan-occurrences':
      loadMorePlanOccurrences(args);
      break;
    case 'open-create-counter':
      openCreateCounterModal(args);
      break;
    case 'open-edit-counter':
      openEditCounterModal(args);
      break;
    case 'delete-counter':
      deleteCounter(args);
      break;
    case 'open-record-reading':
      openRecordReadingModal(args);
      break;
    case 'open-reset-counter':
      openResetCounterModal(args);
      break;
    case 'open-counter-history':
      openCounterHistoryModal(args);
      break;

    // Phase 6 - Documents, Étiquettes & Paramètres
    case 'open-upload-document':
      openUploadDocModal(args || {});
      break;
    case 'open-new-version':
      openNewVersionModal(args || {});
      break;
    case 'open-document-history':
      openDocumentHistoryModal(args || {});
      break;
    case 'restore-doc-version':
      restoreDocumentVersion(args || {});
      break;
    case 'open-edit-document':
      openEditDocumentModal(args || {});
      break;
    case 'delete-document':
      deleteDocument(args || {});
      break;
    case 'load-more-documents':
      loadMoreDocuments();
      break;
    case 'print-equipment-label':
      closeModal();
      location.hash = `#/etiquettes?ids=${encodeURIComponent(args)}`;
      break;
    case 'trigger-print':
      window.print();
      break;
    case 'switch-settings-tab':
      switchSettingsTab(args);
      break;
    case 'open-create-storage':
      openCreateStorageModal();
      break;
    case 'open-edit-storage':
      openEditStorageModal(args);
      break;
    case 'test-storage':
      testStorageConnection(args);
      break;
    case 'activate-storage':
      activateStorage(args);
      break;
    case 'delete-storage':
      deleteStorage(args);
      break;

    // Phase 7 — Pilotage, Tableaux de bord, Reporting, Audit, Exports, Paramètres
    case 'toggle-export-dropdown':
      toggleExportDropdown(actEl);
      break;
    case 'run-export':
      handleRunExport(args);
      break;
    case 'download-intervention-pdf':
      downloadFile(`/api/interventions/${encodeURIComponent(args)}/pdf`);
      break;
    case 'toggle-dashboard-customize':
      toggleDashboardCustomize();
      break;
    case 'open-add-widget-modal':
      openAddDashboardWidgetModal();
      break;
    case 'add-widget-from-catalog':
      addWidgetFromCatalog(args);
      break;
    case 'move-widget-prev':
      moveDashboardWidget(args, 'prev');
      break;
    case 'move-widget-next':
      moveDashboardWidget(args, 'next');
      break;
    case 'cycle-widget-size':
      cycleDashboardWidgetSize(args);
      break;
    case 'open-widget-config':
      openDashboardWidgetConfigModal(args);
      break;
    case 'delete-dashboard-widget':
      removeDashboardWidget(args);
      break;
    case 'save-dashboard-layout':
      saveDashboardLayout();
      break;
    case 'reset-dashboard-layout':
      resetDashboardLayout();
      break;
    case 'switch-reporting-tab':
      switchReportingTab(args);
      break;
    case 'refresh-reporting-data':
      loadReportingData();
      break;
    case 'refresh-audit-list':
      loadAuditList(true);
      break;
    case 'load-more-audit':
      loadAuditList(false);
      break;
    case 'open-audit-detail':
      openAuditDetailModal(args);
      break;
    case 'test-smtp-settings':
      handleTestSmtp();
      break;
    case 'switch-role-dashboard-tab':
      loadSettingsDashboardDefaults(args);
      break;
    case 'save-role-dashboard-defaults':
      saveRoleDashboardDefaults();
      break;
    case 'role-widget-up':
      handleRoleDefaultMove(parseInt(args, 10), 'up');
      break;
    case 'role-widget-down':
      handleRoleDefaultMove(parseInt(args, 10), 'down');
      break;
    case 'role-widget-remove':
      handleRoleDefaultRemove(parseInt(args, 10));
      break;
    case 'open-add-role-widget-modal':
      openAddRoleWidgetModal();
      break;
    case 'add-widget-to-role-layout':
      addWidgetToRoleLayout(args);
      break;
    case 'nav-login':
      showLoginScreen();
      break;

    default:
      break;
  }
}

function handleGlobalSubmit(e) {
  const form = e.target.closest('form');
  if (!form) return;

  e.preventDefault();
  const action = form.dataset.action || form.id;
  let args = null;
  if (form.dataset.args) {
    try {
      args = JSON.parse(form.dataset.args);
    } catch (_) {}
  }

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

    // Phase 5
    case 'create-maintenance-plan-submit':
      handleCreateMaintenancePlanSubmit(form);
      break;
    case 'edit-maintenance-plan-submit':
      handleEditMaintenancePlanSubmit(form);
      break;
    case 'skip-plan-submit':
      handleSkipPlanSubmit(form);
      break;
    case 'create-counter-submit':
      handleCreateCounterSubmit(form);
      break;
    case 'edit-counter-submit':
      handleEditCounterSubmit(form);
      break;
    case 'record-reading-submit':
      handleRecordReadingSubmit(form);
      break;
    case 'reset-counter-submit':
      handleResetCounterSubmit(form);
      break;

    // Phase 4 - Magasin & Pièces
    case 'create-part-submit':
      handleCreatePartSubmit(form);
      break;
    case 'edit-part-submit':
      handleEditPartSubmit(form, args);
      break;
    case 'movement-in-submit':
      handleMovementInSubmit(form);
      break;
    case 'movement-transfer-submit':
      handleMovementTransferSubmit(form);
      break;
    case 'movement-adjust-submit':
      handleMovementAdjustSubmit(form);
      break;
    case 'add-intervention-part-submit':
      handleAddInterventionPartSubmit(form, args);
      break;
    case 'add-intervention-reservation-submit':
      handleAddInterventionReservationSubmit(form, args);
      break;
    case 'consume-reservation-submit':
      handleConsumeReservationSubmit(form, args);
      break;
    case 'add-part-equipment-submit':
      handleAddPartEquipmentSubmit(form, args);
      break;
    case 'add-equipment-part-submit':
      handleAddEquipmentPartSubmit(form, args);
      break;

    // Phase 6 - Documents, Étiquettes & Paramètres
    case 'docUploadForm':
      submitDocUpload(form);
      break;
    case 'docVersionForm':
      submitNewVersion(form);
      break;
    case 'editDocForm':
      submitEditDocument(form);
      break;
    case 'createStorageForm':
      submitCreateStorage(form);
      break;
    case 'editStorageForm':
      submitEditStorage(form);
      break;
    case 'settingsDocumentsForm':
      submitSettingsDocuments(form);
      break;
    case 'settingsLabelsForm':
      submitSettingsLabels(form);
      break;

    // Phase 7
    case 'reset-password-request-submit':
      handleResetPasswordRequestSubmit(form);
      break;
    case 'reset-password-confirm-submit':
      handleResetPasswordConfirmSubmit(form);
      break;
    case 'save-widget-config-submit':
      handleSaveWidgetConfigSubmit(args, form);
      break;
    case 'profile-email-submit':
      handleProfileEmailSubmit(form);
      break;
    case 'profile-notifications-submit':
      handleProfileNotificationsSubmit(form);
      break;
    case 'settings-general-submit':
      handleSettingsGeneralSubmit(form);
      break;
    case 'settings-security-submit':
      handleSettingsSecuritySubmit(form);
      break;
    case 'settings-stock-submit':
      handleSettingsStockSubmit(form);
      break;
    case 'settings-smtp-submit':
      handleSettingsSmtpSubmit(form);
      break;
    case 'settings-alerts-submit':
      handleSettingsAlertsSubmit(form);
      break;
    case 'settings-audit-submit':
      handleSettingsAuditSubmit(form);
      break;

    default:
      break;
  }
}

function handleGlobalInput(e) {
  if (e.target && e.target.id === 'documentsSearchInput') {
    state.documentsSearchQuery = e.target.value;
    debounce('documents-search', () => loadDocumentsList(true), 300);
    return;
  }

  if (e.target && e.target.closest('#settingsLabelsForm')) {
    updateLabelsPreviewLive();
    return;
  }

  const input = e.target.closest('input[data-action]');
  if (!input) return;

  const action = input.dataset.action;
  if (action === 'change-labels-offset') {
    changeLabelsOffset(input.value);
    return;
  }
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
  } else if (action === 'filter-maintenance-plans-search') {
    state.maintenancePlansSearchQuery = input.value;
    debounce('maintenance-plans-search', () => loadMaintenancePlans(true), 300);
  } else if (action === 'filter-parts-search') {
    state.partsSearchQuery = input.value;
    debounce('parts-search', () => loadPartsListView(true), 300);
  } else if (action === 'filter-audit-query') {
    state.auditQuery = input.value;
    debounce('audit-search', () => loadAuditList(true), 300);
  } else if (action === 'on-int-part-qty-input') {
    updateInterventionPartEstim();
  }
}

function handleGlobalChange(e) {
  // Sélecteur de site d'en-tête
  if (e.target && e.target.id === 'siteSelect') {
    state.selectedSiteId = e.target.value;
    const r = currentRoute();
    if (r === 'tableau-de-bord') {
      loadDashboard();
    } else if (r === 'reporting') {
      loadReportingData();
    } else if (r === 'audit') {
      loadAuditList(true);
    } else if (r === 'utilisateurs') {
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
    } else if (r === 'stock') {
      initStockPageView();
    } else if (r === 'maintenance-preventive') {
      initMaintenancePageView();
    } else if (r === 'documents') {
      loadDocumentsList(true);
    }
    return;
  }

  if (e.target && e.target.id === 'docFileInput') {
    const file = e.target.files && e.target.files[0];
    if (file) {
      const titleInput = document.getElementById('docTitleInput');
      if (titleInput && !titleInput.value.trim()) {
        titleInput.value = file.name.replace(/\.[^/.]+$/, '');
      }
    }
    return;
  }

  if (e.target && e.target.closest('#settingsLabelsForm')) {
    updateLabelsPreviewLive();
    return;
  }

  // Case à cocher
  const chk = e.target.closest('input[type="checkbox"][data-action]');
  if (chk) {
    const chkAction = chk.dataset.action;
    if (chkAction === 'filter-parts-low-stock') {
      state.partsLowStockFilter = chk.checked;
      loadPartsListView(true);
      return;
    }
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
  } else if (action === 'filter-maintenance-plans-frequency') {
    state.maintenancePlansFrequencyFilter = sel.value;
    loadMaintenancePlans(true);
  } else if (action === 'filter-maintenance-plans-due') {
    state.maintenancePlansDueFilter = sel.value;
    loadMaintenancePlans(true);
  } else if (action === 'filter-maintenance-plans-equipment') {
    state.maintenancePlansEquipmentFilter = sel.value;
    loadMaintenancePlans(true);
  } else if (action === 'filter-schedule-equipment') {
    state.maintenancePlansEquipmentFilter = sel.value;
    loadMaintenanceSchedule();
  } else if (action === 'on-plan-form-frequency-change') {
    const freq = sel.value;
    const intervalGroup = document.getElementById('planIntervalDaysGroup');
    const intervalInput = document.getElementById('fPlanIntervalDays');
    const counterGroup = document.getElementById('planCounterGroup');
    const counterSelect = document.getElementById('fPlanCounterSelect');
    const counterInterval = document.getElementById('fPlanCounterInterval');
    const calendarGroup = document.getElementById('planCalendarGroup');
    const anchorInput = document.getElementById('fPlanAnchorDate');

    if (intervalGroup) intervalGroup.hidden = freq !== 'interval_days';
    if (intervalInput) {
      if (freq === 'interval_days') {
        intervalInput.setAttribute('required', 'required');
      } else {
        intervalInput.removeAttribute('required');
      }
    }

    if (counterGroup) counterGroup.hidden = freq !== 'counter';
    if (counterSelect) {
      if (freq === 'counter') {
        counterSelect.setAttribute('required', 'required');
      } else {
        counterSelect.removeAttribute('required');
      }
    }
    if (counterInterval) {
      if (freq === 'counter') {
        counterInterval.setAttribute('required', 'required');
      } else {
        counterInterval.removeAttribute('required');
      }
    }

    if (calendarGroup) calendarGroup.hidden = freq === 'counter';
    if (anchorInput) {
      if (freq !== 'counter') {
        anchorInput.setAttribute('required', 'required');
      } else {
        anchorInput.removeAttribute('required');
      }
    }
  } else if (action === 'on-plan-form-equipment-change') {
    updateCounterSelectForEquipment(sel.value);
  } else if (action === 'filter-parts-category') {
    state.partsCategoryFilter = sel.value;
    loadPartsListView(true);
  } else if (action === 'filter-stock-location') {
    state.stockLevelsLocationFilter = sel.value;
    loadStockLevelsView();
  } else if (action === 'filter-stock-movements-type') {
    state.stockMovementsTypeFilter = sel.value;
    loadStockMovementsListView(true);
  } else if (action === 'on-int-part-select') {
    const siteId = sel.dataset.args ? JSON.parse(sel.dataset.args) : '';
    updateInterventionPartLocations(sel.value, siteId);
  } else if (action === 'on-int-part-loc-select') {
    updateInterventionPartEstim();
  } else if (action === 'on-movement-in-part-change') {
    const opt = sel.selectedOptions[0];
    const priceInput = document.getElementById('mInPrice');
    if (opt && priceInput && opt.dataset.pmp) {
      priceInput.value = centsToEuroInput(opt.dataset.pmp);
    }
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
  } else if (action === 'filter-documents-entity-type') {
    state.documentsEntityTypeFilter = sel.value;
    loadDocumentsList(true);
  } else if (action === 'filter-documents-category') {
    state.documentsCategoryFilter = sel.value;
    loadDocumentsList(true);
  } else if (action === 'toggle-storage-driver-fields') {
    const isS3 = sel.value === 's3';
    const folderFields = document.getElementById('storageFolderFields');
    const s3Fields = document.getElementById('storageS3Fields');
    if (folderFields) folderFields.hidden = isS3;
    if (s3Fields) s3Fields.hidden = !isS3;
  } else if (action === 'filter-reporting-period') {
    state.reportingPeriod = sel.value;
    const customDates = document.getElementById('reportingCustomDates');
    if (customDates) customDates.hidden = sel.value !== 'custom';
    loadReportingData();
  } else if (action === 'filter-reporting-site') {
    state.reportingSiteId = sel.value;
    loadReportingData();
  } else if (action === 'filter-reporting-line') {
    state.reportingLineId = sel.value;
    loadReportingData();
  } else if (action === 'filter-reporting-equipment') {
    state.reportingEquipmentId = sel.value;
    loadReportingData();
  } else if (action === 'filter-audit-action') {
    state.auditAction = sel.value;
    loadAuditList(true);
  } else if (action === 'filter-audit-entity') {
    state.auditEntity = sel.value;
    loadAuditList(true);
  } else if (action === 'filter-audit-site') {
    state.auditSiteId = sel.value;
    loadAuditList(true);
  }

  // Événements de changement sur inputs de type date
  const dateInput = e.target.closest('input[type="date"][data-action]');
  if (dateInput) {
    const dAction = dateInput.dataset.action;
    if (dAction === 'filter-reporting-from') {
      state.reportingFrom = dateInput.value;
      if (state.reportingPeriod === 'custom' && state.reportingFrom && state.reportingTo) {
        loadReportingData();
      }
    } else if (dAction === 'filter-reporting-to') {
      state.reportingTo = dateInput.value;
      if (state.reportingPeriod === 'custom' && state.reportingFrom && state.reportingTo) {
        loadReportingData();
      }
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
  document.addEventListener('dragover', handleDropzoneDragOver);
  document.addEventListener('dragleave', handleDropzoneDragLeave);
  document.addEventListener('drop', handleDropzoneDrop);

  // Glisser-déposer des widgets du tableau de bord
  document.addEventListener('dragstart', e => {
    const handle = e.target.closest('[data-action="widget-drag-handle"]');
    if (!handle) return;
    let widgetId = '';
    try {
      widgetId = handle.dataset.args ? JSON.parse(handle.dataset.args) : '';
    } catch (_) {
      widgetId = handle.dataset.args || '';
    }
    if (!widgetId) return;
    const widgetEl = handle.closest('.dashboard-widget');
    if (widgetEl) widgetEl.classList.add('dragging');
    e.dataTransfer.setData('text/plain', widgetId);
    e.dataTransfer.effectAllowed = 'move';
  });

  document.addEventListener('dragend', e => {
    const handle = e.target.closest('[data-action="widget-drag-handle"]');
    if (handle) {
      const widgetEl = handle.closest('.dashboard-widget');
      if (widgetEl) widgetEl.classList.remove('dragging');
    }
  });

  document.addEventListener('dragover', e => {
    const targetWidget = e.target.closest('.dashboard-widget');
    if (targetWidget && state.dashboardCustomizing) {
      e.preventDefault();
      e.dataTransfer.dropEffect = 'move';
    }
  });

  document.addEventListener('drop', e => {
    const targetWidget = e.target.closest('.dashboard-widget');
    if (!targetWidget || !state.dashboardCustomizing) return;
    e.preventDefault();
    const sourceId = e.dataTransfer.getData('text/plain');
    const targetId = targetWidget.dataset.widgetId;
    if (!sourceId || !targetId || sourceId === targetId) return;

    const widgets = (state.dashboardLayout && state.dashboardLayout.widgets) || [];
    const srcIdx = widgets.findIndex(w => w.id === sourceId);
    const dstIdx = widgets.findIndex(w => w.id === targetId);
    if (srcIdx !== -1 && dstIdx !== -1) {
      const item = widgets.splice(srcIdx, 1)[0];
      widgets.splice(dstIdx, 0, item);
      renderDashboardGrid();
    }
  });

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
