/// HTTP and caching abstractions for RAP client operations.
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Abstraction over HTTP client for RAP tool invocation and toolset discovery.
///
/// Native targets keep the `Send + Sync` bounds so client futures can cross
/// threads (servers spawn work freely).
#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
pub trait HttpClient: Send + Sync + Clone {
    /// Error type returned by HTTP operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// POST a JSON body to the given URL. Returns the HTTP status code.
    async fn post(&self, url: &str, body: &str) -> Result<u16, Self::Error>;
    /// GET the given URL. Returns the HTTP status code and response body.
    async fn get(&self, url: &str) -> Result<(u16, Vec<u8>), Self::Error>;
}

/// Abstraction over HTTP client for RAP tool invocation and toolset discovery
/// (browser variant: no `Send`/`Sync` bounds, see the native definition).
///
/// On `wasm32-unknown-unknown` the futures are deliberately `!Send`: reqwest's
/// browser backend wraps thread-local JS objects and the wasm runtime is
/// single-threaded (work item W2; overview §9.4).
#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
pub trait HttpClient: Clone {
    /// Error type returned by HTTP operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// POST a JSON body to the given URL. Returns the HTTP status code.
    async fn post(&self, url: &str, body: &str) -> Result<u16, Self::Error>;
    /// GET the given URL. Returns the HTTP status code and response body.
    async fn get(&self, url: &str) -> Result<(u16, Vec<u8>), Self::Error>;
}

/// Abstraction over toolset manifest caching.
#[async_trait]
pub trait ToolsetCache: Send + Sync {
    /// Error type returned by cache operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Look up a cached value by key.
    async fn get_cached(&self, cache_key: &str) -> Result<Option<String>, Self::Error>;
    /// Store a value in the cache.
    async fn put_cache(&self, cache_key: &str, json: &str) -> Result<(), Self::Error>;
}

// ── SimpleHttpClient ──

/// Error type for [`SimpleHttpClient`].
#[derive(Debug)]
pub struct SimpleHttpError(String);
impl std::fmt::Display for SimpleHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for SimpleHttpError {}

/// Plain reqwest-based [`HttpClient`] with no authentication.
#[derive(Clone)]
pub struct SimpleHttpClient {
    client: reqwest::Client,
}

impl Default for SimpleHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SimpleHttpClient {
    /// Create a new client with default settings.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Shared POST body for the two target-specific [`HttpClient`] impls.
    async fn post_request(&self, url: &str, body: &str) -> Result<u16, SimpleHttpError> {
        let resp = self
            .client
            .post(url)
            .header("content-type", "application/json")
            .body(body.to_owned())
            .send()
            .await
            .map_err(|e| SimpleHttpError(e.to_string()))?;
        Ok(resp.status().as_u16())
    }

    /// Shared GET body for the two target-specific [`HttpClient`] impls.
    async fn get_request(&self, url: &str) -> Result<(u16, Vec<u8>), SimpleHttpError> {
        let resp = self
            .client
            .get(url)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|e| SimpleHttpError(e.to_string()))?;
        let status = resp.status().as_u16();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| SimpleHttpError(e.to_string()))?;
        Ok((status, bytes.to_vec()))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl HttpClient for SimpleHttpClient {
    type Error = SimpleHttpError;

    async fn post(&self, url: &str, body: &str) -> Result<u16, SimpleHttpError> {
        self.post_request(url, body).await
    }

    async fn get(&self, url: &str) -> Result<(u16, Vec<u8>), SimpleHttpError> {
        self.get_request(url).await
    }
}

#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
impl HttpClient for SimpleHttpClient {
    type Error = SimpleHttpError;

    async fn post(&self, url: &str, body: &str) -> Result<u16, SimpleHttpError> {
        self.post_request(url, body).await
    }

    async fn get(&self, url: &str) -> Result<(u16, Vec<u8>), SimpleHttpError> {
        self.get_request(url).await
    }
}

// ── InMemoryToolsetCache ──

/// Error type for [`InMemoryToolsetCache`].
#[derive(Debug)]
pub struct CacheError(String);
impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for CacheError {}

/// In-memory [`ToolsetCache`] backed by a `HashMap`. Useful for testing and CLI usage.
pub struct InMemoryToolsetCache {
    store: Arc<Mutex<HashMap<String, String>>>,
}

impl Default for InMemoryToolsetCache {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryToolsetCache {
    /// Create a new empty cache.
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ToolsetCache for InMemoryToolsetCache {
    type Error = CacheError;

    async fn get_cached(&self, key: &str) -> Result<Option<String>, CacheError> {
        Ok(self
            .store
            .lock()
            .expect("bug: cache mutex poisoned")
            .get(key)
            .cloned())
    }

    async fn put_cache(&self, key: &str, json: &str) -> Result<(), CacheError> {
        self.store
            .lock()
            .expect("bug: cache mutex poisoned")
            .insert(key.to_owned(), json.to_owned());
        Ok(())
    }
}
