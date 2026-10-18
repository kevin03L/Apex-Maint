# Administration d'Apex Maint

L'utilitaire `apex-maint-admin` reprend celui de Supervision : configuration, assistant initial,
état local, service, journaux, diagnostics, sauvegardes et maintenance. Les opérations longues
s'exécutent hors du fil graphique. Fermer l'utilitaire n'arrête pas le moteur.

`apex-maint backup [--output <dossier>] [--include-files folder_only|all|none]`
`[--with-secret-key] [--password-file <fichier>]` crée une archive cohérente à chaud.
`apex-maint restore <archive> [--password-file <fichier>] [--force]` exige le moteur arrêté.
Sans `--force`, confirmer le remplacement ; une sauvegarde de sécurité précède la restauration.
L'option interne `--confirmed` sert à la confirmation graphique et conserve cette sauvegarde.
Sans `secret.key`, ressaisir les secrets chiffrés après restauration.

`apex-maint purge [--dry-run] [--older-than <années>]` (minimum 2 ans) conserve les références
encore utilisées. Une sauvegarde précède toute purge réelle. Les orphelins ne sont jamais effacés.
`apex-maint files verify [--storage <id>] [--sample <n>]` contrôle les empreintes et les orphelins.
`apex-maint files migrate --from <id> --to <id> [--dry-run]` copie et vérifie avant de changer les
références ; les fichiers source sont conservés et l'opération peut reprendre.

Les réglages et les formats JSON sont décrits dans [API phase 8](API-PHASE-8.md).
