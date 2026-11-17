//! HTTP transport for the relay.
//!
//! The relay contract is three calls and deliberately dumb: push events (dedupe
//! by id), pull events after a sequence, list sessions. A local dev server and a
//! Cloudflare Worker can implement the same three.

use serde::{Deserialize, Serialize};

use crate::types::{SessionId, StoredEvent, SyncEvent};
use crate::SyncError;

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
    base_url: String,
    client: reqwest::Client,
}

impl HttpTransport {
    pub fn new(base_url: impl Into<String>) -> Result<Self, SyncError> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(SyncError::Transport)?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client,
        })
    }

    /// Push events. Retrying is safe: the relay ignores ids it already has.
    pub async fn push(&self, events: &[SyncEvent]) -> Result<PushAck, SyncError> {
        if events.is_empty() {
            return Ok(PushAck::default());
        }
        let response = self
            .client
            .post(format!("{}/v1/sync/push", self.base_url))
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
            .client
            .get(format!("{}/v1/sync/pull", self.base_url))
            .query(&[("session", session.as_str()), ("after", &after_position.to_string())])
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
            .client
            .get(format!("{}/v1/sync/sessions", self.base_url))
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
