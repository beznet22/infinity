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
