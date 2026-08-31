#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use nexus_core::{initialize, CoreError};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tracing::{error, info, warn};

#[derive(Clone, Serialize)]
#[serde(rename_all = "lowercase")]
enum StartupPhase {
    Ready,
    Degraded,
}

#[derive(Clone, Serialize)]
struct StartupStatus {
    phase: StartupPhase,
    message: String,
}

impl StartupStatus {
    fn ready() -> Self {
        Self {
            phase: StartupPhase::Ready,
            message: "本地核心已就绪。".to_owned(),
        }
    }

    fn degraded(message: &'static str) -> Self {
        Self {
            phase: StartupPhase::Degraded,
            message: message.to_owned(),
        }
    }
}

#[tauri::command]
fn get_startup_status(state: State<'_, StartupStatus>) -> StartupStatus {
    state.inner().clone()
}

fn initialize_logging() {
    if let Err(error) = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init()
    {
        eprintln!("Nexus 日志初始化失败: {error}");
    }
}

fn initialize_startup(app: &AppHandle) -> StartupStatus {
    let data_directory = match app.path().app_data_dir() {
        Ok(directory) => directory,
        Err(_) => {
            error!(error_kind = "app_data_directory", "无法定位本地数据目录");
            return StartupStatus::degraded("无法定位本地数据目录，当前处于降级模式。");
        }
    };

    if std::fs::create_dir_all(&data_directory).is_err() {
        error!(
            error_kind = "app_data_directory_create",
            "无法准备本地数据目录"
        );
        return StartupStatus::degraded("无法准备本地数据目录，当前处于降级模式。");
    }

    let database_path = data_directory.join("nexus.sqlite3");

    match initialize(database_path) {
        Ok(()) => {
            info!("Nexus 本地核心已就绪");
            StartupStatus::ready()
        }
        Err(error) => {
            log_core_error(&error);
            StartupStatus::degraded(error.user_message())
        }
    }
}

fn log_core_error(error: &CoreError) {
    match error.kind() {
        "database_schema_unsupported" | "database_schema_invalid" => {
            warn!(error_kind = error.kind(), "本地数据库版本不受支持")
        }
        _ => error!(error_kind = error.kind(), "本地核心初始化失败"),
    }
}

fn main() {
    initialize_logging();

    let result = tauri::Builder::default()
        .setup(|app| {
            let status = initialize_startup(app.handle());
            app.manage(status);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_startup_status])
        .run(tauri::generate_context!());

    if result.is_err() {
        eprintln!("Nexus 桌面壳层启动失败。");
        std::process::exit(1);
    }
}
