const COMMANDS: &[&str] = &["count_contacts", "shizuku_status", "whatsapp_web_probe"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
