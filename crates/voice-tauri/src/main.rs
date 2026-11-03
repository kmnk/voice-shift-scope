// Do not show a console window in Windows release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    voice_tauri_lib::run();
}
