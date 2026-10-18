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

## Phase 7 — Pilotage et paramètres

- **Mot de passe oublié : deux voies.**
  - Réinitialisation par l'admin (toujours disponible) ; l'utilisateur change son mot de passe à la
    connexion suivante.
  - Lien par e-mail si le SMTP est configuré dans les Paramètres. Le jeton est à usage unique,
    expire après 30 minutes, n'est stocké qu'haché, et la réponse est identique que le compte existe
    ou non.
  Les paramètres SMTP serviront aussi aux alertes (stock bas, échéances).
- **Interface en français seulement**, comme Supervision.

## Phase 8 — Exploitation

- **Pas d'import de l'ancienne GMAO** : `import-legacy` est retiré du plan, et on démarre à vide.

## Phase 9 — Terrain

- Le mode hors-ligne des interventions **reste en phase 9**, une fois les écrans stabilisés.

## Phase 10 — Liaison avec Apex Supervision

- **Liaison facultative**, activée dans les Paramètres. Désactivée par défaut.
- **API et clé de service** : Supervision appelle `POST /api/integrations/supervision/...` avec une
  clé dédiée. Cette clé est générée dans Maint, rattachée à un site, révocable, et stockée hachée. Les
  bases restent séparées.
- **Chaque type d'événement s'active séparément.** Sont retenus :
  - **compteurs d'heures** : ils alimentent les compteurs d'équipement, donc les plannings `counter` ;
  - **statut de l'équipement** : l'arrêt ou la dégradation détectés par Supervision mettent à jour le
    statut dans Maint.
  L'alarme qui créerait une demande d'intervention n'est pas retenue pour l'instant.
- Correspondance des équipements : par code d'équipement, au sein du site de la clé.

## Restent ouverts

- Code partagé avec Supervision (`apex-common`) : à revoir après la phase 8.
- Dépôt public et signature (SignPath) : à décider à la première version installable.
