//! Résolution du site en conservant les conventions des dépôts existants.
use crate::{Actor, MaintError};

#[derive(Clone, Copy)]
pub(crate) enum SitePolicy {
    Strict,
    Maintenance,
}

pub(crate) fn resolve_site_id_for_create(
    actor: &Actor,
    requested_site_id: Option<&str>,
    policy: SitePolicy,
) -> Result<String, MaintError> {
    let maintenance = matches!(policy, SitePolicy::Maintenance);
    if actor.is_admin() {
        let sid = requested_site_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                MaintError::BadRequest(
                    if maintenance {
                        "Le site_id est requis pour un administrateur"
                    } else {
                        "Le site est obligatoire"
                    }
                    .into(),
                )
            })?;
        Ok(sid.to_string())
    } else {
        if let Some(sid) = requested_site_id {
            if !maintenance && sid != actor.site_id.as_deref().unwrap_or_default() {
                return Err(MaintError::Forbidden(
                    "Accès refusé : vous ne pouvez pas créer de ressource sur un autre site".into(),
                ));
            }
        }
        actor.site_id.clone().ok_or_else(|| {
            MaintError::Forbidden(
                if maintenance {
                    "Utilisateur sans site rattaché"
                } else {
                    "Aucun site rattaché"
                }
                .into(),
            )
        })
    }
}

pub(crate) fn resolve_site_id_for_query<'a>(
    actor: &'a Actor,
    requested_site_id: Option<&'a str>,
    policy: SitePolicy,
) -> Result<Option<&'a str>, MaintError> {
    let maintenance = matches!(policy, SitePolicy::Maintenance);
    if actor.is_admin() {
        Ok(if maintenance {
            requested_site_id.map(str::trim)
        } else {
            requested_site_id
        }
        .filter(|s| !s.is_empty()))
    } else {
        let actor_site = if maintenance {
            actor
                .site_id
                .as_deref()
                .ok_or_else(|| MaintError::Forbidden("Utilisateur sans site rattaché".into()))?
        } else {
            actor.site_id.as_deref().unwrap_or_default()
        };
        if let Some(req_sid) = requested_site_id {
            if !maintenance && req_sid != actor_site {
                return Err(MaintError::NotFound("Site".into()));
            }
        }
        Ok(Some(actor_site))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Role;

    fn outcome<T: std::fmt::Debug>(result: Result<T, MaintError>) -> String {
        match result {
            Ok(value) => format!("{value:?}"),
            Err(MaintError::BadRequest(message)) => format!("400:{message}"),
            Err(MaintError::Forbidden(message)) => format!("403:{message}"),
            Err(MaintError::NotFound(message)) => format!("404:{message}"),
            Err(other) => panic!("Erreur inattendue : {other}"),
        }
    }

    #[test]
    fn c5_conventions_de_site_et_messages_inchanges() {
        use SitePolicy::{Maintenance, Strict};
        let admin = Actor::new("admin", None, Role::Admin, []);
        let user = Actor::new("tech", Some("a".into()), Role::Technicien, []);
        let without_site = Actor::new("tech", None, Role::Technicien, []);
        for (actor, requested, policy, create, query) in [
            (&admin, None, Strict, "400:Le site est obligatoire", "None"),
            (
                &admin,
                None,
                Maintenance,
                "400:Le site_id est requis pour un administrateur",
                "None",
            ),
            (&admin, Some(" a "), Strict, "\"a\"", "Some(\" a \")"),
            (&admin, Some(" a "), Maintenance, "\"a\"", "Some(\"a\")"),
            (
                &admin,
                Some(" "),
                Strict,
                "400:Le site est obligatoire",
                "Some(\" \")",
            ),
            (
                &admin,
                Some(" "),
                Maintenance,
                "400:Le site_id est requis pour un administrateur",
                "None",
            ),
            (&user, None, Strict, "\"a\"", "Some(\"a\")"),
            (&user, Some("a"), Strict, "\"a\"", "Some(\"a\")"),
            (
                &user,
                Some("b"),
                Strict,
                "403:Accès refusé : vous ne pouvez pas créer de ressource sur un autre site",
                "404:Site",
            ),
            (&user, Some("b"), Maintenance, "\"a\"", "Some(\"a\")"),
            (
                &without_site,
                None,
                Strict,
                "403:Aucun site rattaché",
                "Some(\"\")",
            ),
            (
                &without_site,
                None,
                Maintenance,
                "403:Utilisateur sans site rattaché",
                "403:Utilisateur sans site rattaché",
            ),
        ] {
            assert_eq!(
                outcome(resolve_site_id_for_create(actor, requested, policy)),
                create
            );
            assert_eq!(
                outcome(resolve_site_id_for_query(actor, requested, policy)),
                query
            );
        }
    }
}
