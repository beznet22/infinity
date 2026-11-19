//! HTTP transport for the relay.
//!
//! The relay contract is three calls and deliberately dumb: push events (dedupe
//! by id), pull events after a sequence, list sessions. A local dev server and a
//! Cloudflare Worker can implement the same three.

use serde::{Deserialize, Serialize};

use crate::SyncError;
use crate::types::{SessionId, StoredEvent, SyncEvent};

/// What the relay accepted for a push.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PushAck {
    /// Number of events the relay had not seen before.
    pub accepted: usize,
    /// Highest sequence known for each pushed session.
    pub last_seq: u64,
}

/// Client for the relay's push/pull API.
#[derive(Debug, Clone)]
pub struct HttpTransport {
    token: Option<String>,
    base_url: String,
    client: reqwest::Client,
}

impl HttpTransport {
    /// Send a bearer token on every call; the relay requires one when configured.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn new(base_url: impl Into<String>) -> Result<Self, SyncError> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(SyncError::Transport)?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            client,
            token: None,
        })
    }

    /// Build a transport around an existing client.
    ///
    /// Required on Android: reqwest's default TLS stack routes through
    /// `rustls-platform-verifier`, whose Android backend stalls the handshake
    /// unless it was initialized (see the fork's D21). Hosts pass a client built
    /// from compiled-in roots instead.
    pub fn with_client(client: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            client,
            token: None,
        }
    }

    /// Attach the relay token when one is configured.
    fn authorized(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    /// Push events. Retrying is safe: the relay ignores ids it already has.
    pub async fn push(&self, events: &[SyncEvent]) -> Result<PushAck, SyncError> {
        if events.is_empty() {
            return Ok(PushAck::default());
        }
        let response = self
            .authorized(self.client.post(format!("{}/v1/sync/push", self.base_url)))
            .json(&PushRequest { events })
            .send()
            .await
            .map_err(SyncError::Transport)?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(SyncError::Relay {
                status: status.as_u16(),
                body,
            });
        }
        response.json().await.map_err(SyncError::Transport)
    }

    /// Events of one session after a relay position, oldest first.
    pub async fn pull(
        &self,
        session: &SessionId,
        after_position: i64,
    ) -> Result<Vec<StoredEvent>, SyncError> {
        let response = self
            .authorized(self.client.get(format!("{}/v1/sync/pull", self.base_url)))
            .query(&[
                ("session", session.as_str()),
                ("after", &after_position.to_string()),
            ])
            .send()
            .await
            .map_err(SyncError::Transport)?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(SyncError::Relay {
                status: status.as_u16(),
                body,
            });
        }
        response.json().await.map_err(SyncError::Transport)
    }

    /// Every session the relay knows about.
    pub async fn sessions(&self) -> Result<Vec<SessionId>, SyncError> {
        let response = self
            .authorized(
                self.client
                    .get(format!("{}/v1/sync/sessions", self.base_url)),
            )
            .send()
            .await
            .map_err(SyncError::Transport)?;
        let raw: Vec<String> = response.json().await.map_err(SyncError::Transport)?;
        Ok(raw.into_iter().map(SessionId::new).collect())
    }
}

#[derive(Serialize)]
struct PushRequest<'a> {
    events: &'a [SyncEvent],
}
