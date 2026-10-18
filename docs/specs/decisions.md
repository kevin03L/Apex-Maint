# Décisions pour les phases 5 à 10 (2026-10-18)

Arbitrages pris avec le responsable du produit. Ils priment sur `docs/PLAN.md` (§4 et §5) en cas
d'écart, et chaque spécification de phase doit les reprendre.

## Phase 5 — Maintenance préventive

- **Génération des interventions préventives : automatique et manuelle.** Par défaut, le moteur crée
  l'intervention N jours avant l'échéance (N réglable par planning), avec une seule intervention
  ouverte à la fois par planning. La génération automatique se désactive par planning. Un bouton
  « Générer l'intervention » reste disponible pour le responsable.
- **Prochaine échéance calculée depuis la date prévue** (calendrier fixe) : échéance théorique +
  fréquence. Un retard de réalisation ne décale pas les échéances suivantes. Une échéance dépassée et
  non réalisée reste visible comme en retard.
- **Compteurs facultatifs, réglés par équipement.** Un équipement peut avoir un ou plusieurs compteurs
  activés (heures de marche, cycles…). Les relevés sont saisis à la main en phase 5. Les plannings de
  type `counter` ne sont proposés que pour un équipement qui a un compteur actif.

## Phase 6 — Documents et QR code

- **Fichiers sur disque, nommés par UUID**, métadonnées en base (nom d'origine, type MIME, taille,
  empreinte SHA-256, auteur, site).
- **Emplacement de stockage paramétrable dans les Paramètres** (droit `settings.edit`), avec deux
  pilotes :
  - **dossier** : local (`data/files` par défaut) ou partage réseau (chemin UNC `\\serveur\partage`,
    montage NFS) ;
  - **compatible S3** : point d'accès, bucket, région, clés d'accès (MinIO, NAS, hébergeur…). La
    clé secrète est stockée chiffrée, jamais renvoyée à l'IHM, et seulement remplaçable.
  Un bouton « Tester la connexion » est prévu. Changer de pilote ne déplace pas les fichiers
  existants : chaque document garde la référence de son stockage d'origine (la migration entre
  stockages sera un outil à part).
- **Le QR code contient seulement le code de l'équipement** (ex. `PRS-01`). Il est lu par le scanner
  intégré à l'IHM (phase 9), qui ouvre la fiche. Étiquette imprimable : QR, code et nom.
- **Documents joints** aux équipements, aux interventions, aux pièces détachées et aux plannings
  préventifs.
- **Types de fichiers paramétrables** par l'admin : PDF, images, Word, Excel et CAO (DWG, DXF,
  STEP, IGES), choisis dans un catalogue sûr (SVG, HTML et exécutables interdits). La taille
  maximale est réglable, 50 Mo par défaut.
- **Historique des versions** : une nouvelle version remplace la version affichée, et les
  anciennes restent consultables et restaurables.
- **Format d'étiquette paramétrable** : planche (A4 ou Letter, colonnes, lignes, marges) ou
  étiquette unitaire aux dimensions libres, pour une imprimante thermique ; le contenu affiché est
  au choix.

## Phase 7 — Pilotage et paramètres

- **Mot de passe oublié : deux voies.**
  - Réinitialisation par l'admin (toujours disponible) ; l'utilisateur change son mot de passe à la
    connexion suivante.
  - Lien par e-mail si le SMTP est configuré dans les Paramètres. Le jeton est à usage unique,
    expire après 30 minutes, n'est stocké qu'haché, et la réponse est identique que le compte existe
    ou non.
  Les paramètres SMTP serviront aussi aux alertes (stock bas, échéances).
- **Interface en français seulement**, comme Supervision.
- **Tableau de bord paramétrable** :
  - une disposition par défaut pour chaque rôle, fixée par l'admin ;
  - une disposition personnelle pour les utilisateurs qui ont `dashboard.customize` ;
  - des widgets filtrés par permission ;
  - indicateurs retenus : activité des interventions, fiabilité (MTBF, MTTR), préventif, coûts et
    stock.
- **Exports en CSV, Excel (.xlsx) et PDF** : listes filtrées, rapports, fiche intervention.
- **Alertes par e-mail** : stock bas, échéances préventives, intervention critique ou machine à
  l'arrêt, affectation d'un technicien. Chaque utilisateur choisit ses alertes ; les valeurs par
  défaut viennent de son rôle.
- **Journal d'audit** : durée de conservation paramétrable, 3 ans par défaut et 1 an au minimum.

## Phase 8 — Exploitation

- **Pas d'import de l'ancienne GMAO** : `import-legacy` est retiré du plan, et on démarre à vide.
- **Sauvegardes automatiques (quotidiennes) et manuelles**, avec rétention et dossier de
  destination réglables.
- **Fichiers joints dans les sauvegardes** : réglage paramétrable, qui par défaut inclut ceux d'un
  stockage dossier et garde la seule référence d'un stockage S3. Il peut tout inclure, S3 compris,
  ou rien.
- **Purge** des données supprimées depuis longtemps (durée réglable) et des journaux anciens. Pas
  de purge automatique des fichiers orphelins : ils sont seulement signalés par le contrôle
  d'intégrité.
- **Plateformes** : Windows (Inno Setup) et Debian / Ubuntu (DEB). Ni Docker ni archive portable
  pour l'instant.

## Phase 9 — Terrain

- Le mode hors-ligne des interventions **reste en phase 9**, une fois les écrans stabilisés.
- **HTTPS paramétrable**, obligatoire pour le hors-ligne et la caméra : TLS intégré (certificat
  fourni, ou autorité locale auto-signée générée) ou proxy inverse déclaré dans la configuration.
- **Hors ligne, le technicien peut** consulter ses interventions, les équipements et les pièces,
  avancer ses interventions (prise en charge, temps, compte rendu, clôture) et déclarer une
  intervention. Les photos, les pièces jointes et la consommation de pièces restent réservées au
  mode en ligne.

## Phase 10 — Liaison avec Apex Supervision

- **Liaison facultative**, activée dans les Paramètres. Désactivée par défaut.
- **API et clé de service** : Supervision appelle `POST /api/integrations/supervision/...` avec une
  clé dédiée. Cette clé est générée dans Maint, rattachée à un site, révocable, et stockée hachée. Les
  bases restent séparées.
- **Chaque type d'événement s'active séparément.** Sont retenus :
  - **compteurs d'heures** : ils alimentent les compteurs d'équipement, donc les plannings `counter` ;
  - **état de l'équipement** : **affiché à titre d'information seulement** (bandeau sur la fiche,
    historique, widget). Il ne modifie **pas** le statut de l'équipement dans Maint et ne crée pas
    d'intervention. Cette décision du 2026-10-18 remplace la précédente, qui prévoyait une mise à
    jour du statut.
  L'alarme qui créerait une demande d'intervention n'est pas retenue pour l'instant.
- Correspondance des équipements : par `external_ref` (référence Supervision) ou à défaut par le
  code de l'équipement, au sein du site de la clé. Les compteurs sont rapprochés uniquement par leur
  `external_ref`.
- **Pas de sens Maint → Supervision** pour l'instant.

## Restent ouverts

- Code partagé avec Supervision (`apex-common`) : à revoir après la phase 8.
- Dépôt public et signature (SignPath) : à décider à la première version installable.
