const COMMANDS: &[&str] = &["count_contacts", "shizuku_status"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
