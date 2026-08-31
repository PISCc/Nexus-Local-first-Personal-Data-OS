#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// M0.3 先固定依赖方向；启动初始化行为留到 M0.5。
use nexus_core as _;

fn main() {
    if let Err(error) = tauri::Builder::default().run(tauri::generate_context!()) {
        eprintln!("failed to run the Nexus desktop shell: {error}");
        std::process::exit(1);
    }
}
