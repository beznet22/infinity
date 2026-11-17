use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

pub use models::*;

#[cfg(desktop)]
mod desktop;
#[cfg(mobile)]
mod mobile;

mod commands;
mod error;
mod models;

pub use error::{Error, Result};

#[cfg(desktop)]
use desktop::InfinityAndroid;
#[cfg(mobile)]
use mobile::InfinityAndroid;

/// Extensions to [`tauri::App`], [`tauri::AppHandle`] and [`tauri::Window`] to
/// access the infinity-android APIs.
pub trait InfinityAndroidExt<R: Runtime> {
    fn infinity_android(&self) -> &InfinityAndroid<R>;
}

impl<R: Runtime, T: Manager<R>> crate::InfinityAndroidExt<R> for T {
    fn infinity_android(&self) -> &InfinityAndroid<R> {
        self.state::<InfinityAndroid<R>>().inner()
    }
}

/// Initializes the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("infinity-android")
        .invoke_handler(tauri::generate_handler![
            commands::count_contacts,
            commands::shizuku_status,
            commands::whatsapp_web_probe,
            commands::whatsapp_pair,
            commands::whatsapp_state,
            commands::whatsapp_read,
            commands::whatsapp_send
        ])
        .setup(|app, api| {
            #[cfg(mobile)]
            let infinity_android = mobile::init(app, api)?;
            #[cfg(desktop)]
            let infinity_android = desktop::init(app, api)?;
            app.manage(infinity_android);
            Ok(())
        })
        .build()
}
