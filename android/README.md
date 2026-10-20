# Apex Maint pour Android

Client natif Kotlin / Jetpack Compose de l’API Apex Maint existante. Aucun changement du serveur Rust ou de `static/`. Les données d’exploitation ne sont jamais utilisées par les tests.

## Ouvrir et construire

Prérequis : Android Studio récent compatible avec AGP 8.13, JDK **17 au minimum** (le JBR d’Android Studio convient), SDK Android **36**, Build Tools **36.0.0**, Platform Tools. Android 8.0 / API 26 minimum sur téléphone ou tablette. Le premier téléchargement des dépendances nécessite un accès à Google Maven et Maven Central.

Ouvrir le dossier **`android/`** dans Android Studio. Déclarer si nécessaire `sdk.dir=C:/Users/.../AppData/Local/Android/Sdk` dans `android/local.properties` (fichier exclu de Git). Choisir le JDK de Gradle dans les paramètres de l’IDE ; le Java 8 éventuellement présent dans le `PATH` ne convient pas.

Le JAR du wrapper est volontairement absent, conformément à la demande. Depuis une installation locale de Gradle **8.13**, dans `android/` :

```powershell
gradle wrapper --gradle-version 8.13 --distribution-type bin
.\gradlew.bat :app:assembleDebug
.\gradlew.bat :app:testDebugUnitTest
```

La génération du wrapper peut remplacer les deux lanceurs minimaux fournis par les scripts officiels. Sous Linux/macOS : `chmod +x gradlew`, puis `./gradlew :app:assembleDebug :app:testDebugUnitTest`.

Brancher un téléphone ou une tablette avec le débogage USB autorisé, puis :

```powershell
adb devices
adb -s IDENTIFIANT install -r app/build/outputs/apk/debug/app-debug.apk
```

Android Studio peut également lancer l’application sur un émulateur téléphone et un émulateur tablette API 36. Les versions release ne sont pas signées : aucune clé ni configuration de signature privée n’est fournie.

## Serveur, certificat et connexion

Au premier démarrage, saisir l’URL **racine HTTPS**, avec son port éventuel, par exemple `https://maint.atelier.local:8443/`. Les chemins, fragments, paramètres et identifiants dans l’URL sont refusés. L’URL est enregistrée dans DataStore.

Pour l’autorité propre à l’instance (`server.tls.mode="self_signed"`), obtenir le PEM public à `/api/tls/ca.pem` auprès de l’administrateur, vérifier son empreinte par un canal connu, puis l’installer dans les paramètres Android : Sécurité / Chiffrement et identifiants / Installer un certificat / Certificat d’autorité. Les intitulés dépendent du constructeur. Le bouton de connexion ouvre cette route dans le navigateur ; il ne contourne pas un certificat encore inconnu. Le nom ou l’adresse de l’URL doit figurer dans le certificat serveur (`self_signed_names`). Les autorités système **et utilisateur** sont acceptées, avec validation TLS et du nom d’hôte normales.

Le HTTP est refusé en release. En debug seulement, `http://localhost:PORT/` et `http://10.0.2.2:PORT/` sont autorisés. `10.0.2.2` désigne l’hôte depuis l’émulateur Android. Utiliser une **instance portable temporaire** et un port tel que **8443 ou 8088**, jamais 3000 ou 3100, et jamais une base d’exploitation.

La connexion utilise `POST /api/auth/login`, puis `GET /api/auth/me`. Le serveur renvoie `apex_maint_session; HttpOnly`, les permissions, le site et `must_change_password`. Le cookie persiste dans des préférences chiffrées **AES-GCM avec clé Android Keystore**, de même que les métadonnées de session nécessaires au hors ligne. Aucun mot de passe n’est stocké ou journalisé. Les sauvegardes Android sont désactivées. Les redirections HTTP sont refusées pour éviter de transmettre des données à une autre origine.

Le contrat actuel n’émet **aucun jeton CSRF distinct**. Son contrôle porte sur `Origin` et `Sec-Fetch-Site`, lorsqu’ils sont présents ; le client natif n’invente pas d’en-tête de jeton. Un changement de mot de passe imposé intercepte la navigation et utilise `/api/auth/password`.

Un administrateur choisit un site de travail parmi `/api/sites`. Les données sont ensuite filtrées explicitement par `site_id`, y compris l’instantané global renvoyé aux administrateurs. Le changement de site efface le cache et est désactivé dans l’interface tant que la file n’est pas vide. Un utilisateur non administrateur conserve le site fixé par le serveur.

Une réponse 401 ramène à la connexion en conservant la file du même compte. Une autre identité entraîne sa purge, avant tout usage. La déconnexion affiche le nombre d’opérations perdues, efface le cookie, les métadonnées, Room et les fichiers temporaires privés. Sans réseau, elle est locale : le serveur ne peut pas recevoir la révocation.

## Écrans

- Configuration du serveur ; connexion ; changement obligatoire du mot de passe ; choix du site.
- Terrain : mes interventions / interventions du site, filtres de statut, détail, prise en charge, remplacement de mon temps total, description, compte rendu, clôture confirmée, sous-interventions en lecture.
- Déclaration : titre, description, équipement recherché dans le cache ou scanné, priorité, état machine (`ARRET`, `DEGRADE`, `FONCTIONNEL`), type. Le type a posteriori ajoute compte rendu et durée ; ses dates sont calculées à partir de la durée et de l’heure courante.
- Catalogue des pièces, recherche, fiche, quantités par emplacement chargées en ligne ; sortie de pièces depuis une intervention et consommations affichées.
- Documents d’une intervention ou d’un équipement ; ouverture avec `ACTION_VIEW`, URI FileProvider et droit de lecture temporaire ; ajout de fichier et capture photo par l’application photo Android.
- Scan CameraX / ML Kit, modèle de détection embarqué, permission caméra à la demande et saisie manuelle de secours. Le QR web est le **code équipement seul**. Les liens de la même instance `/#/equipments/ID` et `/#/interventions/ID` sont également reconnus ; les autres origines sont refusées.
- Synchronisation : file complète, messages métier, comparaison de la modification locale avec l’état distant, nouvelle tentative explicitement choisie ou abandon confirmé.

`currentWindowAdaptiveInfo().windowSizeClass` (Material 3 Adaptive) détermine le mode : barre du bas sur largeur compacte inférieure à **600 dp**, rail et liste/détail simultanés à partir de 600 dp, y compris en paysage. Aucun verrouillage d’orientation ou de taille de texte. Contrôles Material avec cibles d’au moins 48 dp et formulaires défilants. Couleurs du thème clair web : fond `#EEF2F8`, texte `#0F1B2E`, accent `#0284C7`, secondaire `#6366F1`. L’icône vectorielle reprend les tracés et couleurs de `static/icons/icon.svg`.

## Hors ligne et synchronisation

Room contient les instantanés JSON d’équipements, interventions, pièces utiles, lignes, compteurs et utilisateurs, ainsi que les opérations. Les représentations JSON préservent les champs et les valeurs nulles du serveur. Chaque ligne est isolée par **utilisateur + site**. Les fonctions du dépôt reçoivent un `Actor`. Les suppressions de lignes individuelles et abandons utilisent une marque logique ; l’effacement physique est réservé à la purge complète du cache privé.

Les cinq opérations terrain sont projetées localement et mises en file dans une transaction Room. Leur ordre est une position croissante persistante, indépendante de l’horloge et conservée lors d’une résolution. Les envois sont séquentiels, avec :

```text
Idempotency-Key: UUID propre à l’opération
X-Occurred-At: date RFC3339 de l’action
If-Unmodified-Since: updated_at serveur exactement mémorisé
```

Les clés UUID sont conservées lors des reprises réseau. La date locale d’affichage ne remplace jamais la version serveur. Prise en charge, temps, PATCH et clôture portent la précondition initiale. Après acquittement **conditionné**, seules les opérations dépendantes portant **cette même version** sont recalées sur `updated_at` de la réponse. Une réponse non conditionnée ne recale ni les opérations ni la version connue du cache. La réponse de temps est identifiée par `intervention_id`, et non par l’identifiant du technicien.

Une création reçoit `temp_UUID`. Son acquittement réécrit, dans une transaction, le cache et les références URL / `dependsOn` / `body.intervention_id` de toutes ses dépendances, puis initialise leurs préconditions. Une interruption après l’acquittement reprend avec les versions persistées.

Les refus **400, 403, 409, 412 et 422** deviennent « À résoudre » ; seules les opérations sur la même intervention sont bloquées, y compris aux prochaines synchronisations. Une 401 arrête la file sans la supprimer ; un incident réseau ou serveur conserve les opérations pour reprise avec temporisation exponentielle. Une résolution recharge l’état distant, vérifie qu’il correspond encore à la version comparée, conserve l’ordre et attribue une **nouvelle clé**, car le serveur mémorise aussi les refus 4xx. Seule l’opération explicitement choisie reçoit cette version rechargée ; les autres préconditions restent intactes et peuvent nécessiter leur propre résolution. L’abandon d’une création abandonne aussi ses dépendances temporaires. L’abandon d’une modification reconstruit l’affichage à partir de la dernière base serveur et des autres opérations, sans conserver le champ abandonné.

WorkManager déclenche les envois sur réseau disponible, à l’ouverture, après une action et sur demande. Une tâche périodique de 15 minutes complète ce mécanisme ; Android peut différer son exécution. Une exclusion mutuelle commune sérialise session, dépôt et Worker pour empêcher un envoi concurrent ou un changement d’identité pendant un envoi.

Le delta utilise **`GET /api/offline/snapshot?since=...`**, après synchronisation. `full` remplace les collections de référence ; les `deleted_at` deviennent des tombstones ; `intervention_ids` retire les interventions qui ne sont plus visibles. Les techniciens sont remplacés pour chaque intervention reçue, même avec une liste vide. Les modifications locales en attente gardent leur projection jusqu’à résolution. Les pièces consommées, fichiers et photos ne sont pas mis en file hors ligne : ils nécessitent le réseau.

## Routes utilisées et écarts du contrat

| Usage | Routes existantes |
|---|---|
| Session | `POST /api/auth/login`, `GET /api/auth/me`, `POST /api/auth/logout`, `POST /api/auth/password` |
| Sites | `GET /api/sites` |
| Instantané et delta | `GET /api/offline/snapshot[?since=...]` |
| Terrain | `GET/POST /api/interventions`, `GET/PATCH /api/interventions/{id}`, `POST .../{id}/take`, `PUT .../{id}/technicians/{user_id}/time`, `POST .../{id}/complete`, `GET .../{id}/sub-interventions` |
| Équipements / QR | `GET /api/equipments/{id}`, `GET /api/equipments/by-code/{code}` |
| Pièces | `GET /api/parts`, `GET /api/parts/{id}`, `GET/POST /api/interventions/{id}/parts` |
| Documents | `GET /api/documents?entity_type=&entity_id=`, `POST /api/documents` multipart, `GET /api/documents/{id}/download?download=1` |
| Autorité | `GET /api/tls/ca.pem` dans le navigateur |

Pagination par curseur de toutes les pages du catalogue et des interventions du site (`limit=100`). Les quantités par emplacement viennent de `PartDetail.locations`, sans appel supplémentaire à `/api/stock`.

Absences et limites constatées en lisant les routes Rust et les contrats :

- Pas de `/api/terrain` : le delta existant est `/api/offline/snapshot`.
- Pas de route de changement du site courant d’une session utilisateur ni de liste d’affectations multi-sites d’un compte. Le choix administrateur est un filtre de travail client, pas une mutation de session. Pour les autres comptes, cette fonction attend un contrat serveur.
- Pas de route générant un QR d’intervention ni de format QR d’intervention défini par le web. Le client lit les liens de la même instance en complément du format équipement existant.
- `/api/settings/documents` exige `settings.view` et ne publie pas le plafond effectif `min(documents.max_size_mb, files.max_size_mb)`. Sans ajouter de route, le client utilise une limite conservatrice de **1 Mio**, minimum autorisé par les deux réglages. Le serveur valide les types et leur signature ; ses erreurs 413/415 sont affichées. Les photos sont réduites avant envoi (échantillonnage jusqu’à 1600 px environ, JPEG qualité 80), et restent refusées si elles dépassent 1 Mio. Une route publique aux comptes `documents.create` donnant les limites effectives améliorerait ce parcours.

## Organisation et versions

```text
android/
  settings.gradle.kts, build.gradle.kts, gradle.properties
  gradlew, gradlew.bat, gradle/wrapper/gradle-wrapper.properties
  gradle/libs.versions.toml
  app/build.gradle.kts
  app/src/main/AndroidManifest.xml
  app/src/main/java/fr/apex/maint/
    MaintApplication (dans AppContainer.kt), AppContainer, SyncWorker
    MainActivity
    data/  Api, Models, Security, Settings, Database, Queue, LocalProjection, Repository, Documents
    ui/    MaintViewModel, Screens, ScannerScreen
  app/src/main/res/  drawable/ic_apex.xml, xml/...
  app/src/debug/res/xml/network_security_config.xml
  app/src/test/java/fr/apex/maint/data/  QueueProtocolTest, LocalProjectionTest, ApiTest
```

Toutes les versions sont figées dans `gradle/libs.versions.toml` :

| Composant | Version |
|---|---|
| AGP / Gradle | 8.13.2 / 8.13 |
| Kotlin et compilateur Compose | 2.2.21 |
| KSP | 2.2.21-2.0.4 |
| Compose BOM / Adaptive | 2025.12.00 / 1.2.0 |
| Activity / Core | 1.12.1 / 1.17.0 |
| Navigation Compose / Lifecycle | 2.9.6 / 2.10.0 |
| Room / WorkManager | 2.8.4 / 2.11.0 |
| OkHttp / Retrofit | 4.12.0 / 3.0.0 |
| kotlinx.serialization / coroutines | 1.9.0 / 1.10.2 |
| CameraX / ML Kit barcode | 1.5.2 / 17.3.0 |
| DataStore / JUnit | 1.2.0 / 4.13.2 |

Repères officiels : [compatibilité AGP et Gradle](https://developer.android.com/build/releases/agp-8-13-0-release-notes), [KSP pour Kotlin 2.2.21](https://github.com/google/ksp/releases/tag/2.2.21-2.0.4), [BOM Compose](https://developer.android.com/develop/ui/compose/bom), [WindowSizeClass](https://developer.android.com/reference/androidx/window/core/layout/WindowSizeClass). Ce socle figé reste à résoudre et compiler dans l’environnement Android cible.

## Vérifications et première recette

**Aucun build Gradle n’a été exécuté**, conformément à la consigne. **22 tests du cœur de file et des projections passent sur JVM**, via le compilateur Kotlin 2.3.10 et les bibliothèques préinstallés dans Android Studio, avec un lanceur local des méthodes JUnit et de leurs exceptions attendues. Les annotations Room sont remplacées uniquement dans ce harnais, sans exécuter Room. Cela ne valide pas la résolution du catalogue Gradle ni l’intégration Android. Les six tests `ApiTest`, dont MockWebServer, restent à exécuter avec Gradle. L’analyse syntaxique de toutes les sources Kotlin et des scripts Kotlin DSL ne signale aucune erreur ; les cinq XML sont bien formés. Cette analyse ne vérifie pas les types Android ni les appels Compose.

Les tests JVM fournis utilisent le même moteur séquentiel et les mêmes transformations de file que la production : ordre, deux PATCH, reprise après coupure, création puis deux PATCH, réécriture temporaire, prise / temps / clôture, acquittement sans précondition, vraie concurrence, blocage ciblé et persistant, isolation d’identité et de site, session expirée, incident serveur, en-têtes et sérialisation. MockWebServer teste le contrat Retrofit, les corps JSON, cookies HttpOnly, préconditions, erreurs et réponses 204. Ils n’utilisent ni appareil ni serveur Apex.

Au premier build et sur instances portables temporaires :

1. Générer le JAR, résoudre les dépendances, exécuter `:app:assembleDebug`, `:app:testDebugUnitTest` puis `:app:lintDebug`. Vérifier la génération Room/KSP et conserver le schéma Room généré dans `app/schemas/`.
2. Téléphone et tablette API 26 / 36 : portrait, paysage, redimensionnement, taille de police 200 %, clavier, navigation retour et cibles tactiles.
3. Certificat utilisateur, nom d’hôte, refus d’une autorité inconnue, refus HTTP release ; connexion, mot de passe imposé, 401, changement de compte et de site, purge à la déconnexion.
4. Mode avion : créer, prendre, saisir deux fois le temps total, modifier, clôturer ; relancer le processus ; rétablir le réseau et vérifier une seule écriture par clé.
5. Depuis un second poste, modifier une intervention pour provoquer 412 ; vérifier la comparaison, le blocage des dépendances, la nouvelle tentative et l’abandon.
6. Quantités par emplacement, sortie de pièces et refus 403/409 ; documents autorisés/refusés, lecteur absent, capture photo et permission caméra refusée, QR équipement et lien d’intervention.

Limites restantes : recette Android et compilation non réalisées ; tests JVM sans intégration Room/Keystore/WorkManager ; pas d’export de file à la déconnexion ; comparaison des conflits sans éditeur de fusion intégré ; lecture hors ligne des sous-interventions limitée aux données disponibles ; pièces, pièces jointes et photos en ligne seulement ; historique récent des opérations acquittées non affiché. La tâche de fond dépend des restrictions de batterie du système. Les textes ne passent pas par HTML : `esc()` et `jsArg()` ne s’appliquent pas à Compose.
