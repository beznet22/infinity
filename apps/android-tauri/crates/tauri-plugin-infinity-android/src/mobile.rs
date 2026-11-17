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

    /// Reports whether Shizuku is installed and whether its privileged server is
    /// running. Never requires Shizuku to be present.
    pub fn shizuku_status(&self) -> crate::Result<ShizukuStatus> {
        self.0
            .run_mobile_plugin("shizukuStatus", ())
            .map_err(Into::into)
    }
}
