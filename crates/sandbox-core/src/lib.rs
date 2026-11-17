pub mod callback;
pub mod error;
/// Sandbox command/file execution, portable across targets.
pub mod executor;
/// Git process plumbing (native only, see the `native` feature).
#[cfg(feature = "native")]
pub mod git;
/// Jujutsu process plumbing (native only, see the `native` feature).
#[cfg(feature = "native")]
pub mod jj;
pub mod metadata;
pub mod sandbox;
/// The sandbox HTTP server (native only, see the `native` feature).
#[cfg(feature = "native")]
pub mod server;
pub mod types;

/// The thread identifier used to key sandboxes and repo state (RAP's
/// `group_id`). Re-exported so backend crates can name it without a direct
/// `rap-protocol` dependency.
pub use rap_protocol::ThreadId;

pub const DEFAULT_SANDBOX_NAME: &str = "Infinity 🤖";
pub const DEFAULT_SANDBOX_EMAIL: &str = "infinity@hydro.run";

/// Walk up from `start` to find the repository root: the closest ancestor
/// (including `start` itself) containing a `.jj` directory or a `.git`
/// entry (directory, or file for worktrees/submodules).
///
/// This allows repositories to be detected even when the provided path is
/// nested inside the repo — important for jj repos, where subdirectories
/// carry no marker of their own (unlike git, where commands walk up
/// themselves).
pub fn find_repo_root(start: &std::path::Path) -> Option<std::path::PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(".jj").is_dir() || dir.join(".git").exists())
        .map(std::path::Path::to_path_buf)
}
