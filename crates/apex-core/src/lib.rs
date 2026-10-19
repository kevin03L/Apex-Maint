//! # apex-core — Moteur métier GMAO d'Apex Maint
//!
//! Contient la logique métier pure (sans I/O ni dépendance à Axum), les dépôts SQLite et les
//! services de transaction.

pub mod db;
pub mod documents;
pub mod domain;
pub mod error;
pub mod exploitation;
#[cfg(test)]
#[path = "exploitation_tests.rs"]
mod exploitation_tests;
pub mod integration;
#[cfg(test)]
#[path = "integration_tests.rs"]
mod integration_tests;
pub mod interventions;
pub mod maintenance;
pub mod pilotage;
pub mod referentiel;
pub mod services;
pub mod stock;
pub mod store;
pub mod terrain;
#[cfg(test)]
mod terrain_tests;

pub use domain::{actor::Actor, actor::Role, pagination::Cursor, permission::Permission};
pub use error::MaintError;

#[cfg(test)]
mod audit_tests;
#[cfg(test)]
mod tests;
