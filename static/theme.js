/* ============================================================================
   Apex Maint — Choix du thème (sombre ou clair)
   ----------------------------------------------------------------------------
   Chargé de façon bloquante dans <head> pour appliquer le thème avant le premier
   affichage (pas de flash). Sans choix enregistré, le thème suit celui du système.
   ============================================================================ */
'use strict';

const THEME_KEY = 'apex_maint_theme';

/** Thème enregistré par l'utilisateur, ou null s'il n'a jamais choisi. */
function savedTheme() {
  try {
    const t = localStorage.getItem(THEME_KEY);
    return t === 'light' || t === 'dark' ? t : null;
  } catch (_) {
    return null;
  }
}

function systemTheme() {
  return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
}

function currentTheme() {
  return document.documentElement.dataset.theme === 'light' ? 'light' : 'dark';
}

function applyTheme(theme) {
  document.documentElement.dataset.theme = theme;
}

/** Bascule entre les deux thèmes et mémorise le choix. */
function toggleTheme() {
  const next = currentTheme() === 'light' ? 'dark' : 'light';
  applyTheme(next);
  try {
    localStorage.setItem(THEME_KEY, next);
  } catch (_) { /* choix non conservé */ }
  return next;
}

applyTheme(savedTheme() || systemTheme());

// Sans choix explicite, on suit les changements du système (mode nuit automatique...).
window.matchMedia('(prefers-color-scheme: light)').addEventListener('change', () => {
  if (!savedTheme()) applyTheme(systemTheme());
});
