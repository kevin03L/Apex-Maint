# Règles communes aux agents (Claude, Codex, Antigravity)

Apex Maint : GMAO multi-sites en Rust (workspace `crates/`), deuxième produit de la gamme Apex.
Il reprend l'architecture et les conventions d'Apex Supervision (`D:\Vs-code\Apex-Supervision`),
avec une IHM web statique (`static/`, HTML/CSS/JS sans framework ni build). Le plan de référence
est `docs/PLAN.md`.

## Langue et style
- Textes de l'IHM, messages, commentaires et documentation : **en français**.
- Suivre le style du code voisin (densité de commentaires, nommage, idiomes) et, à défaut, celui
  d'Apex Supervision.
- Pas d'émojis dans les libellés de l'IHM : icônes SVG en ligne.

## Règles impératives
- Ne jamais toucher aux bases d'exploitation ni à `D:\Vs-code\Apex-Supervision` (lecture seule) ;
  tester uniquement sur des instances portables temporaires, sur des ports autres que 3000 et 3100.
- Ne rien commiter, pousser ni créer de branche : c'est l'humain (ou Claude, sur sa demande) qui décide.
- Données métier : toute fonction de dépôt reçoit un `Actor` et filtre par `site_id` ; suppression
  logique (`deleted_at`) uniquement.
- IHM : toute donnée insérée dans le HTML passe par `esc()`, tout argument de gestionnaire `onclick`
  par `jsArg()`.
- Le serveur reste la référence des droits : l'IHM masque, le serveur refuse.
- La session est un cookie `HttpOnly` : aucun jeton de session lisible en JavaScript.

## Vérifications
- JavaScript : `node --check static/*.js`.
- Rust : `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
