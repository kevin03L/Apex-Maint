//! Logique et types métier purs (sans I/O ni dépendance web).

pub mod actor;
pub mod pagination;
pub mod permission;

pub use actor::{Actor, Role};
pub use pagination::Cursor;
pub use permission::Permission;
