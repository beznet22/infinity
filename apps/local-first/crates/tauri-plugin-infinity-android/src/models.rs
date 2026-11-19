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
    /// Which screen the client is showing: `chat_list` when the session is
    /// linked, otherwise `login`, `phone_entry`, `pairing_code` or unknown.
    pub stage: Option<String>,
    /// Whether the chat list is rendered, i.e. the linked session was restored.
    pub chat_list: bool,
    /// Whether the "link with phone number instead" link was found and clicked.
    pub phone_link_clicked: bool,
    /// Number of `<input>` elements on screen (the phone form has one).
    pub phone_inputs: u32,
    /// The 8-character pairing code, once the client displays it.
    pub pairing_code: Option<String>,
    /// Layout metrics, so a too-wide desktop layout is measurable rather than guessed.
    pub inner_width: f64,
    pub client_width: f64,
    pub scroll_width: f64,
    pub device_pixel_ratio: f64,
    pub visual_scale: Option<f64>,
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

/// Result of submitting a phone number to the linking form.
///
/// `stage` is `pairing_code` when WhatsApp displayed the 8-character code the
/// user has to type into the app on this phone.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PairOutcome {
    pub stage: Option<String>,
    pub pairing_code: Option<String>,
    pub phone_inputs: u32,
    pub body_snippet: Option<String>,
    pub error: Option<String>,
}

/// Whether the linked client finished loading and is ready to be driven.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChatState {
    pub stage: Option<String>,
    /// True once the chat list renders (the session is linked and synced).
    pub chat_list: bool,
    pub chat_rows: u32,
    pub body_snippet: Option<String>,
    pub error: Option<String>,
}

/// Recent messages of one conversation.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReadResult {
    pub count: u32,
    pub messages: Vec<String>,
    pub chat_header: Option<String>,
    pub error: Option<String>,
}

/// Outcome of typing (and optionally sending) a message.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SendResult {
    pub sent: bool,
    /// True when the text is sitting in the composer unsent (`dry_run`).
    pub drafted: bool,
    pub chars: u32,
    pub reason: Option<String>,
    pub error: Option<String>,
}
