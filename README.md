# Apex Maint

GMAO (gestion de la maintenance assistée par ordinateur) multi-sites écrite en Rust : un moteur métier avec une IHM web pour les équipes de maintenance et de production. Deuxième produit de la gamme **Apex**, aux côtés d'[Apex Supervision](https://github.com/kevin03L/Apex-Supervision), dont il reprend l'architecture et les conventions.

> **Statut : développement, phase 4 (`0.1.0-alpha.1`).** Authentification, référentiel technique, interventions et backend complet des pièces & stock (catalogue, PMP, mouvements, consommations, réservations et alertes) sont disponibles. La feuille de route complète est dans [docs/PLAN.md](docs/PLAN.md).

## Fonctionnalités prévues

- **Multi-sites** : chaque donnée est rattachée à un site, l'administrateur voit tous les sites.
- **Équipements** en arborescence, lignes de production, emplacements, fournisseurs.
- **Interventions** correctives, préventives et a posteriori, avec état de la machine et pièces consommées.
- **Maintenance préventive** par fréquence ou par compteur, avec calcul automatique des échéances.
- **Pièces et stock** par emplacement, mouvements transactionnels, alertes de stock bas.
- **Documents** et **QR codes** d'équipement.
- **Tableau de bord**, reporting, export CSV, journal d'audit.
- **Contrôle d'accès** par rôles (Administrateur, Responsable, Production, Technicien) et matrice de permissions.
- **Exploitation** : service Windows ou unité systemd, installateur, utilitaire desktop, sauvegarde et restauration, reprise des données de l'ancienne GMAO.

## Démarrage rapide (développement, mode portable)

Prérequis : [Rust](https://rustup.rs) 1.95 ou plus récent.

```bash
cargo build
target/debug/apex-maint config init --portable --demo   # crée ./apex-maint.toml
target/debug/apex-maint run                             # Ctrl+C pour arrêter proprement
```

L'IHM web est disponible sur <http://localhost:3100>.

Au premier démarrage, le compte **`admin` / `admin123`** est créé ; le changement de ce mot de passe est obligatoire avant d'accéder aux fonctions d'administration. Le site « Site principal » est créé si aucun site n'existe.

Avec `config init --demo` (qui active `[demo] enabled = true`), les comptes **`responsable`**, **`technicien`** et **`production`**, mot de passe **`demo1234`**, sont créés une seule fois sur le « Site de démonstration ». Réservez cette option aux instances de démonstration.

Les sessions utilisent un cookie `HttpOnly`, `SameSite=Strict` et expirent après l'inactivité définie par `[security] session_ttl_hours` (12 heures par défaut). Activez `secure_cookie = true` derrière HTTPS. Les changements de droits, désactivations, suppressions et réinitialisations de mot de passe révoquent les sessions concernées.

## Développement

Les règles communes aux contributeurs et aux agents sont dans [AGENTS.md](AGENTS.md). Avant chaque proposition :

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
node --check static/app.js
```

## Licence

Ce projet est distribué sous licence **GPL-3.0-or-later** (GNU General Public License version 3 ou ultérieure).

Copyright © 2026 kevin03L.

Consultez le texte intégral dans le fichier [LICENSE](LICENSE) à la racine du dépôt.
