# API Phase 4 — Stock : sorties idempotentes

Complément au contrat de stock décrit dans [la spécification de phase 4](specs/phase-4.md).

| Route | Corps | Réponse réussie |
|---|---|---|
| `POST /api/interventions/{id}/parts` | `part_id`, `location_id`, `quantity` ; champs facultatifs existants inchangés | 201, `InterventionPart` |
| `POST /api/reservations/{id}/consume` | `quantity` facultative ; corps absent accepté pour consommer toute la réservation | 200, `InterventionPart` |

Ces deux routes acceptent désormais `Idempotency-Key: UUID`. Sans cet en-tête,
le comportement reste inchangé : chaque requête valide effectue sa consommation.

Avec cet en-tête, la réponse réussie est enregistrée dans la même transaction SQLite
que la sortie, le mouvement de stock et l’audit. Le rejeu renvoie exactement le même
code et le même corps JSON, sans second mouvement, même après redémarrage.
La consommation de réservation est rejouable après son passage à l’état consommé.

Le mécanisme est celui des routes [terrain de phase 9](API-PHASE-9.md) : verrou commun,
clé liée à l’utilisateur et à son site courant, permissions actuelles vérifiées avant
rejeu, refus métier 4xx mémorisés et purge commune après trente jours. Une clé déjà
utilisée par un autre utilisateur, site, corps ou une autre route est refusée en **422**.
Une valeur d’en-tête autre qu’un UUID est également refusée en 422.

La migration **0016** ajoute l’empreinte SHA-256 du corps reçu (octets exacts, au plus
64 Kio avec clé). Les anciennes clés terrain sans empreinte restent rejouables selon
leurs règles précédentes jusqu’à leur purge. Les nouvelles clés contrôlent toutes
le corps, y compris celles des routes terrain. Un client doit conserver sa clé et
son corps exact tant qu’il n’a pas reçu l’acquittement ; une nouvelle consommation
après succès reçoit une nouvelle clé. La protection s’arrête à la purge de la clé.

La sortie directe exige toujours `interventions.edit` et `stock.movements` ; la
consommation de réservation conserve `stock.movements`. Les contrôles `Actor`,
de site, d’état d’intervention, de réservation et de disponibilité restent appliqués
à l’écriture. Ces routes ne deviennent pas des opérations de la file hors ligne Android.
