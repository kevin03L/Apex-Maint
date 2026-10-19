/* ============================================================================
   Apex Maint — Service Worker (PWA & Cache Hors-ligne)
   Règles impératives :
   - Fichiers statiques mis en cache pour fonctionnement hors ligne.
   - JAMAIS de mise en cache des requêtes /api/ (gérées via IndexedDB).
   - Détection de nouvelle version sans forcer le rechargement en pleine saisie.
   ============================================================================ */
'use strict';

const CACHE_NAME = 'apex-maint-v0.8.0';

const STATIC_ASSETS = [
  '/',
  '/index.html',
  '/styles.css',
  '/print.css',
  '/theme.js',
  '/app.js',
  '/offline.js',
  '/manifest.webmanifest',
  '/icons/icon-192.png',
  '/icons/icon-512.png',
  '/icons/icon-512-maskable.png',
  '/icons/icon.svg',
  '/vendor/jsqr/jsQR.js'
];

self.addEventListener('install', event => {
  event.waitUntil(
    caches.open(CACHE_NAME).then(cache => {
      return cache.addAll(STATIC_ASSETS).catch(err => {
        // En cas d'échec sur une ressource facultative, continuer
        console.warn('Pré-chargement partiel du cache statique:', err);
      });
    })
  );
});

self.addEventListener('activate', event => {
  event.waitUntil(
    caches.keys().then(keys => {
      return Promise.all(
        keys.filter(k => k !== CACHE_NAME).map(k => caches.delete(k))
      );
    }).then(() => self.clients.claim())
  );
});

self.addEventListener('fetch', event => {
  const url = new URL(event.request.url);

  // RÈGLE : JAMAIS de mise en cache du Service Worker pour les routes /api/
  if (url.pathname.startsWith('/api/')) {
    return;
  }

  // Ne pas intercepter les requêtes non-GET
  if (event.request.method !== 'GET') {
    return;
  }

  // Pour les requêtes de navigation (HTML de la page)
  if (event.request.mode === 'navigate') {
    event.respondWith(
      fetch(event.request).catch(async () => {
        const cache = await caches.open(CACHE_NAME);
        const match = await cache.match('/index.html') || await cache.match('/');
        return match || new Response('Hors-ligne : interface non disponible en cache.', {
          status: 503,
          headers: { 'Content-Type': 'text/plain; charset=utf-8' }
        });
      })
    );
    return;
  }

  // Pour les ressources statiques : Cache d'abord, puis réseau avec mise à jour du cache
  event.respondWith(
    caches.match(event.request).then(cached => {
      if (cached) {
        // En arrière-plan, tenter de mettre à jour le cache statique si en ligne
        fetch(event.request).then(response => {
          if (response && response.status === 200 && response.type === 'basic') {
            const respClone = response.clone();
            caches.open(CACHE_NAME).then(cache => cache.put(event.request, respClone));
          }
        }).catch(() => {
          // Ignorer les erreurs réseau en arrière-plan
        });
        return cached;
      }
      return fetch(event.request).then(response => {
        if (!response || response.status !== 200 || response.type !== 'basic') {
          return response;
        }
        const respClone = response.clone();
        caches.open(CACHE_NAME).then(cache => cache.put(event.request, respClone));
        return response;
      });
    })
  );
});

self.addEventListener('message', event => {
  if (event.data && event.data.type === 'SKIP_WAITING') {
    self.skipWaiting();
  }
});
