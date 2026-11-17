const COMMANDS: &[&str] = &[
    "count_contacts",
    "shizuku_status",
    "whatsapp_web_probe",
    "whatsapp_pair",
    "whatsapp_state",
    "whatsapp_read",
    "whatsapp_send",
    "start_foreground_service",
    "stop_foreground_service",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
