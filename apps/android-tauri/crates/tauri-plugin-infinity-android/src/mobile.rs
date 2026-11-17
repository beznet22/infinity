use serde::de::DeserializeOwned;
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use crate::models::*;

/// Registers the Kotlin `InfinityAndroidPlugin` class with the Tauri runtime.
pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> crate::Result<InfinityAndroid<R>> {
    #[cfg(target_os = "android")]
    {
        let handle =
            api.register_android_plugin("com.plugin.infinity_android", "InfinityAndroidPlugin")?;
        Ok(InfinityAndroid(handle))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = api;
        Err(crate::Error::Unsupported(
            "infinity-android is only implemented for Android".to_string(),
        ))
    }
}

/// Access to the infinity-android APIs.
pub struct InfinityAndroid<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> InfinityAndroid<R> {
    /// Counts the contacts the app can read. Requests `READ_CONTACTS` when it has
    /// not been granted yet, so the first call may resolve only after the user
    /// answers the system prompt.
    pub fn count_contacts(&self) -> crate::Result<ContactCount> {
        self.0
            .run_mobile_plugin("countContacts", ())
            .map_err(Into::into)
    }

    /// Reports whether the linked client is ready to be driven.
    pub fn whatsapp_state(&self) -> crate::Result<ChatState> {
        self.0
            .run_mobile_plugin("whatsappState", ())
            .map_err(Into::into)
    }

    /// Opens a chat by name and returns its most recent messages.
    pub fn whatsapp_read(&self, chat: &str, limit: u32) -> crate::Result<ReadResult> {
        self.0
            .run_mobile_plugin(
                "whatsappRead",
                serde_json::json!({ "chat": chat, "limit": limit }),
            )
            .map_err(Into::into)
    }

    /// Types a message into a chat. With `dry_run` it stays in the composer.
    pub fn whatsapp_send(
        &self,
        chat: &str,
        text: &str,
        dry_run: bool,
    ) -> crate::Result<SendResult> {
        self.0
            .run_mobile_plugin(
                "whatsappSend",
                serde_json::json!({ "chat": chat, "text": text, "dryRun": dry_run }),
            )
            .map_err(Into::into)
    }

    /// Submits a phone number to WhatsApp Web's linking form and returns the
    /// pairing code. Pairing still requires approving the link inside WhatsApp
    /// on the phone; nothing can do that automatically.
    pub fn whatsapp_pair(&self, phone: &str) -> crate::Result<PairOutcome> {
        self.0
            .run_mobile_plugin("whatsappPair", serde_json::json!({ "phone": phone }))
            .map_err(Into::into)
    }

    /// Loads web.whatsapp.com in an overlay WebView and reports what booted.
    ///
    /// Blocks until the page finishes loading and the probe JS answers (tens of
    /// seconds on a cold load), so call it off the UI thread.
    pub fn whatsapp_web_probe(&self) -> crate::Result<WhatsAppProbe> {
        self.0
            .run_mobile_plugin("whatsappWebProbe", ())
            .map_err(Into::into)
    }

    /// Reports whether Shizuku is installed and whether its privileged server is
    /// running. Never requires Shizuku to be present.
    pub fn shizuku_status(&self) -> crate::Result<ShizukuStatus> {
        self.0
            .run_mobile_plugin("shizukuStatus", ())
            .map_err(Into::into)
    }
}
