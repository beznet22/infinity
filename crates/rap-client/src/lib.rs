#![warn(missing_docs)]

//! RAP client library for interacting with RAP tool servers.
//!
//! Provides toolset discovery, tool invocation, lifecycle notifications,
//! and a callback server for receiving async results.

/// Local HTTP callback server for receiving async RAP tool results.
///
/// Native only: it binds a loopback TCP socket, which a browser cannot do.
/// Browser callback ingestion is an in-page seam (work item W3).
#[cfg(not(target_arch = "wasm32"))]
pub mod callback_server;
/// HTTP and caching abstractions for RAP client operations.
pub mod http;
/// Best-effort lifecycle notifications to RAP tool servers.
pub mod notifier;
/// Toolset discovery via `/.well-known/rap-toolset` endpoints.
pub mod toolset_loader;
