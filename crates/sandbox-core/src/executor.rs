//! Sandbox execution: run commands and read/write files inside a sandbox,
//! independent of where that sandbox lives.
//!
//! Native targets back this with OS processes and the host filesystem; the
//! browser backs it with a WebAssembly shell (`wasmsh`) and OPFS. The trait is
//! deliberately free of `std` process and path types so it compiles for
//! `wasm32-unknown-unknown` (see `docs/docs/WASM/README.md`).

use async_trait::async_trait;

use crate::error::SandboxError;

/// A command to run inside a sandbox.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecRequest {
    /// Program and arguments: `argv[0]` is the program. No shell wrapping.
    pub argv: Vec<String>,
    /// Working directory inside the sandbox; `None` means the sandbox root.
    pub cwd: Option<String>,
    /// Bytes piped to the command's standard input.
    pub stdin: Option<Vec<u8>>,
    /// Additional environment variables.
    pub env: Vec<(String, String)>,
}

impl ExecRequest {
    /// Build a request from an argument vector.
    pub fn new<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            argv: argv.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// Set the working directory (relative to the sandbox root).
    #[must_use]
    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Pipe bytes to the command's standard input.
    #[must_use]
    pub fn with_stdin(mut self, stdin: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(stdin.into());
        self
    }

    /// Add an environment variable.
    #[must_use]
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

/// The captured result of [`SandboxExecutor::run`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecOutput {
    /// Exit status code (`0` is success; negative when the sandbox could not
    /// report one).
    pub status: i32,
    /// Captured standard output.
    pub stdout: Vec<u8>,
    /// Captured standard error.
    pub stderr: Vec<u8>,
}

impl ExecOutput {
    /// Whether the command exited successfully.
    pub fn success(&self) -> bool {
        self.status == 0
    }

    /// Standard output decoded as text (invalid UTF-8 replaced).
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Standard error decoded as text (invalid UTF-8 replaced).
    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// Runs commands and reads/writes files inside a sandbox.
///
/// Native targets keep the `Send + Sync` bounds so executors can be registered
/// on multi-threaded runtimes.
#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
pub trait SandboxExecutor: Send + Sync {
    /// Run one command and capture its output.
    async fn run(&self, request: ExecRequest) -> Result<ExecOutput, SandboxError>;
    /// Read a file inside the sandbox.
    async fn read_file(&self, path: &str) -> Result<Vec<u8>, SandboxError>;
    /// Write a file inside the sandbox, creating parent directories.
    async fn write_file(&self, path: &str, contents: &[u8]) -> Result<(), SandboxError>;
}

/// Runs commands and reads/writes files inside a sandbox (browser variant).
///
/// The browser sandbox executes through WebAssembly, whose futures are `!Send`
/// and whose runtime is single-threaded, so the `Send + Sync` bounds are
/// dropped (see `docs/docs/WASM/README.md`).
#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
pub trait SandboxExecutor {
    /// Run one command and capture its output.
    async fn run(&self, request: ExecRequest) -> Result<ExecOutput, SandboxError>;
    /// Read a file inside the sandbox.
    async fn read_file(&self, path: &str) -> Result<Vec<u8>, SandboxError>;
    /// Write a file inside the sandbox, creating parent directories.
    async fn write_file(&self, path: &str, contents: &[u8]) -> Result<(), SandboxError>;
}

/// A [`SandboxExecutor`] backed by OS processes and the host filesystem,
/// rooted at a sandbox directory.
///
/// Native only (see the `native` feature): it spawns child processes.
#[cfg(feature = "native")]
pub struct ProcessSandboxExecutor {
    root: std::path::PathBuf,
}

#[cfg(feature = "native")]
impl ProcessSandboxExecutor {
    /// Create an executor rooted at `root`. Every path passed to the executor
    /// resolves inside it.
    pub fn new(root: impl Into<std::path::PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The sandbox root.
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// Resolve a sandbox-relative path, rejecting escapes.
    fn resolve(&self, path: &str) -> Result<std::path::PathBuf, SandboxError> {
        let candidate = std::path::Path::new(path);
        let escapes = candidate.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        });
        if escapes {
            return Err(SandboxError::Other(format!(
                "path must stay inside the sandbox: {path}"
            )));
        }
        Ok(self.root.join(candidate))
    }
}

#[cfg(feature = "native")]
#[async_trait]
impl SandboxExecutor for ProcessSandboxExecutor {
    async fn run(&self, request: ExecRequest) -> Result<ExecOutput, SandboxError> {
        let Some((program, args)) = request.argv.split_first() else {
            return Err(SandboxError::CommandError(
                "empty argv: no program to run".to_owned(),
            ));
        };

        let cwd = match &request.cwd {
            Some(dir) => self.resolve(dir)?,
            None => self.root.clone(),
        };

        let mut command = tokio::process::Command::new(program);
        command
            .args(args)
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        for (key, value) in &request.env {
            command.env(key, value);
        }

        let mut child = command.spawn()?;
        if let Some(bytes) = request.stdin
            && let Some(mut stdin) = child.stdin.take()
        {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(&bytes).await?;
            // Dropping `stdin` closes the pipe so the child sees EOF.
        }

        let output = child.wait_with_output().await?;
        Ok(ExecOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    async fn read_file(&self, path: &str) -> Result<Vec<u8>, SandboxError> {
        Ok(tokio::fs::read(self.resolve(path)?).await?)
    }

    async fn write_file(&self, path: &str, contents: &[u8]) -> Result<(), SandboxError> {
        let resolved = self.resolve(path)?;
        if let Some(parent) = resolved.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(resolved, contents).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A portable mock, also proving the trait is object-safe.
    struct MockExecutor {
        files: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    }

    impl MockExecutor {
        fn new() -> Self {
            Self {
                files: std::sync::Mutex::new(std::collections::HashMap::new()),
            }
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), async_trait)]
    #[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
    impl SandboxExecutor for MockExecutor {
        async fn run(&self, request: ExecRequest) -> Result<ExecOutput, SandboxError> {
            Ok(ExecOutput {
                status: 0,
                stdout: request.argv.join(" ").into_bytes(),
                stderr: Vec::new(),
            })
        }

        async fn read_file(&self, path: &str) -> Result<Vec<u8>, SandboxError> {
            self.files
                .lock()
                .expect("bug: mock executor mutex poisoned")
                .get(path)
                .cloned()
                .ok_or_else(|| SandboxError::Other(format!("no such file: {path}")))
        }

        async fn write_file(&self, path: &str, contents: &[u8]) -> Result<(), SandboxError> {
            self.files
                .lock()
                .expect("bug: mock executor mutex poisoned")
                .insert(path.to_owned(), contents.to_vec());
            Ok(())
        }
    }

    #[test]
    fn exec_request_builder_sets_every_field() {
        let request = ExecRequest::new(["bash", "-c", "echo hi"])
            .with_cwd("work")
            .with_stdin("input")
            .with_env("KEY", "VALUE");
        assert_eq!(request.argv, vec!["bash", "-c", "echo hi"]);
        assert_eq!(request.cwd.as_deref(), Some("work"));
        assert_eq!(request.stdin.as_deref(), Some(b"input".as_slice()));
        assert_eq!(request.env, vec![("KEY".to_owned(), "VALUE".to_owned())]);
    }

    #[cfg(feature = "native")]
    #[tokio::test]
    async fn mock_executor_round_trips_through_the_trait_object() {
        let executor: Box<dyn SandboxExecutor> = Box::new(MockExecutor::new());
        executor
            .write_file("notes/todo.txt", b"ship it")
            .await
            .expect("write");
        let contents = executor.read_file("notes/todo.txt").await.expect("read");
        assert_eq!(contents, b"ship it");

        let output = executor
            .run(ExecRequest::new(["echo", "hello"]))
            .await
            .expect("run");
        assert!(output.success());
        assert_eq!(output.stdout_text(), "echo hello");
    }

    #[cfg(feature = "native")]
    #[tokio::test]
    async fn process_executor_runs_commands_and_respects_the_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let executor = ProcessSandboxExecutor::new(dir.path());

        let output = executor
            .run(ExecRequest::new(["echo", "hello"]))
            .await
            .expect("run echo");
        assert!(output.success());
        assert_eq!(output.stdout_text().trim(), "hello");

        executor
            .write_file("nested/file.txt", b"contents")
            .await
            .expect("write");
        assert_eq!(
            executor.read_file("nested/file.txt").await.expect("read"),
            b"contents"
        );

        let escape = executor.read_file("../outside.txt").await;
        assert!(escape.is_err(), "paths must not escape the sandbox root");
    }
}
