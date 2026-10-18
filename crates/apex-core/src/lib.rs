//! # apex-core — Moteur métier GMAO d'Apex Maint
//!
//! Contient la logique métier pure (sans I/O ni dépendance à Axum), les dépôts SQLite et les
//! services de transaction.

pub mod db;
pub mod documents;
pub mod domain;
pub mod error;
pub mod interventions;
pub mod maintenance;
pub mod pilotage;
pub mod referentiel;
pub mod services;
pub mod stock;
pub mod store;

pub use domain::{actor::Actor, actor::Role, pagination::Cursor, permission::Permission};
pub use error::MaintError;

#[cfg(test)]
mod tests;
