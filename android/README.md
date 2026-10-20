# Apex Maint pour Android

Client natif Kotlin / Jetpack Compose de l’API Apex Maint existante. Aucun changement du serveur Rust ou de `static/`. Les données d’exploitation ne sont jamais utilisées par les tests.

## Ouvrir et construire

Prérequis : Android Studio récent compatible avec AGP 8.13, JDK **17 au minimum** (le JBR d’Android Studio convient), SDK Android **36**, Build Tools **36.0.0**, Platform Tools. Android 8.0 / API 26 minimum sur téléphone ou tablette. Le premier téléchargement des dépendances nécessite un accès à Google Maven et Maven Central.

Ouvrir le dossier **`android/`** dans Android Studio. Déclarer si nécessaire `sdk.dir=C:/Users/.../AppData/Local/Android/Sdk` dans `android/local.properties` (fichier exclu de Git). Choisir le JDK de Gradle dans les paramètres de l’IDE ; le Java 8 éventuellement présent dans le `PATH` ne convient pas.

Le wrapper Gradle **8.13** est fourni. Depuis `android/`, avec les dépendances déjà en cache :

```powershell
$env:JAVA_HOME = 'C:\Program Files\Android\Android Studio\jbr'
.\gradlew.bat --offline assembleDebug testDebugUnitTest lintDebug
```

Sous Linux/macOS : `chmod +x gradlew`, puis `./gradlew --offline assembleDebug testDebugUnitTest lintDebug`. Si le cache n’est pas encore constitué, un premier build connecté est nécessaire.

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

`currentWindowAdaptiveInfo().windowSizeClass` (Material 3 Adaptive) détermine le mode : barre du bas sur largeur compacte inférieure à **600 dp**, rail et liste/détail simultanés à partir de 600 dp, y compris en paysage. Aucun verrouillage d’orientation ou de taille de texte. Contrôles Material avec cibles d’au moins 48 dp et formulaires défilants. Couleurs du thème clair web : fond `#EEF2F8`, texte `#0F1B2E`, accent `#0369A1` (bleu web assombri pour le contraste du texte blanc), secondaire `#6366F1`. L’icône vectorielle reprend les tracés et couleurs de `static/icons/icon.svg`.

## Hors ligne et synchronisation

Room contient les instantanés JSON d’équipements, interventions, pièces utiles, lignes, compteurs et utilisateurs, ainsi que les opérations. Les représentations JSON préservent les champs et les valeurs nulles du serveur. Chaque ligne est isolée par **utilisateur + site**. Les fonctions du dépôt reçoivent un `Actor`. Les suppressions de lignes individuelles et abandons utilisent une marque logique ; l’effacement physique est réservé à la purge complète du cache privé.

Les contenus JSON, corps d’opération et messages de conflit sont chiffrés AES-GCM avec la clé Keystore ; seuls les identifiants, index, états et dates techniques restent lisibles par SQLite. Le contexte authentifié inclut le compte, le site et l’identifiant de ligne. L’ancien cache est converti dans une transaction, sans abandonner la file, puis compacté avec checkpoint WAL et `VACUUM`. Le schéma Room reste inchangé. Une erreur de lecture du stockage sécurisé conserve les données et demande une intervention ; elle ne supprime plus silencieusement les métadonnées. Les exclusions de sauvegarde couvrent aussi le transfert d’appareil Android 12 et ultérieur.

La navigation et les listes sont isolées par compte et site. Les filtres et l’équipement du formulaire passent par `SavedStateHandle`. Les brouillons textuels utilisent un `Saver` chiffré avant leur passage dans l’état sauvegardé Android ; ils survivent à la rotation et à la restauration du processus par Android. Ils ne constituent pas un enregistrement métier : seule la validation ajoute une opération dans Room. Les mots de passe restent uniquement en mémoire et doivent être ressaisis après recréation de l’écran.

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

**Synchroniser** signifie envoyer la file, appliquer l’instantané puis recharger toutes les pages des interventions du site et du catalogue autorisé. L’instantané seul est limité aux affectations du compte non administrateur. Le geste tirer-pour-rafraîchir des listes lance cette même synchronisation ; le bouton distinct **Recharger les interventions du site** recharge uniquement cette liste. Les projections encore en attente ou en conflit sont préservées. Une pagination répétant son curseur est arrêtée ; le nettoyage des anciennes entrées n’a lieu qu’après réception de toutes les pages.

La prise en charge envoie explicitement `technician_ids: [utilisateur_connecté]`, car un corps vide n’affecte le compte que si l’intervention n’a aucun technicien. Un temps `null` reste non renseigné ; enregistrer zéro minute est une saisie valide. La clôture est confirmée, exige un compte rendu et au moins un temps enregistré, et signale les sous-interventions actives connues. Le serveur conserve tous ses contrôles. Les limites locales suivent les textes du contrat : titre 128 caractères, description 4 096, compte rendu 16 384 et corps JSON 64 Kio.

Les répétitions automatiques de transport OkHttp restent désactivées. Les cinq opérations de la file reprennent par WorkManager avec leur clé d’idempotence. Une sortie de pièce reste une action en ligne, désormais protégée par `Idempotency-Key` côté serveur : une tentative est enregistrée **avant envoi** dans `SecureStore` chiffré (compte/site, route, corps et UUID). Après coupure, renvoyer la même demande réutilise sa clé, y compris après recréation du client ; après succès, une nouvelle validation reçoit une nouvelle clé. Toute erreur garde la tentative : un refus métier mémorisé sera donc rejoué avec cette clé, et modifier le corps constitue une autre demande. Déconnexion et changement de compte/site purgent ces tentatives. Le serveur conserve les clés trente jours ; au-delà, il faut vérifier les consommations avant toute reprise ancienne. Voir [le contrat stock](../docs/API-PHASE-4.md) et sa migration 0016. Le web reste inchangé et ne bénéficie pas de cette reprise persistante.

Les copies nécessaires aux applications externes restent temporairement en clair dans **`cache/external/`**, seul dossier exposé par le FileProvider non exporté, avec des noms UUID et sans sauvegarde. Elles sont supprimées à la reprise de l’activité, à la déconnexion et au changement de compte/site. Le démarrage purge tout fichier âgé d’au moins **cinq minutes**, y compris les anciens dossiers `documents/` et `photos/`. Une capture réussie est déplacée avant la reprise de l’activité dans `cache/document_work/`, inaccessible au FileProvider ; sa source est supprimée dès la copie de travail créée et le travail est supprimé en fin d’envoi, succès ou erreur. Une capture annulée ou impossible est supprimée immédiatement, ainsi qu’un téléchargement qu’aucun lecteur ne peut ouvrir. Aucun document ni photo n’est mis en file hors ligne : aucun fichier en clair n’y est conservé. Une application externe peut toutefois garder sa propre copie ou un descripteur ouvert ; la suppression ne garantit pas l’effacement physique sur mémoire flash.

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
    data/  Api, Models, Security, PayloadCipher, Settings, Database, Queue, LocalProjection, TerrainRules, Repository, Documents, StockConsumption, TemporaryFiles
    ui/    MaintViewModel, Screens, ScannerScreen, Presentation
  app/src/main/res/  drawable/ic_apex.xml, xml/...
  app/src/debug/res/xml/network_security_config.xml
  app/src/test/java/fr/apex/maint/
    data/  QueueProtocolTest, LocalProjectionTest, ApiTest, PayloadCipherTest, EncryptedDaoTest, TerrainRulesTest, StockConsumptionTest, TemporaryFilesTest
    ui/    PresentationTest
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

## Vérifications et recette après audit du 20 octobre 2026

L’audit et ses corrections sont détaillés dans [AUDIT.md](AUDIT.md). Le complément corrige l’idempotence du stock côté Rust/Android et la durée de vie des fichiers temporaires. Aucun serveur ni base d’exploitation n’a été lancé et aucun fichier `static/` n’a été modifié. Les tests serveur utilisent uniquement leurs instances portables temporaires.

La commande demandée a été essayée avec le JBR d’Android Studio :

```powershell
.\gradlew.bat --offline assembleDebug testDebugUnitTest lintDebug
```

Elle est bloquée **avant compilation**, par `AccessDeniedException` sur `C:\Users\Lerou\.gradle\wrapper\dists\gradle-8.13-bin\5xuhj0ry160q40clulazy9h7d\gradle-8.13-bin.zip.lck`. Aucun contournement du cache ou des permissions n’a été tenté. **Ni assembleDebug, ni testDebugUnitTest, ni lintDebug ne sont donc validés pour cette révision.** Les tests antérieurement exécutés par un harnais local ne constituent pas une validation de cette révision.

La suite compte maintenant **57 tests JVM déclarés**, répartis entre protocole de file (19), projections (7), API / MockWebServer (8), chiffrement AES-GCM (6), adaptation du DAO chiffré avec un DAO mémoire (3), règles de formulaire et de clôture (5), présentation française (4), sorties idempotentes / MockWebServer (2) et fichiers temporaires (3). Les cinq tests du complément vérifient la même clé après coupure et recréation, une nouvelle clé après succès, l’isolation compte/site/corps, le seuil de purge avec horloge injectable, la reprise, le déplacement d’une capture et le périmètre de nettoyage. Aucun ne contacte un serveur Apex. Gradle a de nouveau été tenté avec le JBR indiqué et reste bloqué sur la même ACL avant toute compilation.

La revue initiale avait analysé la syntaxe de 28 sources Kotlin/Kotlin DSL sans vérifier les types Android/Compose ; cette analyse n’a pas été réexécutée pour le complément. Pour ce complément : **6 XML** bien formés, `git diff --check`, `cargo fmt --all` et Clippy avec avertissements interdits réussis. La suite `cargo test --workspace --no-fail-fast` compile mais reste partiellement en échec dans le bac à sable : ACL Windows sur fichiers secrets/de contrôle et génération TLS, dont le nouveau test stock bloqué au démarrage. Aucun contournement tenté. La compilation Room/KSP et les tests/lint Android restent à exécuter hors du bac à sable. Aucun fichier JS modifié.

Recette à réaliser sur instances portables temporaires, sur des ports autres que 3000 et 3100 :

1. Relancer la commande Gradle ci-dessus. Vérifier Room/KSP et lint, puis installer le nouvel APK.
2. Téléphone 1080×2400 et tablette API 26 / 36 : portrait, paysage, largeur 600 dp, clavier, police 200 %, TalkBack, retour et cibles tactiles. Vérifier rail défilant, sélection et position conservées dans les listes, largeur des formulaires plafonnée à 680 dp.
3. Vérifier configuration, CA utilisateur et refus TLS/HTTP release ; connexion, mot de passe imposé, 401, changement de compte et de site. Une file non vide bloque désormais le changement de site dans le conteneur aussi.
4. Synchroniser et comparer au site web : toutes les pages du site doivent être chargées, sans filtre d’affectation serveur supplémentaire. Tester tirer-pour-rafraîchir et le bouton distinct de rechargement du site. Vérifier états vides, filtrage personnel et bandeau absent en ligne avec file vide.
5. En mode avion : créer, prendre, saisir deux fois le temps total, modifier, clôturer ; restaurer le processus et vérifier brouillons, sélection d’équipement et file ; rétablir le réseau et vérifier une écriture par clé. Tester explicitement zéro minute enregistré.
6. Depuis un second poste : provoquer 412 ; comparer, réessayer avec une nouvelle clé ou abandonner. Vérifier que seules les dépendances de la même intervention sont bloquées et que les projections locales sont conservées.
7. Vérifier le cache Android existant avant/après conversion vers les contenus chiffrés ; provoquer une interruption et une indisponibilité de Keystore sur une installation de test. Valider transactions Room, checkpoint/VACUUM et exclusions de transfert d’appareil.
8. Vérifier consommations et erreurs 403/409 ; documents depuis l’intervention et l’équipement ; erreur documentaire n’empêchant pas les autres rubriques ; permission caméra refusée, absence d’application photo/lecteur, photo annulée, photo JPEG normalisée et QR de la même origine.
9. Couper le réseau après réception d’une sortie par le serveur : répéter exactement la demande, puis après recréation du processus, et vérifier un seul mouvement. Après succès, valider à nouveau : nouvelle clé et nouvelle consommation. Vérifier la migration 0016 sur une base portable et le refus de réutilisation avec un autre corps/utilisateur/route.
10. Ouvrir un document puis revenir : fichier source supprimé et URI devenue illisible. Tester photo annulée, lancement en échec, capture réussie avec retour immédiat, refus d’envoi, rotation et restauration pendant capture/envoi ; contrôler le déplacement avant purge de reprise. Vérifier aussi la suppression au changement de compte/site et la purge de cinq minutes au démarrage, anciens dossiers compris.

Limites explicites : aucune recette visuelle sur émulateur ni compilation de cette révision ; tests JVM avec DAO mémoire, sans moteur Room, Keystore réel, WorkManager ni test Compose instrumenté ; lecture hors ligne des sous-interventions limitée au cache disponible ; documents/photos et sorties de pièces en ligne seulement ; copies externes temporairement en clair, copie éventuelle par le lecteur impossible à rappeler ; travail privé interrompu purgé à un démarrage ultérieur s’il a au moins cinq minutes ; idempotence limitée aux trente jours de conservation serveur, anciennes clés terrain sans empreinte jusqu’à leur purge ; pas d’export de file ni d’éditeur de fusion ; pas d’historique des opérations acquittées. L’effacement physique sécurisé sur mémoire flash ne peut pas être garanti. Les brouillons non validés dépendent de la restauration d’état Android et ne remplacent pas la file persistante. La tâche de fond reste soumise aux restrictions de batterie du système.
