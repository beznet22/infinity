use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::models::*;

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> crate::Result<InfinityAndroid<R>> {
    Ok(InfinityAndroid)
}

/// Desktop stand-in: the capabilities behind this plugin are Android-only, so
/// every call reports why it cannot run instead of returning fake data.
pub struct InfinityAndroid;

impl InfinityAndroid {
    pub fn count_contacts(&self) -> crate::Result<ContactCount> {
        Err(crate::Error::Unsupported(
            "contact access is only available on Android".to_string(),
        ))
    }

    pub fn whatsapp_state(&self) -> crate::Result<ChatState> {
        Err(crate::Error::Unsupported(
            "the WhatsApp client only runs on Android".to_string(),
        ))
    }

    pub fn whatsapp_read(&self, _chat: &str, _limit: u32) -> crate::Result<ReadResult> {
        Err(crate::Error::Unsupported(
            "the WhatsApp client only runs on Android".to_string(),
        ))
    }

    pub fn whatsapp_send(
        &self,
        _chat: &str,
        _text: &str,
        _dry_run: bool,
    ) -> crate::Result<SendResult> {
        Err(crate::Error::Unsupported(
            "the WhatsApp client only runs on Android".to_string(),
        ))
    }

    pub fn whatsapp_pair(&self, _phone: &str) -> crate::Result<PairOutcome> {
        Err(crate::Error::Unsupported(
            "phone-number linking needs an Android WebView".to_string(),
        ))
    }

    pub fn whatsapp_web_probe(&self) -> crate::Result<WhatsAppProbe> {
        Err(crate::Error::Unsupported(
            "the WhatsApp Web probe needs an Android WebView".to_string(),
        ))
    }

    pub fn shizuku_status(&self) -> crate::Result<ShizukuStatus> {
        Err(crate::Error::Unsupported(
            "Shizuku is only meaningful on Android".to_string(),
        ))
    }
}
