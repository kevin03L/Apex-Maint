# Audit du dépôt Apex Maint

Date : 19 octobre 2026. Périmètre : état présent du dépôt, y compris les fichiers non commités. Les chemins ci-dessous sont relatifs à `D:\Vs-code\Apex-Maint`.

Les règles d’AGENTS.md, les décisions, les spécifications des phases et les contrats HTTP ont servi de référence. Aucun fichier source, configuration ou donnée d’exploitation n’a été modifié par l’audit. Les seules écritures demandées sont ce rapport et les sorties des commandes Cargo autorisées.

**Bilan : 0 critique, 6 hautes, 7 moyennes, 2 basses.** Les constats résultent de la lecture des chemins de code indiqués ; les reproductions destructrices ou susceptibles d’épuiser la mémoire n’ont pas été exécutées.

Vérifications exécutées :
- `node --check` sur chacun des fichiers JavaScript directement dans `static/` : succès.
- `cargo test --workspace --offline --locked --lib` : 94 tests réussis, 1 ignoré (MinIO).
- Vérification sous Node, avec un magasin simulé en mémoire, que la création hors ligne abandonne le compte rendu, la durée et les techniciens de son corps envoyé.
- Exécution isolée, sous Node, de la véritable fonction `formatDate()` extraite de `static/app.js` : une chaîne contenant une balise HTML est retournée intégralement, sans échappement.
- Pas d’exécution des installateurs ni de restauration réelle. Les constats correspondants sont établis par leurs appels et leurs opérations sur les chemins/droits, sans attribuer à l’application une limitation du bac à sable.

## Critique

Aucun constat établi.

## Haute

### H1 — Injection HTML persistante par les dates d’intervention

**Fichiers :** `crates/apex-core/src/interventions.rs:930`, `crates/apex-core/src/interventions.rs:968`, `static/app.js:62`, `static/app.js:7389`, `static/app.js:7683`.

**Défaut :** des dates reçues comme chaînes libres sont enregistrées puis insérées dans le HTML sans `esc()`, car `formatDate()` restitue la chaîne originale si elle est invalide.

**Scénario :** un compte disposant de `interventions.create` crée par API une intervention `a_posteriori` sur un équipement accessible, avec un compte rendu, une durée et `completed_at: "<b id='audit-marqueur'>contenu injecté</b>"`. Cette valeur satisfait même la comparaison SQL textuelle avec une date de début commençant par « 2026 ». L’ouverture de la fiche par un autre utilisateur interprète la balise ; un contenu trompeur ou une redirection HTML peut ainsi être injecté. La CSP limite les scripts en ligne mais ne neutralise pas cette injection HTML.

**Correction :** analyser et normaliser les dates RFC 3339 côté serveur avant toute écriture, en comparant des dates analysées. Appliquer `esc(formatDate(...))` à toutes les interpolations HTML de dates, y compris les cas d’erreur du formateur.

### H2 — Conservation des données du précédent utilisateur dans le cache hors ligne

**Fichiers :** `static/app.js:1432`, `static/app.js:1751`, `static/app.js:16378`, `static/offline.js:126`, `static/offline.js:134`.

**Défaut :** une nouvelle connexion ne vérifie pas le propriétaire du cache IndexedDB et fusionne un delta avec les données de l’utilisateur précédent.

**Scénario :** A, du site A, charge un instantané ; sa session expire, puis B, du site B, se connecte sur le même navigateur sans déconnexion explicite de A. Le dernier horodatage de A est repris dans `?since=` ; `saveSnapshot()` remplace les métadonnées utilisateur mais ne vide pas les collections puisque la réponse est un delta. Les équipements, pièces et utilisateurs du site A restent enregistrés sous le cache désormais attribué à B, consultables dans le stockage du navigateur ; l’ancienne outbox reste également présente.

**Correction :** comparer utilisateur et site stockés à la nouvelle identité avant tout usage du cache. En cas de changement, traiter explicitement l’ancienne outbox, effacer les données de A et demander un instantané complet pour B, sans réutiliser son curseur.

### H3 — Les liens de réinitialisation survivent au changement du mot de passe

**Fichiers :** `crates/apex-core/src/services.rs:369`, `crates/apex-core/src/store.rs:457`, `crates/apex-core/src/pilotage/reset.rs:85`.

**Défaut :** le changement de mot de passe personnel ou administratif révoque les sessions mais laisse valides les jetons de récupération déjà émis.

**Scénario :** un tiers possède un lien de récupération encore valable ; l’utilisateur ou l’administrateur change le mot de passe pour sécuriser le compte. Pendant les trente minutes de validité du lien, le tiers peut toujours appeler `password-reset/confirm`, remplacer le nouveau mot de passe et révoquer les sessions de la victime. Le contrôle du hash dans `confirm()` compare deux lectures faites pendant la confirmation, sans rattacher le jeton au mot de passe existant lors de son émission.

**Correction :** invalider tous les jetons de récupération du compte dans la transaction de chaque changement ou réinitialisation de mot de passe. Faire de même lors du changement de l’adresse de récupération.

### H4 — Épuisement mémoire lors du rattrapage des échéances de compteur

**Fichiers :** `crates/apex-core/src/maintenance.rs:365`, `crates/apex-core/src/maintenance.rs:377`, `crates/apex-core/src/maintenance.rs:1155`, `crates/apex-core/src/maintenance.rs:2797`.

**Défaut :** `advance_counter()` construit sans limite un vecteur contenant chaque échéance manquée à partir d’une valeur externe de compteur.

**Scénario :** un planning de compteur a un pas de 1 et une occurrence ouverte ; un utilisateur autorisé à relever les compteurs saisit `1000000000000`, valeur positive acceptée. La clôture, l’annulation ou le report qui avance le planning tente alors de construire près de mille milliards d’éléments, puis autant d’écritures : mémoire épuisée et processus susceptible d’être arrêté, avec verrou d’écriture détenu pendant le traitement. Le même problème existe avec un relevé Supervision.

**Correction :** calculer d’abord le nombre d’échéances par arithmétique vérifiée et refuser les opérations dépassant une limite explicite avant toute allocation. Prévoir un traitement borné ou une représentation adaptée aux grands rattrapages, et protéger les additions contre les dépassements d’entier.

### H5 — La mise à jour Windows retire les droits du compte de service dédié

**Fichiers :** `packaging/windows/apex-maint.iss:204`, `packaging/windows/apex-maint.iss:213`, `crates/apex-server/src/service_windows.rs:352`, `crates/apex-server/src/service_windows.rs:358`.

**Défaut :** l’installateur réinitialise récursivement les ACL au profit de LocalService même lorsqu’un service existant utilise un compte dédié.

**Scénario :** l’installation fonctionne sous `DOMAINE\compte`, configuré pour un partage UNC. Une mise à jour appelle systématiquement `service permissions`, dont le `/reset /T` retire les ACE de ce compte ; les droits de data/run sont ensuite réattribués seulement à SYSTEM, Administrateurs et LocalService. Le service existant n’est pas réinstallé et garde son compte SCM, qui ne peut plus lire la base ou les secrets à son redémarrage.

**Correction :** détecter le compte du service existant et préserver ou réappliquer ses droits, sans réinitialisation aveugle de l’arborescence. Réserver le provisionnement LocalService à une installation neuve.

### H6 — La première configuration Linux est illisible par le service

**Fichiers :** `packaging/linux/debian/postinst:7`, `crates/apex-config/src/store.rs:291`, `crates/apex-config/src/store.rs:356`, `packaging/linux/systemd/apex-maint.service:14`.

**Défaut :** la création initiale du fichier de configuration par root ne lui attribue pas le groupe `apex-maint`.

**Scénario :** après installation du DEB, l’assistant ou `config init` est lancé en administrateur comme documenté, avec le groupe principal root et l’umask habituel 022. Le dossier est root:apex-maint en 0750, sans bit setgid ; le temporaire de configuration est donc créé root:root en 0640. Le bloc qui conserve le groupe ne s’exécute qu’en présence d’un ancien fichier. Le service exécuté sous `apex-maint` ne peut pas lire cette première configuration et refuse de démarrer.

**Correction :** attribuer explicitement le groupe `apex-maint` et un mode permettant sa lecture lors de la création système initiale. Une autre solution est de provisionner correctement l’héritage de groupe du dossier et de vérifier le résultat avant de démarrer le service.

## Moyenne

### M1 — Une consommation ordinaire laisse une réservation devenue impossible à honorer

**Fichiers :** `crates/apex-core/src/stock.rs:1610`, `crates/apex-core/src/stock.rs:1623`, `crates/apex-core/src/stock.rs:1633`.

**Défaut :** la sortie ordinaire exclut les réservations de sa propre intervention du contrôle de disponibilité sans les consommer ni les libérer.

**Scénario :** un emplacement contient 5 pièces, toutes réservées pour l’intervention I. Un POST sur `/interventions/I/parts` consomme les 5 pièces : le contrôle autorise la sortie, le stock devient 0, mais la réservation reste `prepared` pour 5. Le disponible devient −5 et la consommation de cette réservation échoue ensuite, alors que l’intervention est toujours active.

**Correction :** transformer atomiquement les réservations concernées lors d’une consommation ordinaire, ou imposer la route de consommation de réservation lorsque les pièces sont préparées. Maintenir l’invariant « réservations préparées ≤ stock physique ».

### M2 — Le parcours hors ligne n’est pas raccordé à l’interface

**Fichiers :** `static/app.js:1432`, `static/app.js:1463`, `static/app.js:8118`, `static/app.js:16373`, `static/offline.js:604`.

**Défaut :** l’IHM ne restaure pas l’identité hors ligne et ses actions de progression continuent à appeler exclusivement le serveur.

**Scénario :** un technicien charge l’application et son instantané en ligne, puis ferme et rouvre la PWA en mode avion : `checkAuth()` échoue sur `/auth/me` et affiche la connexion, sans voie de reprise locale. S’il coupe simplement le réseau en gardant la page ouverte, « Prendre en charge », les temps et la clôture utilisent l’API ; les fonctions locales correspondantes de `offline.js` ne sont jamais appelées par `app.js`. Seule la création possède un raccord local.

**Correction :** mémoriser les seules données d’identité prévues par la spécification et ouvrir les vues depuis l’instantané en cas de perte réseau. Raccorder lecture et progression aux fonctions locales et à l’outbox, avec une vérification actuelle des droits au retour en ligne.

### M3 — La déclaration hors ligne perd le compte rendu et le temps saisis

**Fichiers :** `static/app.js:7977`, `static/app.js:7990`, `static/offline.js:554`, `static/offline.js:583`.

**Défaut :** `localCreateIntervention()` reconstruit un corps incomplet et supprime les champs de clôture a posteriori ainsi que la liste des techniciens.

**Scénario :** en gardant la page ouverte lors d’une coupure, l’utilisateur crée une intervention `a_posteriori`, saisit son compte rendu et 45 minutes, puis valide. L’IHM annonce un enregistrement local réussi ; la fonction locale ignore `solution_report`, `duration_minutes` et `technician_ids`, tant dans l’instantané que dans le corps de l’outbox. Au retour du réseau, le serveur refuse la création faute de compte rendu et de durée ; les textes et temps saisis ne sont plus récupérables dans la file. Une création corrective perd également ses affectations choisies.

**Correction :** conserver et transmettre tous les champs métier applicables au type choisi, dans l’instantané et l’outbox. Si la création a posteriori n’est pas supportée hors ligne, la désactiver explicitement avant saisie.

### M4 — La restauration copie la base vers un emplacement différent de celui de la configuration restaurée

**Fichiers :** `crates/apex-server/src/exploitation.rs:391`, `crates/apex-server/src/exploitation.rs:468`, `crates/apex-server/src/exploitation.rs:476`, `crates/apex-config/src/paths.rs:133`.

**Défaut :** les destinations sont calculées avec la configuration courante, puis remplacées par une configuration sauvegardée pouvant désigner d’autres chemins.

**Scénario :** l’instance cible utilise la base relative `apex-maint.db` dans son dossier de données ; l’archive vient d’une instance dont `database.file` vaut `restauree.db`, ou dont `paths.data_dir` est personnalisé. La restauration copie vers la destination courante, écrit ensuite le TOML de l’archive et annonce un succès. Au démarrage, le nouveau TOML dirige le moteur ailleurs : il ouvre l’ancienne base de ce chemin ou en crée une vide au lieu de la base restaurée. Les fichiers par défaut et `secret.key` peuvent subir le même décalage de dossier.

**Correction :** résoudre et valider un plan cohérent de destinations avant toute écriture. Soit préserver les paramètres d’implantation de la cible dans le TOML restauré, soit installer tous les éléments aux chemins effectivement désignés par ce TOML, en vérifiant également les verrous concernés.

### M5 — Une remise à zéro du compteur entre en collision avec les occurrences historiques

**Fichiers :** `crates/apex-core/src/maintenance.rs:1203`, `crates/apex-core/src/maintenance.rs:2388`, `crates/apex-core/migrations/0006_maintenance.sql:96`, `crates/apex-core/src/integration/ingest.rs:105`.

**Défaut :** les échéances sont recalées après remise à zéro, mais leur unicité reste basée sur la seule paire planning/valeur, sans distinguer les cycles du compteur.

**Scénario :** un planning de pas 100 a une occurrence réalisée à 100 ; le compteur est à 100 et la prochaine échéance vaut 200. Après remise à zéro à 0, la prochaine échéance devient 100, conformément à la conservation de la distance restante. Lorsque le nouveau compteur atteint 100, la génération réutilise une valeur historique déjà unique : elle échoue, et aucune nouvelle intervention n’est créée.

**Correction :** distinguer les générations du compteur dans les clés d’occurrence, ou conserver une grandeur cumulée interne indépendante de la valeur affichée. Adapter ensemble le calcul d’échéance et les contraintes d’unicité sans effacer l’historique.

### M6 — Un état Supervision ancien peut remplacer un état plus récent

**Fichiers :** `crates/apex-core/src/integration/ingest.rs:224`, `crates/apex-core/src/integration/ingest.rs:232`, `crates/apex-core/src/integration/ingest.rs:240`.

**Défaut :** la protection contre les événements désordonnés compare la date de début de l’état conservée, plutôt que la date du dernier événement accepté.

**Scénario :** Maint connaît `running` depuis 10 h ; les événements réels suivants sont `stopped` depuis 11 h puis `running` depuis 12 h. Le paquet de 12 h arrive en premier : puisque l’état vaut encore `running`, l’UPSERT conserve `since=10 h`. Quand arrive le paquet retardé `stopped, since=11 h`, il passe le contrôle `11 h ≥ 10 h` et remplace le véritable état courant par un ancien arrêt.

**Correction :** conserver séparément la date du dernier événement accepté et la date de début de l’état affiché. Comparer l’ordre des événements avec la première, y compris lorsque deux messages portent le même état.

### M7 — La création de sous-interventions contourne la permission de modification

**Fichiers :** `crates/apex-core/src/interventions.rs:871`, `crates/apex-core/src/interventions.rs:919`, `crates/apex-core/src/interventions.rs:1103`, `crates/apex-server/src/http/interventions.rs:248`.

**Défaut :** la route de sous-intervention exige seulement `interventions.create`, alors que le contrat réserve cette action à `interventions.edit`.

**Scénario :** un compte Production, qui peut déclarer un incident mais ne peut pas modifier les interventions, appelle `POST /api/interventions/{id}/sub-interventions` pour une intervention active de son site. Le dépôt délègue directement à `create_intervention()` et accepte l’écriture ; cette sous-intervention peut ensuite empêcher la clôture du parent. Le même contournement existe via `POST /api/interventions` avec `parent_id`.

**Correction :** exiger `interventions.edit` dès qu’un parent est demandé, dans le service commun de création. Conserver le contrôle du parent et de son site dans la même transaction.

## Basse

### B1 — Le bouton de mise à jour n’active pas le nouveau service worker

**Fichiers :** `static/app.js:16361`, `static/sw.js:104`.

**Défaut :** l’IHM envoie `{type: "SKIP_WAITING"}` tandis que le worker attend `{action: "skipWaiting"}`.

**Scénario :** une version est en attente et l’utilisateur clique « Recharger » ; le message est ignoré, puis le rechargement immédiat peut rester sous le contrôle de l’ancien worker au lieu d’activer celui proposé.

**Correction :** utiliser le même protocole de message des deux côtés et recharger après l’événement `controllerchange` confirmant l’activation.

### B2 — La déconnexion efface les conflits sans avertissement de perte

**Fichiers :** `static/app.js:1838`, `static/app.js:1848`, `static/offline.js:302`.

**Défaut :** l’avertissement de déconnexion compte seulement les opérations `pending`, alors que l’effacement supprime également celles en conflit.

**Scénario :** toutes les opérations non envoyées sont passées en « À résoudre » ; le nombre d’opérations en attente vaut zéro. Une déconnexion efface l’outbox entière sans confirmation, et le compte rendu ou la déclaration refusée est perdu.

**Correction :** compter toutes les opérations non synchronisées, conflits compris, avant l’effacement. Proposer la confirmation et l’export prévus pour ces données.

## Zones non relues ou non validées exhaustivement

- Pas de relecture ligne par ligne de l’ensemble des 18 000 lignes de `static/app.js` : revue des points d’insertion HTML, appels API, authentification, interventions, cache/synchronisation et intégration ; formulaires secondaires, calculs d’impression et graphiques seulement parcourus.
- Utilitaire graphique `apex-maint-admin` : parcours configuration/service/sauvegarde examinés partiellement ; écrans de diagnostic, journaux, sonde et ergonomie non relus exhaustivement.
- Configuration, démarrage, TLS et purge : revue ciblée des chemins, permissions, secrets, restauration et transactions ; pas de preuve exhaustive de toutes les branches ni de toutes les courses interprocessus.
- Tous les tests de bibliothèques ont été exécutés, mais leurs sources et les tests HTTP boîte noire n’ont pas été intégralement relus ; les tests HTTP, Clippy et la suite Cargo complète avec binaires n’ont pas été lancés.
- Pas de recette réelle Windows/DEB/systemd, de navigateur, de caméra/PWA sur Android/iOS, de stockage S3 distant, de partage UNC ou de proxy TLS.
- Dépendances tierces et code embarqué `static/vendor/jsqr/jsQR.js`, binaires, polices et images : non audités. Pas d’analyse de vulnérabilités des dépendances ni d’interrogation réseau.

## Suivi des corrections

Les modifications déjà présentes pour H1–H5, H2/B2 sont conservées. Les autres constats ont été confirmés dans le code avant correction. Vérifications sur bases en mémoire et instances portables temporaires uniquement ; aucun commit, changement de branche ni accès aux bases d’exploitation.

| Constat | État, fichiers et test |
| --- | --- |
| H1 | Corrigé précédemment — `crates/apex-core/src/interventions.rs`, `maintenance.rs`, `static/app.js` ; `terrain_tests::dates_d_intervention_analysees_et_normalisees` passe. |
| H2 | Corrigé précédemment — `static/offline.js`, `static/app.js` ; test 6 de `tests/offline.test.js` passe. |
| H3 | Corrigé précédemment — `crates/apex-core/src/store.rs` ; `pilotage::tests::reset_jetons_revoques_par_changement_de_mot_de_passe_ou_d_adresse` passe. |
| H4 | Corrigé et compilé — `crates/apex-core/src/maintenance.rs` ; rattrapage borné déjà présent conservé, additions de création/modification et remise à zéro vérifiées ; `maintenance::tests::test_echeances_compteur_et_remise_a_zero` passe. |
| H5 | Corrigé précédemment — `packaging/windows/apex-maint.iss`, correction conservée ; contrôle de code, installateur non exécuté. |
| H6 | Corrigé — `packaging/linux/debian/postinst` (dossier setgid root:apex-maint), `crates/apex-config/src/store.rs` (groupe du dossier et mode 0640 dès la première création, erreurs propagées) ; test Unix `premiere_configuration_lisible_par_le_groupe_du_dossier` ajouté, non exécuté sous Windows. |
| M1 | Corrigé — `crates/apex-core/src/stock.rs`, sortie ordinaire refusée lorsque les mêmes pièces sont préparées pour cette intervention, route de consommation de réservation requise ; `terrain_tests::consommation_ordinaire_refuse_les_pieces_preparees` passe. |
| M2 | Corrigé au minimum sûr autorisé — `static/app.js`, `static/offline.js` ; prise en charge, saisie de son propre temps et clôture raccordées aux fonctions locales quand `navigator.onLine === false`, détail lu dans l’instantané, identité vérifiée, temps remplacé comme sur l’API ; tests M2 dans `tests/offline.test.js` et `tests/audit-ui.test.js` passent. Reste : réouverture après fermeture en mode avion, restauration de l’identité, listes et formulaires secondaires hors ligne ; recette navigateur non effectuée. Les droits restent revérifiés par le serveur à la synchronisation. |
| M3 | Corrigé — `static/offline.js`, conservation de `solution_report`, `duration_minutes`, `technician_ids`, `started_at`, `completed_at` dans l’instantané et l’outbox ; test M3 de `tests/offline.test.js` passe. |
| M4 | Corrigé — `crates/apex-server/src/exploitation.rs`, paramètres `paths` et `database` de la cible conservés avant validation et écriture ; test unitaire `restauration_conserve_les_destinations_de_la_cible` passe ; test temporaire `sauvegarde_chaude_chiffree_restauration_et_gardes` enrichi avec un nom de base cible différent, non atteint par la suite complète qui s’arrête sur les tests HTTP d’authentification. |
| M5 | Corrigé — migration `crates/apex-core/migrations/0012_audit.sql`, `maintenance.rs`, `integration/ingest.rs` ; cycles distincts lors des remises à zéro manuelles et Supervision, unicité conservée dans chaque cycle sans effacer l’historique ; `terrain_tests::occurrences_compteur_distinctes_apres_remise_a_zero` passe, avec génération réelle d’une intervention au même seuil dans le nouveau cycle et refus d’un doublon dans ce cycle. |
| M6 | Corrigé — migration `0012_audit.sql`, `crates/apex-core/src/integration/ingest.rs` ; `last_event_at` distinct de `since`, mis à jour même pour un état identique ; régression dans `integration_tests::ingestion_statuts_rapprochement_historique_html_et_peremption` passe. |
| M7 | Corrigé — `crates/apex-core/src/interventions.rs`, permission `interventions.edit` exigée dans la création commune dès que `parent_id` est fourni ; `terrain_tests::sous_intervention_exige_modification` passe. |
| B1 | Corrigé — `static/app.js`, `static/sw.js`, protocole `type: SKIP_WAITING` commun, rechargement après `controllerchange` ; test B1 de `tests/audit-ui.test.js` passe. |
| B2 | Corrigé précédemment — `static/app.js`, `static/offline.js`, avertissement et export des conflits inclus ; test 7 de `tests/offline.test.js` passe. |

Limites de validation : les tests d’intégration HTTP Windows échouent au démarrage des instances temporaires lorsque la restriction des ACL de `secret.key` est refusée par l’environnement (`Accès refusé`). La protection des fichiers privés n’a pas été affaiblie pour contourner ce refus. Le lancement standard de `node --test` échoue également avec `spawn EPERM` ; les mêmes tests passent avec `--experimental-test-isolation=none` (11/11).

Résultats finaux des commandes :

- `cargo fmt --all` : code 0 ; `cargo fmt --all -- --check` : code 0.
- `cargo clippy --workspace --all-targets -- -D warnings` : code 0, aucun avertissement.
- `cargo test --workspace` : code 101 ; 128 tests réussis et 1 ignoré (MinIO) avant les 7 échecs de `tests/auth.rs` dus au refus des ACL lors du démarrage. Les suites suivantes ne sont pas exécutées. Journal : `target/audit-cargo-test.log`.
- `cargo test --workspace --lib` : code 0, 100 tests réussis, 1 ignoré (avant renforcement final du scénario M5, qui passe également dans la commande complète finale).
- `cargo test -p apex-server --bin apex-maint restauration_conserve_les_destinations_de_la_cible` : code 0, 1 test réussi.
- `node --check` sur chacun des quatre fichiers `static/*.js` : code 0.
- `node --test tests/offline.test.js` : code 1, `spawn EPERM` ; lancement avec les deux fichiers de tests : même refus de création des processus.
- `node --test --experimental-test-isolation=none tests/offline.test.js tests/audit-ui.test.js` : code 0, 11 tests réussis, aucun ignoré.
- `git diff --check` : code 0.

Le contrat de vérification complète sans erreur reste donc non satisfait dans cet environnement : relancer les commandes standards Node et les tests HTTP dans un environnement autorisant leurs processus et les ACL Windows. Le test H6 doit aussi être exécuté sous Unix. Aucune recette d’exploitation n’a été tentée.
