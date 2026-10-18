// Windows の release の build で、黒いコンソールの窓を出さない
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    voice_tauri_lib::run();
}
