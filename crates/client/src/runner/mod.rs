pub mod native;

use anyhow::Result;
use async_trait::async_trait;
use tokio::process::Child;

use crate::session_runner::SessionConfig;

/// Abstracts how a Claude session process is spawned and killed.
///
/// The returned [`Child`] must have stdin/stdout/stderr piped — the generic
/// session loop in [`crate::session_runner`] owns all I/O from that point.
///
/// To add VM mode later: implement this trait in a `runner::vm` module and
/// select it at startup based on config.
#[async_trait]
pub trait Runner: Send + Sync {
    async fn spawn(&self, config: &SessionConfig) -> Result<Child>;
    /// Hard-kill the session — process exits, session is over.
    async fn kill(&self, child: &mut Child, config: &SessionConfig);
    /// Interrupt the current response (SIGINT) — process keeps running,
    /// Claude returns to the input prompt.
    async fn interrupt(&self, child: &mut Child, config: &SessionConfig);
}
