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
