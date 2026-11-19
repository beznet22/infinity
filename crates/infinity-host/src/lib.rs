//! Host assembly point for the Infinity hybrid fork.
//!
//! [`HostCtx`] bundles the capabilities a host environment provides to the
//! agent core — conversation and state stores, an HTTP client, a toolset
//! cache, a sandbox executor and (optionally) an MCP transport factory — so
//! that consumers (the Tauri app, the browser runtime, the edge worker)
//! assemble them in one place instead of threading generics through their own
//! code.
//!
//! Why a separate crate: the pieces live in crates that do not depend on each
//! other (`infinity-agent-core`, `rap-client`, `sandbox-core`,
//! `infinity-mcp-bridge`). Bundling them here keeps the upstream crates free of
//! new coupling while still giving hosts a single contract (decision D20).
//!
//! # Targets
//!
//! The crate is target-neutral. Native builds enable the `native` feature by
//! default, which turns on the native surface of its dependencies; browser
//! builds use `default-features = false` (see `docs/docs/WASM/README.md`).

use infinity_agent_core::traits::{ConversationStore, StateStore};
use infinity_mcp_bridge::McpTransportFactory;
use rap_client::http::{HttpClient, ToolsetCache};
use sandbox_core::executor::SandboxExecutor;

/// The capabilities a host provides to the agent core.
///
/// Constructed once per host and passed around behind a shared reference;
/// every capability is injected rather than created internally, so the same
/// core runs on native, in a browser, or at the edge.
pub struct HostCtx<C, S, H, T, E> {
    conversation_store: C,
    state_store: S,
    http: H,
    toolset_cache: T,
    sandbox: E,
    mcp_transport: Option<McpTransportFactory>,
}

impl<C, S, H, T, E> HostCtx<C, S, H, T, E>
where
    C: ConversationStore,
    S: StateStore,
    H: HttpClient,
    T: ToolsetCache,
    E: SandboxExecutor,
{
    /// Assemble the required capabilities.
    pub fn new(
        conversation_store: C,
        state_store: S,
        http: H,
        toolset_cache: T,
        sandbox: E,
    ) -> Self {
        Self {
            conversation_store,
            state_store,
            http,
            toolset_cache,
            sandbox,
            mcp_transport: None,
        }
    }

    /// Attach an MCP transport factory. Hosts that expose no MCP servers leave
    /// it unset.
    #[must_use]
    pub fn with_mcp_transport(mut self, factory: McpTransportFactory) -> Self {
        self.mcp_transport = Some(factory);
        self
    }

    /// The conversation store threads are persisted in.
    pub fn conversation_store(&self) -> &C {
        &self.conversation_store
    }

    /// The per-thread state store.
    pub fn state_store(&self) -> &S {
        &self.state_store
    }

    /// The HTTP client used for RAP tool calls and toolset discovery.
    pub fn http(&self) -> &H {
        &self.http
    }

    /// The toolset manifest cache.
    pub fn toolset_cache(&self) -> &T {
        &self.toolset_cache
    }

    /// The executor commands run in.
    pub fn sandbox(&self) -> &E {
        &self.sandbox
    }

    /// The MCP transport factory, when the host exposes MCP servers.
    pub fn mcp_transport(&self) -> Option<&McpTransportFactory> {
        self.mcp_transport.as_ref()
    }

    /// Consume the bundle into its parts, for hosts that wire the core's
    /// builder directly.
    pub fn into_parts(self) -> (C, S, H, T, E, Option<McpTransportFactory>) {
        (
            self.conversation_store,
            self.state_store,
            self.http,
            self.toolset_cache,
            self.sandbox,
            self.mcp_transport,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use infinity_agent_core::stores::{InMemoryConversationStore, InMemoryStateStore};
    use rap_client::http::InMemoryToolsetCache;
    use sandbox_core::error::SandboxError;
    use sandbox_core::executor::{ExecOutput, ExecRequest};

    use super::*;

    #[derive(Clone)]
    struct MockHttp;

    #[cfg_attr(not(target_arch = "wasm32"), async_trait)]
    #[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
    impl HttpClient for MockHttp {
        type Error = std::io::Error;

        async fn post(&self, _url: &str, _body: &str) -> Result<u16, Self::Error> {
            Ok(202)
        }

        async fn get(&self, _url: &str) -> Result<(u16, Vec<u8>), Self::Error> {
            Ok((200, b"{}".to_vec()))
        }
    }

    struct MockSandbox {
        writes: Mutex<Vec<(String, Vec<u8>)>>,
    }

    #[cfg_attr(not(target_arch = "wasm32"), async_trait)]
    #[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
    impl SandboxExecutor for MockSandbox {
        async fn run(&self, request: ExecRequest) -> Result<ExecOutput, SandboxError> {
            Ok(ExecOutput {
                status: 0,
                stdout: request.argv.join(" ").into_bytes(),
                stderr: Vec::new(),
            })
        }

        async fn read_file(&self, _path: &str) -> Result<Vec<u8>, SandboxError> {
            Ok(Vec::new())
        }

        async fn write_file(&self, path: &str, contents: &[u8]) -> Result<(), SandboxError> {
            self.writes
                .lock()
                .expect("bug: mock sandbox mutex poisoned")
                .push((path.to_owned(), contents.to_vec()));
            Ok(())
        }
    }

    fn host() -> HostCtx<
        InMemoryConversationStore,
        InMemoryStateStore,
        MockHttp,
        InMemoryToolsetCache,
        MockSandbox,
    > {
        HostCtx::new(
            InMemoryConversationStore::new(),
            InMemoryStateStore::new(),
            MockHttp,
            InMemoryToolsetCache::new(),
            MockSandbox {
                writes: Mutex::new(Vec::new()),
            },
        )
    }

    #[tokio::test]
    async fn every_capability_is_reachable_through_the_bundle() {
        let ctx = host();

        assert_eq!(
            ctx.http()
                .post("https://example.test", "{}")
                .await
                .expect("post"),
            202
        );
        assert_eq!(
            ctx.http().get("https://example.test").await.expect("get").0,
            200
        );

        ctx.sandbox()
            .write_file("notes.txt", b"hi")
            .await
            .expect("write");
        assert_eq!(
            ctx.sandbox()
                .run(ExecRequest::new(["echo"]))
                .await
                .expect("run")
                .stdout_text(),
            "echo"
        );

        ctx.toolset_cache()
            .put_cache("key", "{}")
            .await
            .expect("cache put");
        assert_eq!(
            ctx.toolset_cache()
                .get_cached("key")
                .await
                .expect("cache get"),
            Some("{}".to_owned())
        );

        // Stores are exposed by reference; a clone is a handle to the same data.
        let _conversation = ctx.conversation_store().clone();
        let _state = ctx.state_store().clone();
    }

    #[test]
    fn mcp_transport_is_optional_and_parts_round_trip() {
        let ctx = host();
        assert!(ctx.mcp_transport().is_none());

        let (_c, _s, _h, _t, _e, mcp) = ctx.into_parts();
        assert!(mcp.is_none());
    }
}
