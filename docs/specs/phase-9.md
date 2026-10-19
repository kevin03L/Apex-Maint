# Phase 9 — Terrain

Spécification de travail de la phase 9 (voir `docs/PLAN.md` §4 et `docs/specs/decisions.md`, qui
prime). Elle s'appuie sur les phases 1 à 8. Le serveur reste la référence : le hors-ligne n'est
qu'un **cache et une file d'attente** côté appareil.

## Résultat attendu

- **PWA installable** sur tablette et téléphone (Android et iOS), avec une IHM utilisable au doigt.
- **HTTPS paramétrable** : TLS intégré à Apex Maint, ou proxy inverse, au choix.
- **Mode hors-ligne pour le technicien** :
  - consulter ses interventions, les équipements et les pièces ;
  - avancer ses interventions : prise en charge, temps, compte rendu, clôture ;
  - déclarer une intervention.
  Une **synchronisation sûre** a lieu au retour du réseau.
- **Lecture des QR codes** par la caméra : elle ouvre la fiche de l'équipement.

Ne sont **pas** disponibles hors ligne : les photos et les pièces jointes, la consommation et la
réservation de pièces, la maintenance préventive (en dehors de la consultation), l'administration.
Ces actions sont masquées ou désactivées hors ligne, avec la mention « Disponible en ligne ».

## 1. HTTPS paramétrable

Le service worker et la caméra exigent un contexte sécurisé : HTTPS, ou `localhost`.

### Configuration

Elle est lue au démarrage et vérifiée par `config check`. Section `[server.tls]` de
`apex-maint.toml` :

| Clé | Contenu |
|---|---|
| `mode` | `off` (par défaut), `files` ou `self_signed` |
| `cert_file`, `key_file` | Pour `files` : certificat PEM, avec la chaîne complète, et clé privée. Chemins absolus ou relatifs à `data_dir`. |
| `self_signed_names` | Pour `self_signed` : noms DNS et adresses IP couverts. Par défaut, le nom de la machine et ses adresses IP. |
| `http_redirect_port` | Facultatif : un port HTTP qui redirige vers HTTPS. |

### Les trois modes

- **`files`** : le certificat est fourni par l'entreprise (autorité interne ou publique). Il est
  rechargé à chaud quand le fichier change (vérification toutes les heures), sans redémarrage.
- **`self_signed`** : au premier démarrage, Apex Maint génère avec la crate `rcgen` une **petite
  autorité locale** (`data/tls/ca.pem`) et un certificat serveur signé par elle, valable 2 ans,
  renouvelé automatiquement 30 jours avant son expiration.
  - L'IHM et l'utilitaire d'administration proposent de **télécharger l'autorité**, à installer
    une seule fois sur les tablettes. La page d'aide explique la procédure pour Android et iOS.
  - La clé de l'autorité ne quitte jamais `data/tls/`, dont les droits sont restreints.
- **Proxy inverse** : on laisse `mode = "off"` et on déclare `[server] trusted_proxies = ["10.0.0.5"]`.
  Seules ces adresses peuvent fournir `X-Forwarded-For` et `X-Forwarded-Proto`.
  - C'est cohérent avec la règle existante : derrière un proxy, les détails d'exécution et le jeton
    de contrôle restent refusés.
  - Le cookie de session prend `Secure` dès que la requête est vue en HTTPS, en direct ou par un
    proxy de confiance.
- **HSTS** : `Strict-Transport-Security` n'est envoyé qu'en HTTPS. Sa durée courte par défaut
  (1 jour) est réglable.

### Bibliothèques

- `axum-server` avec `rustls` ;
- `rcgen` pour l'autorité locale ;
- aucune dépendance à OpenSSL.

## 2. PWA

- **Manifeste** `static/manifest.webmanifest` :
  - nom `app_name` (statique, « Apex Maint » par défaut) ;
  - icônes de 192, 512 et 512 px (« maskable »), tirées de l'icône de la gamme Apex ;
  - `display: standalone`, `start_url: "/#/terrain"` ;
  - couleurs du thème.
- **Service worker** `static/sw.js`, servi avec `Cache-Control: no-cache` et la portée `/` :
  - il met en **cache de l'application** les fichiers statiques (HTML, CSS, JS, icônes, police),
    avec un nom de cache versionné par la version d'Apex Maint ;
  - à chaque nouvelle version, il installe le nouveau cache et le propose à l'utilisateur
    (« Nouvelle version disponible — Recharger »). Il ne force jamais le rechargement en pleine
    saisie ;
  - il **ne met jamais en cache les réponses de `/api/`**. Les données hors ligne sont gérées par
    l'application dans IndexedDB (§3), pour maîtriser leur contenu et leur effacement.
- **CSP** : `worker-src 'self'` et `manifest-src 'self'` sont ajoutés. Le reste est inchangé.
- **Vue « Terrain »** (`#/terrain`) : écran d'accueil du technicien sur mobile.
  - Gros boutons : « Scanner un équipement », « Mes interventions », « Déclarer une
    intervention ».
  - Indicateur de connexion et nombre d'opérations en attente.
  - Les cibles tactiles mesurent au moins 44 px.

## 3. Hors-ligne

### 3.1 Données gardées sur l'appareil (IndexedDB)

- **Instantané pour le technicien**, rafraîchi à chaque ouverture en ligne et toutes les 15 minutes
  en ligne. Il contient :
  - ses interventions ouvertes et en cours, plus celles des 7 derniers jours, avec leurs
    techniciens et leurs temps ;
  - les équipements de son site : code, nom, statut, ligne, parent, compteurs ;
  - les lignes de production ;
  - le catalogue des pièces en lecture : référence, nom, disponible ;
  - les utilisateurs assignables (nom affiché seulement).
- **Données de session** : le résultat de `/api/auth/me`, c'est-à-dire l'utilisateur, ses
  permissions et son site. L'IHM hors ligne masque selon ces permissions ; le serveur re-vérifie
  tout à la synchronisation.
- **Route** : `GET /api/offline/snapshot` (droit `interventions.view`, site de l'acteur). Elle
  renvoie tout en une réponse compressée, avec `generated_at`.
  - Avec `?since=`, elle ne renvoie que les changements, s'ils remontent à moins de 24 h.
  - Taille visée : moins de 2 Mo pour 5 000 équipements.
- **Ce qui ne va jamais dans IndexedDB** : jeton de session (il reste dans le cookie `HttpOnly`),
  mots de passe, secrets, documents.
- **Effacement** : la **déconnexion efface IndexedDB et le cache des données**.
  - S'il reste des opérations non synchronisées, la déconnexion demande d'abord confirmation
    (« X opérations seront perdues »).
  - Un changement d'utilisateur sur l'appareil efface aussi tout.

### 3.2 Opérations hors ligne

Chaque action faite hors ligne est ajoutée à une **file d'opérations** (`outbox`, dans IndexedDB)
et appliquée tout de suite à l'instantané local, pour que l'écran reste juste.

| Opération | Route appelée à la synchronisation |
|---|---|
| Déclarer une intervention | `POST /api/interventions` |
| Prendre en charge | `POST /api/interventions/{id}/take` |
| Saisir son temps | `PUT /api/interventions/{id}/technicians/{moi}/time` |
| Modifier le compte rendu et la description | `PATCH /api/interventions/{id}` |
| Clôturer | `POST /api/interventions/{id}/complete` |

- **Identifiant de l'opération** : chaque opération porte un **identifiant unique généré par
  l'appareil** (UUID v7), envoyé dans l'en-tête `Idempotency-Key`.
- **Identifiant temporaire** : une intervention déclarée hors ligne reçoit un identifiant
  temporaire. Sa référence `INT-…` est attribuée par le serveur à la synchronisation, et l'IHM
  affiche « Référence provisoire » jusque-là.
  - Les opérations suivantes sur cette intervention, par exemple une prise en charge, attendent
    son identifiant définitif et sont réécrites avec.
- **Horodatage réel** : la date de l'action est envoyée dans l'en-tête `X-Occurred-At`.
  - Le serveur l'utilise pour `reported_at`, `started_at` et `completed_at` si elle est cohérente :
    pas plus de 5 minutes dans le futur, pas plus de 7 jours dans le passé, et pas avant la date
    de l'étape précédente.
  - Sinon, il prend l'heure de réception et l'indique dans l'audit.

### 3.3 Synchronisation

- **Déclenchement** : au retour du réseau (événement `online`, avec une sonde `/api/health`),
  à l'ouverture de l'application et sur demande. **Background Sync** est utilisé si le navigateur
  le permet, sinon la synchronisation se fait au premier plan.
- **Ordre** : les opérations sont envoyées **dans l'ordre**, une à une. Une opération en échec
  bloque seulement celles qui dépendent d'elle, c'est-à-dire celles qui portent sur la même
  intervention.
- **Idempotence côté serveur** : nouvelle table `idempotency_keys` (clé, `user_id`, route, code et
  corps de la réponse, `created_at`), conservée 30 jours.
  - Une même clé rejouée renvoie la réponse enregistrée sans rien réappliquer.
  - La même clé utilisée par un autre utilisateur ou sur une autre route répond 422.
- **Conflits** : le serveur décide.
  - **Refus métier** : par exemple une intervention clôturée ou annulée entre-temps (409), une
    assignation retirée (403) ou une validation échouée (400). L'opération passe dans **« À
    résoudre »** avec le message du serveur. Le technicien peut l'**abandonner**, ou la **reprendre**
    en ligne après avoir vu l'état actuel. Aucune donnée n'est perdue en silence.
  - **Compte rendu modifié des deux côtés** : `PATCH` envoie `If-Unmodified-Since`, c'est-à-dire
    le `updated_at` vu hors ligne. Si l'intervention a changé depuis, la réponse est 412.
    L'IHM montre alors les deux versions et le technicien choisit, ou fusionne à la main.
- **Session expirée** : la synchronisation s'arrête et demande de se reconnecter. La file est
  conservée, car elle est rattachée à l'utilisateur.
  - Si un **autre** utilisateur se connecte, la file du précédent n'est pas envoyée sous son
    identité. Elle est proposée à l'export (fichier JSON), puis effacée.
- **Interface** :
  - badge du nombre d'opérations en attente ;
  - page « Synchronisation » : opérations en attente, à résoudre et synchronisées récemment, avec
    la dernière synchronisation réussie ;
  - bandeau « Hors ligne — vos modifications seront envoyées au retour du réseau ».

## 4. Lecture des QR codes

- **Caméra** : `getUserMedia` avec la caméra arrière. On utilise `BarcodeDetector` si le navigateur
  le fournit (Chrome Android) ; sinon, la bibliothèque **jsQR**.
  - jsQR est sous licence Apache 2.0, en JavaScript pur, sans WebAssembly, donc compatible avec la
    CSP actuelle.
  - Elle est **livrée dans `static/vendor/jsqr/`** avec sa licence, et n'est chargée que sur l'écran
    de lecture.
- **Saisie manuelle** du code, en secours, sur le même écran.
- **Résolution du code** :
  - le code lu est cherché dans les équipements du site de l'utilisateur, dans l'instantané hors
    ligne puis côté serveur en ligne ;
  - s'il n'y a pas de correspondance, le message est « Équipement inconnu sur ce site » ;
  - pour l'admin, si plusieurs sites ont le même code, l'IHM propose de choisir.
- **Après la lecture** : fiche terrain de l'équipement, avec son statut, ses interventions en
  cours, « Déclarer une intervention » (l'équipement est prérempli), ses compteurs et ses
  documents (ouverture en ligne seulement).

## 5. Ajouts côté serveur

- `GET /api/offline/snapshot` (§3.1).
- Prise en charge des en-têtes `Idempotency-Key`, `X-Occurred-At` et `If-Unmodified-Since` sur les
  routes du §3.2, avec la table `idempotency_keys` (migration `0010_terrain.sql`).
- `GET /api/equipments/by-code/{code}` : équipement du site de l'acteur. Pour l'admin, la réponse
  est une liste si le code existe dans plusieurs sites.
- Configuration TLS et proxies de confiance (§1), page de téléchargement de l'autorité locale
  (`GET /api/tls/ca.pem`, route publique, seulement en mode `self_signed`).
- La tâche de purge quotidienne efface les clés d'idempotence de plus de 30 jours.

## 6. Tests

### Rust

- Configuration TLS : `config check` sur des fichiers manquants, une clé ne correspondant pas au
  certificat et un certificat expiré.
- Démarrage en `self_signed` : l'autorité et le certificat sont créés, et un client qui fait
  confiance à l'autorité se connecte.
- Rechargement à chaud d'un certificat `files`.
- Proxies de confiance : `X-Forwarded-Proto` est ignoré quand il vient d'une adresse non déclarée,
  et le cookie a l'attribut `Secure` derrière un proxy de confiance.
- Idempotence :
  - une même clé rejouée donne une seule intervention et la même réponse ;
  - une clé réutilisée sur une autre route répond 422.
- `X-Occurred-At` :
  - une date dans les bornes est utilisée ;
  - une date hors bornes est remplacée par l'heure de réception et notée dans l'audit.
- 412 sur `If-Unmodified-Since` dépassé.
- Instantané :
  - contenu limité au site et à l'utilisateur ;
  - mode `since` ;
  - taille sur un jeu de 5 000 équipements.

### IHM

Tests manuels documentés, plus des tests automatisés de la logique de file : un fichier
`static/offline.js`, testé sous `node --test`, avec un faux IndexedDB.

- Ordre d'envoi.
- Dépendances sur un identifiant temporaire.
- Passage en « À résoudre ».
- Effacement à la déconnexion.
- Pas d'envoi sous une autre identité.

### Recette sur appareil

- Android (Chrome) et iOS (Safari) :
  - installation de la PWA ;
  - lecture d'un QR ;
  - mode avion ;
  - prise en charge, temps et clôture ;
  - déclaration d'une intervention ;
  - retour du réseau et synchronisation.
- Conflit provoqué depuis un poste de bureau.
