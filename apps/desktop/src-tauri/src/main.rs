// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;

// --- IPC Commands ---

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! UnderScreen is running.", name)
}

#[tauri::command]
fn get_app_status() -> serde_json::Value {
    serde_json::json!({
        "status": "running",
        "version": "0.1.0",
        "services": {
            "audio": false,
            "asr": false,
            "llm": false
        }
    })
}

// --- Main Entry Point ---

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            greet,
            get_app_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}