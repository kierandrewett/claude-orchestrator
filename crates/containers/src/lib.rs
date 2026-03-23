//! Docker container lifecycle management via bollard.

pub mod auth;
pub mod config;
pub mod handle;
pub mod manager;
pub mod profiles;

pub use auth::{AuthCredentials, AuthManager};
pub use config::{ContainerConfig, MountPoint, NetworkMode, SessionData};
pub use handle::ContainerHandle;
pub use manager::{ContainerManager, SlashCommand};
pub use profiles::{load_profiles, Profile};
