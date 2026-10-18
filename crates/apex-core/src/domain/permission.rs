//! Clés de permission du système RBAC d'Apex Maint.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::MaintError;

/// Erreur lors de l'analyse d'une chaîne de permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsePermissionError(pub String);

impl fmt::Display for ParsePermissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "permission inconnue : « {} »", self.0)
    }
}

impl std::error::Error for ParsePermissionError {}

/// Énumération de toutes les permissions définies dans PLAN §1.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Permission {
    DashboardView,
    ReportingView,
    SettingsView,
    SettingsEdit,
    AuditView,
    EquipmentsView,
    EquipmentsCreate,
    EquipmentsEdit,
    EquipmentsDelete,
    InterventionsView,
    InterventionsCreate,
    InterventionsEdit,
    InterventionsDelete,
    PartsView,
    PartsCreate,
    PartsEdit,
    PartsDelete,
    ProductionLinesView,
    ProductionLinesCreate,
    ProductionLinesEdit,
    ProductionLinesDelete,
    DocumentsView,
    DocumentsCreate,
    DocumentsDelete,
    LocationsView,
    LocationsManage,
    MaintenanceView,
    MaintenanceManage,
    StockView,
    StockMovements,
    SuppliersView,
    SuppliersManage,
    UsersView,
    UsersManage,
}

impl Permission {
    /// Liste complète de toutes les permissions du système.
    pub const ALL: &'static [Permission] = &[
        Self::DashboardView,
        Self::ReportingView,
        Self::SettingsView,
        Self::SettingsEdit,
        Self::AuditView,
        Self::EquipmentsView,
        Self::EquipmentsCreate,
        Self::EquipmentsEdit,
        Self::EquipmentsDelete,
        Self::InterventionsView,
        Self::InterventionsCreate,
        Self::InterventionsEdit,
        Self::InterventionsDelete,
        Self::PartsView,
        Self::PartsCreate,
        Self::PartsEdit,
        Self::PartsDelete,
        Self::ProductionLinesView,
        Self::ProductionLinesCreate,
        Self::ProductionLinesEdit,
        Self::ProductionLinesDelete,
        Self::DocumentsView,
        Self::DocumentsCreate,
        Self::DocumentsDelete,
        Self::LocationsView,
        Self::LocationsManage,
        Self::MaintenanceView,
        Self::MaintenanceManage,
        Self::StockView,
        Self::StockMovements,
        Self::SuppliersView,
        Self::SuppliersManage,
        Self::UsersView,
        Self::UsersManage,
    ];

    /// Représentation textuelle exacte de la clé de permission (ex. `dashboard.view`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DashboardView => "dashboard.view",
            Self::ReportingView => "reporting.view",
            Self::SettingsView => "settings.view",
            Self::SettingsEdit => "settings.edit",
            Self::AuditView => "audit.view",
            Self::EquipmentsView => "equipments.view",
            Self::EquipmentsCreate => "equipments.create",
            Self::EquipmentsEdit => "equipments.edit",
            Self::EquipmentsDelete => "equipments.delete",
            Self::InterventionsView => "interventions.view",
            Self::InterventionsCreate => "interventions.create",
            Self::InterventionsEdit => "interventions.edit",
            Self::InterventionsDelete => "interventions.delete",
            Self::PartsView => "parts.view",
            Self::PartsCreate => "parts.create",
            Self::PartsEdit => "parts.edit",
            Self::PartsDelete => "parts.delete",
            Self::ProductionLinesView => "production_lines.view",
            Self::ProductionLinesCreate => "production_lines.create",
            Self::ProductionLinesEdit => "production_lines.edit",
            Self::ProductionLinesDelete => "production_lines.delete",
            Self::DocumentsView => "documents.view",
            Self::DocumentsCreate => "documents.create",
            Self::DocumentsDelete => "documents.delete",
            Self::LocationsView => "locations.view",
            Self::LocationsManage => "locations.manage",
            Self::MaintenanceView => "maintenance.view",
            Self::MaintenanceManage => "maintenance.manage",
            Self::StockView => "stock.view",
            Self::StockMovements => "stock.movements",
            Self::SuppliersView => "suppliers.view",
            Self::SuppliersManage => "suppliers.manage",
            Self::UsersView => "users.view",
            Self::UsersManage => "users.manage",
        }
    }

    /// Analyse une clé de permission à partir d'une chaîne.
    pub fn parse(s: &str) -> Result<Self, ParsePermissionError> {
        match s.trim() {
            "dashboard.view" => Ok(Self::DashboardView),
            "reporting.view" => Ok(Self::ReportingView),
            "settings.view" => Ok(Self::SettingsView),
            "settings.edit" => Ok(Self::SettingsEdit),
            "audit.view" => Ok(Self::AuditView),
            "equipments.view" => Ok(Self::EquipmentsView),
            "equipments.create" => Ok(Self::EquipmentsCreate),
            "equipments.edit" => Ok(Self::EquipmentsEdit),
            "equipments.delete" => Ok(Self::EquipmentsDelete),
            "interventions.view" => Ok(Self::InterventionsView),
            "interventions.create" => Ok(Self::InterventionsCreate),
            "interventions.edit" => Ok(Self::InterventionsEdit),
            "interventions.delete" => Ok(Self::InterventionsDelete),
            "parts.view" => Ok(Self::PartsView),
            "parts.create" => Ok(Self::PartsCreate),
            "parts.edit" => Ok(Self::PartsEdit),
            "parts.delete" => Ok(Self::PartsDelete),
            "production_lines.view" => Ok(Self::ProductionLinesView),
            "production_lines.create" => Ok(Self::ProductionLinesCreate),
            "production_lines.edit" => Ok(Self::ProductionLinesEdit),
            "production_lines.delete" => Ok(Self::ProductionLinesDelete),
            "documents.view" => Ok(Self::DocumentsView),
            "documents.create" => Ok(Self::DocumentsCreate),
            "documents.delete" => Ok(Self::DocumentsDelete),
            "locations.view" => Ok(Self::LocationsView),
            "locations.manage" => Ok(Self::LocationsManage),
            "maintenance.view" => Ok(Self::MaintenanceView),
            "maintenance.manage" => Ok(Self::MaintenanceManage),
            "stock.view" => Ok(Self::StockView),
            "stock.movements" => Ok(Self::StockMovements),
            "suppliers.view" => Ok(Self::SuppliersView),
            "suppliers.manage" => Ok(Self::SuppliersManage),
            "users.view" => Ok(Self::UsersView),
            "users.manage" => Ok(Self::UsersManage),
            other => Err(ParsePermissionError(other.to_string())),
        }
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Permission {
    type Err = ParsePermissionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl TryFrom<&str> for Permission {
    type Error = MaintError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::parse(s).map_err(|e| MaintError::BadRequest(e.to_string()))
    }
}

impl Serialize for Permission {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Permission {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_permissions_roundtrip() {
        assert_eq!(Permission::ALL.len(), 34);
        for &perm in Permission::ALL {
            let s = perm.as_str();
            let parsed = Permission::parse(s).expect("analyse de la permission");
            assert_eq!(parsed, perm);
            assert_eq!(parsed.to_string(), s);
        }
    }

    #[test]
    fn invalid_permission_rejected() {
        assert!(Permission::parse("inconnue.view").is_err());
        assert!(Permission::parse("").is_err());
    }

    #[test]
    fn serde_json_roundtrip() {
        for &perm in Permission::ALL {
            let json = serde_json::to_string(&perm).unwrap();
            let de: Permission = serde_json::from_str(&json).unwrap();
            assert_eq!(de, perm);
        }
    }
}
