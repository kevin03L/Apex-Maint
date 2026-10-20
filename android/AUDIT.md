# Audit Android — 20 octobre 2026

Revue des sources Kotlin de production et des tests, scripts Gradle et wrapper, manifestes, ressources XML et schéma Room du dossier `android/`. Contrats rapprochés des routes HTTP Rust d’authentification, terrain, interventions, référentiel, stock et documents, des `docs/API-*.md`, de `static/offline.js` et des couleurs de `static/styles.css`. Les références ci-dessous désignent les lignes après correction. Aucun changement dans Rust ou `static/`, aucun commit, aucune branche et aucune utilisation de base d’exploitation.

La revue est statique : aucun constat ne vaut certification de fonctionnement sur Android. Le build demandé est bloqué avant compilation par les permissions du bac à sable.

## Constats et corrections par gravité

Aucun défaut critique confirmé lors de cette revue statique. Les défauts suivants ont été corrigés dans les sources ; leur validation Android reste à faire.

| Gravité | Référence | Constat et correction |
|---|---|---|
| Haute | `app/src/main/java/fr/apex/maint/data/Database.kt:36`, `data/PayloadCipher.kt:9`, `data/Repository.kt:12` | Les descriptions, rapports et corps de file étaient en clair dans Room. Adaptateur chiffrant les instantanés, corps et erreurs en AES-GCM ; contexte authentifié par compte, site et ligne. Conversion transactionnelle de l’ancien cache, tombstones compris, sans supprimer la file ; checkpoint WAL et compactage. Les identifiants, dates et index techniques restent lisibles. |
| Haute | `app/src/main/java/fr/apex/maint/data/Security.kt:16` | Une erreur de déchiffrement supprimait silencieusement les métadonnées, risquant une purge au prochain changement d’identité. L’erreur conserve désormais les données. Les erreurs du CookieJar sont converties en erreurs d’E/S traitables par OkHttp. La clé Keystore est chargée par une initialisation partagée et synchronisée, évitant des générations concurrentes. |
| Haute | `app/src/main/java/fr/apex/maint/AppContainer.kt:115` | Le blocage d’un changement de site avec file non vide existait seulement dans l’interface. Vérification ajoutée sous le verrou commun avant toute purge. La sélection du site déjà courant conserve son cache. |
| Haute | `app/src/main/java/fr/apex/maint/data/Repository.kt:59` | L’instantané restreint pouvait retirer une intervention portant encore une projection locale ou un conflit. Les références protégées par la file sont conservées. Les listes paginées mettent aussi à jour la base serveur des entités protégées, sans écraser leur projection. |
| Haute | `app/src/main/java/fr/apex/maint/AppContainer.kt:65`, `ui/MaintViewModel.kt:219` | Les sorties de pièces n’ont pas d’idempotence dans l’API ; une répétition transparente du transport était indésirable. Répétitions OkHttp désactivées. Une coupure pendant une sortie invite à recharger les consommations avant de répéter. Cette ambiguïté ne peut pas être totalement supprimée côté client. |
| Moyenne | `app/src/main/java/fr/apex/maint/AppContainer.kt:150`, `data/Repository.kt:79` | Synchroniser n’actualisait que l’instantané, limité aux affectations pour les non-admins. Après l’envoi, rechargement de toutes les pages des interventions du site et du catalogue autorisé. Détection des curseurs répétés et retrait des anciennes entrées uniquement après réception complète des pages. |
| Moyenne | `app/src/main/java/fr/apex/maint/data/Queue.kt:65`, `data/Repository.kt:150` | Une prise en charge avec `{}` n’ajoute pas forcément le technicien connecté quand des affectations existent. Envoi explicite de `technician_ids: [actor.userId]`. La projection locale correspond ainsi à la demande serveur. |
| Moyenne | `app/src/main/java/fr/apex/maint/data/LocalProjection.kt:19`, `data/Repository.kt:123`, `data/TerrainRules.kt:19` | La projection assimilait un temps non renseigné à zéro saisi. Conservation de `null` à la création/prise ; la clôture exige un rapport et au moins un temps réellement renseigné, zéro compris, et signale les sous-interventions actives connues. Le serveur revérifie la clôture. |
| Moyenne | `app/src/main/java/fr/apex/maint/data/TerrainRules.kt:7` | Les formulaires pouvaient ajouter des données manifestement refusables à la file. Vérification du titre sur 128 caractères Unicode, description 4 096, compte rendu 16 384 et corps JSON UTF-8 sur 64 Kio, avant enregistrement. Compteurs et erreurs visibles dans les champs. |
| Moyenne | `app/src/main/java/fr/apex/maint/data/Queue.kt:48`, `ui/MaintViewModel.kt:56` | `password_change_required` pouvait devenir un conflit métier. La file est arrêtée sans mutation ; l’action au premier plan oriente vers le changement de mot de passe. La synchronisation contrôle déjà `/auth/me` avant d’envoyer. |
| Moyenne | `app/src/main/java/fr/apex/maint/data/Queue.kt:75` | Un identifiant vide ou encore temporaire dans un acquittement pouvait faire perdre une création. Une telle réponse est maintenant refusée et l’opération conservée. Les règles existantes de chaînage conditionné sont maintenues. |
| Moyenne | `app/src/main/java/fr/apex/maint/ui/MaintViewModel.kt:34`, `ui/Screens.kt:98` | Navigation et brouillons fragiles après recréation ; filtres réinitialisés en ouvrant une fiche. `SavedStateHandle` pour filtres et équipement, navigation et positions de listes isolées par compte/site, brouillons textuels chiffrés dans l’état sauvegardé. Les mots de passe restent en mémoire seulement. |
| Moyenne | `app/src/main/java/fr/apex/maint/ui/MaintViewModel.kt:145`, `ui/Screens.kt:98` | Les chargements pouvaient remplacer une sélection plus récente ; une actualisation réinitialisait les champs et leur position. Annulation du chargement précédent, compteur des travaux actifs, conservation de la sélection sur rechargement de la même fiche, réconciliation des champs uniquement si le brouillon est inchangé. Les retours d’actions ne réouvrent plus une fiche quittée. |
| Moyenne | `app/src/main/java/fr/apex/maint/ui/MaintViewModel.kt:186`, `data/Api.kt:27` | L’échec documentaire interrompait les rubriques suivantes ; une ressource indisponible restait affichée « Chargement ». Chargements annexes indépendants, 401 toujours propagée, erreurs en snackbar, états indisponibles avec reprise ; explications françaises pour 410/413/415 et autres erreurs sans message JSON. |
| Moyenne | `app/src/main/java/fr/apex/maint/data/Documents.kt:30`, `ui/Screens.kt:591` | La photo convertie en JPEG pouvait garder un nom ou MIME du fournisseur d’origine. Nom `photo.jpg` et MIME JPEG explicites ; nom de fichier nettoyé et MIME invalide remplacé par le type binaire. Nettoyage de la photo annulée ou sans application de capture. |
| Moyenne | `app/src/main/java/fr/apex/maint/ui/MaintViewModel.kt:250`, `ui/ScannerScreen.kt:69` | Comparaison de ports URI non normalisés, codes contenant `:` pris pour des liens, risque de trame non libérée si ML Kit échoue immédiatement. Comparaison d’origine via HttpUrl, refus des identifiants dans les liens, reconnaissance des liens avec `://`, fermeture de la trame sur erreur synchrone ; brouillon de saisie manuelle chiffré. |
| Moyenne | `app/src/main/java/fr/apex/maint/AppContainer.kt:50`, `AppContainer.kt:171` | État réseau basé seulement sur l’arrivée/perte de réseau et exceptions de Worker non traitées. Suivi des changements de capacités ; erreurs inattendues du Worker reprises sans journaliser les contenus, annulation des coroutines conservée. |
| Faible | `app/src/main/AndroidManifest.xml:8`, `res/xml/data_extraction_rules.xml:1` | Sauvegardes déjà désactivées ; exclusions explicites ajoutées pour sauvegarde et transfert d’appareil Android 12+. Le FileProvider reste non exporté et limité aux deux sous-dossiers privés prévus. |
| Faible | `app/src/main/java/fr/apex/maint/ui/Screens.kt:160`, `ui/Presentation.kt:8` | Bandeau permanent, pluriels techniques, dates brutes, couleurs Material par défaut étrangères au thème web. Bandeau conditionnel, formats français, puces accompagnées de texte et palette cohérente. Accent assombri pour le contraste du texte blanc. |

Contrôles existants conservés : cookie HttpOnly chiffré ; absence de mot de passe persistant et de journalisation applicative des corps/secrets ; refus des redirections et contrôle d’origine de chaque requête ; HTTP limité aux deux hôtes debug ; confiance TLS système/utilisateur sans désactiver la validation du nom ; permission caméra à la demande ; composants privés, sauf activité lanceur ; accès Room suspendus hors du fil principal.

Le protocole conserve `Idempotency-Key`, `X-Occurred-At` et la version serveur exacte pour PATCH, prise, temps et clôture. Les `temp_` et dépendances sont réécrits transactionnellement. Seuls une création ou un acquittement conditionné recalent les opérations dépendantes portant la même version. La réponse de temps utilise `intervention_id`. Les refus 400/403/409/412/422 bloquent uniquement l’intervention concernée ; une 401 arrête les envois sans supprimer la file. Nouvelle clé après résolution, ordre persistant conservé et purge lors d’un changement d’identité restent en place.

## Revue UI/UX écran par écran

| Écran | Changements réalisés |
|---|---|
| Configuration | Champ vide avec libellé d’adresse HTTPS complète, exemple avec port, explication de l’URL racine et carte distincte pour le certificat. Validation avant enregistrement. |
| Connexion | Formulaire de largeur lisible, brouillon d’identifiant chiffré, affichage/masquage du mot de passe avec description accessible, confirmation de connexion et erreurs en snackbar. Session expirée et installation de CA expliquées. |
| Mot de passe imposé | Champs secrets non sauvegardés, visibilité explicite, validation de confirmation et snackbar de succès. Formulaire défilant avec gestion du clavier. |
| Choix du site | Choix borné aux sites servis, purge sous verrou, blocage de file non vide dans le conteneur. Cache conservé si le site est déjà sélectionné. |
| Navigation générale | Synchronisation accessible dans la barre même lorsque le bandeau est masqué. Bandeau réservé au hors ligne ou à la file non vide, accès à la file/conflits, pluriels corrects, retour sur les fiches, accueil selon permissions et navigation remise à zéro au changement de compte/site. |
| Terrain | Synchroniser = envoyer puis actualiser ; tirer-pour-rafraîchir ; bouton séparé de rechargement du site ; « Mes interventions » accessible sur toute la ligne ; filtres persistants ; état vide avec action vers le site et retrait du filtre ; listes paresseuses, sélection visible et position conservée. |
| Détail intervention | Cartes actions principales, informations, compte rendu, temps, pièces, documents et sous-interventions. Prise en charge ou clôture selon statut, confirmation avec rapport, prérequis de clôture expliqués, temps personnel réservé aux affectés, compte rendu éditable selon rôle/affectation et droits. Accès à l’équipement et ses documents. Brouillons conservés, champs multilignes bornés en hauteur, marge clavier et indicateur de progression de hauteur constante. |
| Déclaration | Cartes déclaration/équipement/travaux réalisés, type hors équipement sans sélection superflue, retrait d’équipement, sélection issue du scan conservée via le ViewModel, compteurs de caractères, clavier numérique pour la durée, formulaire remis à zéro après création effective locale. |
| Catalogue et fiche pièce | Recherche, état vide utile, rafraîchissement au geste, liste paresseuse, disponibilité et ventilation par emplacement regroupées en cartes, indications sur les quantités mémorisées, reprise d’un chargement échoué. |
| Sortie de pièce | Contrôle d’accès catalogue, états de sélection/emplacement/quantité restaurables, boutons désactivés pendant les demandes, détail de pièce rechargé à la réouverture, clavier numérique, confirmation et retour de succès/erreur. |
| Documents/photos | Rubrique explicite dans l’intervention et l’équipement, états vide/hors ligne/droits insuffisants, icônes, boutons d’ouverture pleine largeur, rechargement documentaire, ajout/capture réservés au réseau et à l’identifiant réel. |
| Équipement | Statut coloré et libellé, description, déclaration liée à l’équipement, documents avant les interventions associées, état vide et reprise d’erreur. |
| Scanner | Même largeur maximale et marge clavier que les formulaires ; saisie manuelle restaurable et chiffrée ; permission seulement à l’action, pause/reprise et secours manuel conservés ; libération du cycle caméra à la sortie et des trames en erreur. |
| Synchronisation/conflits | Dates françaises, titre d’intervention au lieu du seul identifiant, comparaison locale/distante, nouvelle tentative explicite et abandon confirmé conservés ; erreurs réseau et réussites au premier plan en snackbar. |
| Tablette et paysage ≥ 600 dp | NavigationRail défilant, listes/détails simultanés pour interventions et pièces, séparateur, sélection visible, position conservée et formulaires plafonnés à 680 dp. À police agrandie, libellés et boutons peuvent grandir ; pas de verrouillage d’orientation. |

Les contrôles Material conservent leurs cibles tactiles usuelles ; les actions et lignes ajoutées visent au moins 48 dp. Les icônes décoratives ont une description nulle et les commandes iconographiques seules ont un libellé. Les titres des cartes sont signalés comme titres sémantiques. Couleur et texte accompagnent ensemble les statuts/priorités. La vérification réelle TalkBack, contraste de tous les états, police 200 %, IME et dimensions téléphone/tablette reste indispensable.

## Tests et vérifications

**52 tests JVM déclarés, 24 ajoutés. Aucun résultat de réussite Gradle n’est revendiqué pour cette révision.**

| Fichier dans `app/src/test/java/fr/apex/maint/` | Total | Ajouts |
|---|---:|---|
| `data/QueueProtocolTest.kt` | 19 | 3 : acquittements invalides, mot de passe imposé sans conflit, affectation explicite de la prise. |
| `data/LocalProjectionTest.kt` | 7 | 1 : prise avec temps non renseigné. |
| `data/ApiTest.kt` | 8 | 2 : routes/corps/en-têtes prise-temps-clôture et erreurs documentaires sans JSON. |
| `data/PayloadCipherTest.kt` | 6 | 6 : aller-retour, contenu illisible et IV distincts, compte/site/clé différents, troncature et refus de clair. |
| `data/EncryptedDaoTest.kt` | 3 | 3 : contenus et erreurs chiffrés, ancien cache/tombstones, reprise avec nouvelle clé d’idempotence. DAO mémoire, sans Room réel. |
| `data/TerrainRulesTest.kt` | 5 | 5 : temps null/zéro, rapport/statut, enfants actifs, limites Unicode et octets JSON. |
| `ui/PresentationTest.kt` | 4 | 4 : dates locales françaises, dates invalides, durées, singulier/pluriel. |

Commande essayée depuis `android/`, avec `JAVA_HOME=C:\Program Files\Android\Android Studio\jbr` :

```powershell
.\gradlew.bat --offline assembleDebug testDebugUnitTest lintDebug
```

Échec du wrapper avant toute tâche : `java.nio.file.AccessDeniedException` sur `C:\Users\Lerou\.gradle\wrapper\dists\gradle-8.13-bin\5xuhj0ry160q40clulazy9h7d\gradle-8.13-bin.zip.lck`. Aucun changement de cache, de permissions ou de distribution n’a été tenté pour contourner le refus. La compilation, les 52 tests et lint doivent être relancés par l’utilisateur.

Contrôles indépendants réussis : analyse syntaxique des 28 fichiers Kotlin/Kotlin DSL par le parseur Kotlin installé avec Android Studio, sans compilation de l’application ni résolution des types ; six ressources/manifestes XML bien formés ; `git diff --check` sans défaut d’espacement. Aucun test serveur Rust/JavaScript n’a été lancé puisque leurs fichiers sont inchangés.

## Points restants

- Compiler et exécuter la suite puis lint ; valider les appels Compose et la génération Room/KSP, que l’analyse syntaxique ne vérifie pas.
- Faire la recette visuelle téléphone 1080×2400, tablette, paysage, 600 dp, clavier, taille de police 200 % et TalkBack. Aucune capture ni validation visuelle de cette révision n’a été effectuée ici.
- Tester rotation/restauration de processus, conversion d’une ancienne installation, interruption pendant conversion/acquittement, Keystore réel, cookie réellement persistant et Worker Android, avec comptes/sites de test seulement.
- Le chiffrement des contenus ne chiffre pas les index techniques SQLite. Checkpoint/VACUUM ne constituent pas une garantie d’effacement physique des anciennes pages sur mémoire flash. Une clé Keystore perdue ne peut pas être reconstituée par l’application.
- Les documents ouverts et photos utilisent temporairement des fichiers privés en clair pour les applications Android externes. Les téléchargements restent en cache jusqu’à la déconnexion. Les photos d’une capture interrompue peuvent également y rester jusqu’à la purge. Aucun de ces fichiers n’entre dans la file hors ligne.
- Une sortie de pièce avec réponse réseau perdue reste ambiguë sans idempotence serveur : consultation des consommations avant répétition. Aucun nouveau contrat serveur n’a été inventé.
- Les brouillons non validés survivent à la restauration d’état Android, mais ne remplacent pas une sauvegarde métier. La file Room reste la référence des actions validées. Lecture hors ligne des sous-interventions limitée aux données déjà chargées.
- Limites préexistantes conservées : pas d’export de file, pas d’éditeur de fusion des conflits, pas d’historique des opérations acquittées ; tâche de fond soumise aux restrictions système ; pièces jointes limitées à 1 Mio en l’absence de route de limites accessible à tous les comptes autorisés.

La liste de recette complète et les commandes sont dans [README.md](README.md).
