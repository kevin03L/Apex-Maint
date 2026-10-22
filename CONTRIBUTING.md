# Contribuer à Apex Maint

Apex Maint est une GMAO multi-sites en Rust, avec une interface web statique et un client Android natif. Les contributions, signalements de bogues et propositions d’amélioration sont les bienvenus.

## Préparer une proposition

Consultez les tickets existants avant d’en ouvrir un. Décrivez le besoin métier, le comportement attendu et un exemple reproductible. Pour une évolution importante, exposez le périmètre dans un ticket avant d’entreprendre les modifications.

Une vulnérabilité se signale en privé selon [SECURITY.md](SECURITY.md), sans ticket public.

Lisez [AGENTS.md](AGENTS.md), le [plan](docs/PLAN.md), les [décisions](docs/specs/decisions.md) et les contrats du module dans la [documentation](README.md#documentation). Les spécifications historiques peuvent décrire des choix remplacés par les décisions ou les contrats HTTP.

## Protéger les instances et les données

- Testez uniquement sur des instances portables temporaires, sans données d’exploitation, sur des ports autres que **3000 et 3100**.
- Les bases d’exploitation ne doivent jamais être utilisées ni modifiées par les tests. L’installation voisine `D:\Vs-code\Apex-Supervision` est une référence en lecture seule.
- Chaque fonction de dépôt métier reçoit un `Actor` et filtre les accès par `site_id`. Les suppressions métier sont logiques (`deleted_at`).
- Les autorisations restent vérifiées par le serveur : masquer un bouton ne protège pas une route.
- Les données insérées dans le HTML passent par `esc()` ; les arguments des gestionnaires `onclick` par `jsArg()`.
- La session web reste un cookie `HttpOnly` ; aucun jeton de session ne doit devenir lisible en JavaScript. Ne publiez ni secret, ni journal sensible, ni copie de données réelles.

## Style et périmètre

Rédigez en français les textes d’interface, messages, commentaires et documents. Suivez le style et les idiomes des fichiers voisins, ou ceux d’Apex Supervision à défaut. Utilisez des icônes SVG en ligne dans l’IHM, sans émojis dans les libellés.

L’interface `static/` reste en HTML/CSS/JavaScript sans framework ni build. Gardez les changements centrés sur le problème traité et actualisez la documentation lorsque le comportement ou le contrat HTTP change. Ajoutez des tests pertinents pour les règles métier, l’isolation des sites et les permissions touchées.

## Vérifications avant une proposition

Depuis la racine :

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Vérifiez la syntaxe de tous les fichiers `static/*.js`, un fichier par appel :

```powershell
Get-ChildItem static/*.js | ForEach-Object { node --check $_.FullName; if ($LASTEXITCODE -ne 0) { throw "Syntaxe JavaScript invalide" } }
```

Sous un shell POSIX :

```sh
for f in static/*.js; do node --check "$f" || exit; done
```

Le [workflow de référence](.github/workflows/ci.yml) prévoit aussi `cargo fmt --all --check` et `node --test tests/*.test.js`. Les Actions ne tournent pas actuellement : indiquez les contrôles réellement exécutés et leurs résultats, ainsi que toute vérification bloquée. Pour Android, suivez les [instructions de compilation et de recette](android/README.md).

Pour une modification documentaire, vérifiez les liens relatifs, les images et le rendu Markdown ; pour un SVG, vérifiez aussi que le XML est bien formé.

## Commits et pull requests

Utilisez un titre de commit bref en français, avec un domaine si utile, dans le style de l’historique : `Android : corriger la reprise de synchronisation` ou `Documentation : préciser l’installation Linux`. Le corps explique la raison du changement et les points utiles à la relecture. Évitez de mélanger refonte générale et correction ciblée.

Une pull request décrit le problème, le résultat, les tests et les limites connues. Utilisez le [modèle fourni](.github/pull_request_template.md) et liez le ticket concerné. Les captures doivent provenir d’une instance de démonstration et ne contenir aucune donnée sensible.

Lors d’un travail assisté dans le dépôt local, les agents ne créent ni branche, ni commit, ni push de leur propre initiative : ces opérations restent sous le contrôle humain conformément à [AGENTS.md](AGENTS.md).

Les contributions sont proposées sous la licence [GPL-3.0-or-later](LICENSE) du projet.
