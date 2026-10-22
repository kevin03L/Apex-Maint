# Politique de sécurité

## Versions prises en charge

| Version | Correctifs de sécurité |
|---|---|
| Branche `main` | Prise en charge |
| Révisions antérieures | Non prises en charge ; mise à jour vers `main` nécessaire |

Le projet est en phase alpha. Cette politique couvre le serveur, l’interface web, le client Android et l’outillage présents sur `main`.

## Signaler une vulnérabilité

Utilisez le signalement privé de **GitHub Security Advisories** du dépôt : [Signaler une vulnérabilité](https://github.com/kevin03L/Apex-Maint/security/advisories/new).

N’ouvrez pas de ticket public et ne publiez pas de preuve d’exploitation contenant des secrets ou des données réelles. Si le formulaire privé n’est pas proposé, demandez son activation au mainteneur dans un ticket ne contenant aucun détail sur la vulnérabilité.

Dans le rapport privé, précisez :

- La révision concernée et l’environnement : système, navigateur ou version Android.
- Le composant affecté, les droits et le site nécessaires pour reproduire le problème.
- Les étapes minimales de reproduction sur une instance portable temporaire, sans données d’exploitation.
- L’impact observé et, si possible, une piste de correction ou un contournement.

Retirez mots de passe, cookies, clés de service et données personnelles des pièces jointes. Convenez avec le mainteneur de la publication des détails après analyse et correction ; aucun délai de réponse garanti n’est annoncé.

## Configuration et modèle de sécurité

Le serveur contrôle les rôles, les permissions et le périmètre de site. Les sessions web utilisent un cookie `HttpOnly`, `SameSite=Strict`, avec expiration et révocation ; les modifications sont tracées dans le journal d’audit.

Changez le mot de passe initial `admin123` comme demandé à la première connexion. Réservez les comptes de démonstration aux instances de démonstration et utilisez HTTPS pour l’accès réseau. Consultez le [guide d’installation](docs/INSTALLATION.md), le [guide d’administration](docs/ADMINISTRATION.md) et les [instructions Android](android/README.md) pour les certificats et les données locales.
