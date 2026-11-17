use serde::{Deserialize, Serialize};

/// Result of counting the contacts visible to the app.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactCount {
    /// Number of contact rows returned by `ContactsContract.Contacts`.
    pub count: u32,
    /// Whether `READ_CONTACTS` was granted when the count ran.
    pub granted: bool,
}

/// Availability probe for the Shizuku privileged server.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShizukuStatus {
    /// Whether the Shizuku app is installed.
    pub installed: bool,
    /// Whether the Shizuku binder provider answered a ping, i.e. the privileged
    /// server is running. Always `false` when the app is not installed.
    pub running: bool,
    /// Android API level, so a consumer can reason about what the probe covered.
    pub sdk_int: u32,
}

/// Result of loading web.whatsapp.com in an overlay WebView.
///
/// This reports what the client needs in order to boot. It cannot report a
/// paired session: linking requires a human to scan the QR with the phone.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WhatsAppProbe {
    /// Final URL after redirects.
    pub url: Option<String>,
    pub title: Option<String>,
    /// `document.readyState` when the probe ran.
    pub ready_state: Option<String>,
    /// Whether any `<canvas>` is present — the QR code is drawn on one.
    pub qr_canvas: bool,
    pub canvas_count: u32,
    /// Capabilities the real client depends on (Signal keys, crypto, app shell).
    pub service_worker: bool,
    pub indexed_db: bool,
    pub subtle_crypto: bool,
    /// First 300 characters of visible text, enough to spot a block page.
    pub body_snippet: Option<String>,
    pub error: Option<String>,
}
