use tauri::{command, AppHandle, Runtime};

use crate::models::*;
use crate::InfinityAndroidExt;
use crate::Result;

#[command]
pub(crate) async fn count_contacts<R: Runtime>(app: AppHandle<R>) -> Result<ContactCount> {
    app.infinity_android().count_contacts()
}

#[command]
pub(crate) async fn shizuku_status<R: Runtime>(app: AppHandle<R>) -> Result<ShizukuStatus> {
    app.infinity_android().shizuku_status()
}

#[command]
pub(crate) async fn whatsapp_web_probe<R: Runtime>(app: AppHandle<R>) -> Result<WhatsAppProbe> {
    app.infinity_android().whatsapp_web_probe()
}

#[command]
pub(crate) async fn whatsapp_pair<R: Runtime>(
    app: AppHandle<R>,
    phone: String,
) -> Result<PairOutcome> {
    app.infinity_android().whatsapp_pair(&phone)
}

#[command]
pub(crate) async fn whatsapp_state<R: Runtime>(app: AppHandle<R>) -> Result<ChatState> {
    app.infinity_android().whatsapp_state()
}

#[command]
pub(crate) async fn whatsapp_read<R: Runtime>(
    app: AppHandle<R>,
    chat: String,
    limit: Option<u32>,
) -> Result<ReadResult> {
    app.infinity_android()
        .whatsapp_read(&chat, limit.unwrap_or(20))
}

#[command]
pub(crate) async fn whatsapp_send<R: Runtime>(
    app: AppHandle<R>,
    chat: String,
    text: String,
    dry_run: Option<bool>,
) -> Result<SendResult> {
    app.infinity_android()
        .whatsapp_send(&chat, &text, dry_run.unwrap_or(true))
}
