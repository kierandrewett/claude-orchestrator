//! Web dashboard backend — bridges the event bus to the React dashboard.

pub mod api;
pub mod backend;
pub mod ws;

pub use backend::WebBackend;
