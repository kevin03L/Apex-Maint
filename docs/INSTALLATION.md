# Installation d'Apex Maint

Windows : construire avec `packaging/windows/build-installer.ps1` et Inno Setup 6.
Le service `ApexMaint` utilise LocalService. Les données sont dans `%ProgramData%\Apex Maint`.
Pour un partage UNC, installer avec `apex-maint service install --account DOMAINE\compte` ;
le mot de passe est demandé dans le terminal et transmis uniquement au gestionnaire Windows.
Le compte doit aussi être autorisé par le serveur de fichiers.

Debian / Ubuntu : `sh packaging/linux/build-deb.sh`, sur une machine amd64 ou arm64.
Installer le DEB, lancer l'assistant en administrateur, puis démarrer `apex-maint.service`.
Le paquet active le service mais ne le démarre pas sans configuration.
Pour les chemins personnalisés, ajouter les droits Unix et une surcharge systemd
`ReadWritePaths=`. Les données restent présentes après `apt remove`, et disparaissent avec
`apt purge`. L'interface web est installée dans `/usr/share/apex-maint/static`.

À la première connexion : `admin` / `admin123`, changement du mot de passe obligatoire.
Ne jamais utiliser une base d'exploitation pour les tests.

La signature Windows reste désactivée ; aucune clé de signature n'est livrée.
