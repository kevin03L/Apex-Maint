# Journal des modifications

Les changements notables sont consignés ici selon la structure de Keep a Changelog : ajouts, modifications et corrections. Les versions suivent la convention SemVer du workspace.

Ce journal est reconstitué depuis l’historique Git local. Aucun tag n’est présent lors de sa rédaction : la date ci-dessous correspond à l’introduction de la version dans le workspace, et ne constitue pas une date de publication d’un paquet.

## [Non publié]

### Ajouté

- Client Android natif Kotlin / Jetpack Compose pour téléphone et tablette : interventions terrain, déclaration, scan QR, catalogue de pièces et documents (`ac3010b`, 20 octobre 2026).
- Cache Room, file persistante des opérations terrain et résolution des conflits ; chiffrement local avec Android Keystore et protections des données de session (`ac3010b`, `6ea810d`).
- Configuration de signature de la version Android release, avec clé conservée hors du dépôt (`8f86cff`, 21 octobre 2026).
- Présentation GitHub illustrée, bannière SVG, guide de contribution, politique de sécurité et modèles de tickets et de pull requests.

### Modifié

- Ergonomie Android : navigation adaptative, deux volets sur tablette, conservation de l’état et thèmes Système/Clair/Sombre alignés sur le web (`6ea810d`, `6a0708a`, 20–22 octobre 2026).
- Renforcement des préconditions de synchronisation hors ligne sur les opérations d’intervention (`83dbfd9`, 20 octobre 2026).
- Sorties de pièces et consommations de réservations rejouables avec `Idempotency-Key` ; empreinte du corps des requêtes dans la migration 0016 (`69c5a49`, 20 octobre 2026).

### Corrigé

- Corrections d’audit du référentiel, des rapports, de l’isolation et des droits, des sauvegardes/restaurations et de l’utilitaire d’administration (`0ed499c` à `46e8a27`, 19–20 octobre 2026).
- Remplacement des affectations dans les deltas terrain, création idempotente de sous-interventions et prise en compte des modifications dans la file hors ligne (`120e670`).
- Fichiers temporaires Android, reprise réseau en atelier, envois de documents, abandon des conflits et navigation vers la synchronisation (`69c5a49`, `a924b8c`).
- Packaging Linux : construction de l’utilitaire, recette DEB et compatibilité arm64 Debian 12 ; vérifications JavaScript du workflow (`119166d`, `1463f76`, `1689b9c`, `e504ad8`).

Les résultats et les vérifications encore nécessaires pour Android sont détaillés dans [son README](android/README.md) et [son audit](android/AUDIT.md). La présence d’un changement dans ce journal ne vaut pas validation de compilation ou de recette.

## [0.8.0-alpha.1] - 2026-10-18

Version introduite par `2989ddd`. Cette section regroupe le socle et les phases fonctionnelles intégrées le 18 octobre jusqu’à `00b9958`, complétant cette première alpha ; les travaux ultérieurs figurent dans « Non publié ».

### Ajouté

- Workspace Rust, configuration TOML, CLI, journaux, instance unique, SQLite et interface web statique (`1e8cbb0`).
- Authentification, sessions par cookie `HttpOnly`, sites, utilisateurs, rôles et permissions (`9abd527`).
- Référentiel : équipements en arborescence, lignes de production, emplacements et fournisseurs (`4b8123f`).
- Interventions, transitions de statut, sous-interventions et suivi des techniciens (`714631c`).
- Pièces, stocks par emplacement, mouvements, réservations et consommations (`439ab45`).
- Maintenance préventive calendaire ou sur compteur, occurrences et échéances (`99507f9`).
- Documents versionnés, stockage dossier/S3, QR codes et formats d’étiquettes (`8eaf86e`, `16522bc`).
- Tableaux de bord, rapports, exports, audit et paramètres (`35e561b`, `eb5586f`).
- Exploitation : sauvegardes/restaurations, services Windows/systemd, utilitaire graphique et packaging (`2989ddd`, `b8501b8`).
- TLS, PWA et file d’interventions hors ligne (`2989ddd`, `da13541`).
- Liaison Apex Supervision : clés de service, ingestion des compteurs et états, journal des échanges et affichage dans l’IHM (`38e85ac`, `5acb28f`).

### Corrigé

- Raccordements des modules d’exploitation et d’intégration avec le pilotage (`e4aa289`).
- Corrections issues de la relecture de code à la fin des intégrations (`00b9958`).
