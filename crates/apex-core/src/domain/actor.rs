//! Acteur authentifié et rôles du système RBAC d'Apex Maint.

use std::{collections::HashSet, fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{domain::permission::Permission, error::MaintError};

/// Erreur lors de l'analyse d'un rôle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseRoleError(pub String);

impl fmt::Display for ParseRoleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rôle inconnu : « {} »", self.0)
    }
}

impl std::error::Error for ParseRoleError {}

/// Rôles d'utilisateurs disponibles dans l'application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Admin,
    Responsable,
    Production,
    Technicien,
}

impl Role {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Responsable => "responsable",
            Self::Production => "production",
            Self::Technicien => "technicien",
        }
    }

    pub fn parse(s: &str) -> Result<Self, ParseRoleError> {
        match s.trim() {
            "admin" => Ok(Self::Admin),
            "responsable" => Ok(Self::Responsable),
            "production" => Ok(Self::Production),
            "technicien" => Ok(Self::Technicien),
            other => Err(ParseRoleError(other.to_string())),
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = ParseRoleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for Role {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Role {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Acteur à l'origine d'une opération.
///
/// Obligatoire dans toute fonction de dépôt pour garantir le cloisonnement multi-sites
/// et le contrôle d'accès en profondeur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub user_id: String,
    pub site_id: Option<String>,
    pub role: Role,
    pub permissions: HashSet<Permission>,
}

impl Actor {
    /// Crée un nouvel acteur.
    pub fn new(
        user_id: impl Into<String>,
        site_id: Option<String>,
        role: Role,
        permissions: impl IntoIterator<Item = Permission>,
    ) -> Self {
        Self {
            user_id: user_id.into(),
            site_id,
            role,
            permissions: permissions.into_iter().collect(),
        }
    }

    /// Indique si l'acteur a le rôle Administrateur.
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }

    /// Vérifie si l'acteur possède la permission demandée.
    /// Un administrateur dispose implicitement de toutes les permissions.
    pub fn can(&self, perm: Permission) -> bool {
        self.is_admin() || self.permissions.contains(&perm)
    }

    /// Exige une permission sous peine d'erreur `MaintError::Forbidden`.
    pub fn require_permission(&self, perm: Permission) -> Result<(), MaintError> {
        if self.can(perm) {
            Ok(())
        } else {
            Err(MaintError::Forbidden(format!(
                "Permission manquante : {}",
                perm.as_str()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_parsing_and_display() {
        for role in [
            Role::Admin,
            Role::Responsable,
            Role::Production,
            Role::Technicien,
        ] {
            let s = role.as_str();
            assert_eq!(Role::parse(s).unwrap(), role);
            assert_eq!(role.to_string(), s);
        }
        assert!(Role::parse("inconnu").is_err());
    }

    #[test]
    fn admin_has_all_permissions_implicitly() {
        let admin = Actor::new("u1", None, Role::Admin, []);
        assert!(admin.is_admin());
        for &perm in Permission::ALL {
            assert!(admin.can(perm));
            assert!(admin.require_permission(perm).is_ok());
        }
    }

    #[test]
    fn non_admin_permissions_restricted() {
        let tech = Actor::new(
            "u2",
            Some("site-1".into()),
            Role::Technicien,
            [Permission::InterventionsView, Permission::StockView],
        );
        assert!(!tech.is_admin());
        assert!(tech.can(Permission::InterventionsView));
        assert!(tech.can(Permission::StockView));
        assert!(!tech.can(Permission::UsersManage));
        assert!(!tech.can(Permission::SettingsEdit));
        assert!(tech
            .require_permission(Permission::InterventionsView)
            .is_ok());
        assert!(tech.require_permission(Permission::UsersManage).is_err());
    }
}
