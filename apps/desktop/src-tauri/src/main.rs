#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
};

use nexus_core::{
    initialize, rescan_directory_with_control, CoreError, RescanControl, RescanError,
    RescanProgress, RescanSummary, ScanOptions,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tracing::{error, info, warn};

const DEFAULT_SCAN_BATCH_SIZE: usize = 512;
const RESCAN_PROGRESS_EVENT: &str = "rescan-progress";
const RESCAN_FINISHED_EVENT: &str = "rescan-finished";

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

#[derive(Clone, Default)]
struct RescanManager {
    active: Arc<Mutex<Option<ActiveRescan>>>,
    next_id: Arc<AtomicU64>,
}

struct ActiveRescan {
    id: u64,
    control: RescanControl,
    progress: RescanProgress,
}

struct RescanTask {
    id: u64,
    control: RescanControl,
    database_path: PathBuf,
    root_path: PathBuf,
    options: ScanOptions,
    batch_size: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandError {
    code: &'static str,
    message: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartRescanRequest {
    root_path: String,
    #[serde(default)]
    ignored_paths: Vec<String>,
    #[serde(default)]
    follow_symlinks: bool,
    batch_size: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RescanIdRequest {
    scan_id: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartRescanResponse {
    scan_id: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CancelRescanResponse {
    scan_id: u64,
    accepted: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressPayload {
    processed: usize,
    files_succeeded: usize,
    files_failed: usize,
    paths_skipped: usize,
}

impl From<RescanProgress> for ProgressPayload {
    fn from(progress: RescanProgress) -> Self {
        Self {
            processed: progress.processed,
            files_succeeded: progress.files_succeeded,
            files_failed: progress.files_failed,
            paths_skipped: progress.paths_skipped,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct SummaryPayload {
    files_succeeded: usize,
    files_failed: usize,
    paths_skipped: usize,
    records_removed: usize,
    batches_committed: usize,
}

impl From<RescanSummary> for SummaryPayload {
    fn from(summary: RescanSummary) -> Self {
        Self {
            files_succeeded: summary.files_succeeded,
            files_failed: summary.files_failed,
            paths_skipped: summary.paths_skipped,
            records_removed: summary.records_removed,
            batches_committed: summary.batches_committed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RescanProgressEvent {
    scan_id: u64,
    #[serde(flatten)]
    progress: ProgressPayload,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RescanFinishedEvent {
    scan_id: u64,
    status: &'static str,
    message: String,
    summary: Option<SummaryPayload>,
    error_kind: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RescanStatusResponse {
    state: &'static str,
    scan_id: Option<u64>,
    progress: Option<ProgressPayload>,
}

impl RescanManager {
    fn reserve(&self, id: u64, control: RescanControl) -> Result<(), CommandError> {
        let mut active = self.active.lock().map_err(|_| CommandError {
            code: "rescan_state_unavailable",
            message: "无法启动手动重扫。",
        })?;
        if active.is_some() {
            return Err(CommandError {
                code: "rescan_already_running",
                message: "已有手动重扫正在进行。",
            });
        }

        *active = Some(ActiveRescan {
            id,
            control,
            progress: RescanProgress::default(),
        });
        Ok(())
    }

    fn update_progress(&self, id: u64, progress: RescanProgress) {
        let Ok(mut active) = self.active.lock() else {
            return;
        };

        if let Some(scan) = active.as_mut() {
            if scan.id == id {
                scan.progress = progress;
            }
        }
    }

    fn clear(&self, id: u64) {
        let Ok(mut active) = self.active.lock() else {
            return;
        };

        if active.as_ref().is_some_and(|scan| scan.id == id) {
            *active = None;
        }
    }
}

#[tauri::command]
fn get_rescan_status(
    state: State<'_, RescanManager>,
) -> Result<RescanStatusResponse, CommandError> {
    let active = state.active.lock().map_err(|_| CommandError {
        code: "rescan_state_unavailable",
        message: "无法读取手动重扫状态。",
    })?;

    Ok(match active.as_ref() {
        Some(scan) => RescanStatusResponse {
            state: "running",
            scan_id: Some(scan.id),
            progress: Some(scan.progress.into()),
        },
        None => RescanStatusResponse {
            state: "idle",
            scan_id: None,
            progress: None,
        },
    })
}

#[tauri::command]
fn start_rescan(
    app: AppHandle,
    state: State<'_, RescanManager>,
    request: StartRescanRequest,
) -> Result<StartRescanResponse, CommandError> {
    if request.root_path.trim().is_empty() {
        return Err(CommandError {
            code: "rescan_root_empty",
            message: "请先填写扫描目录。",
        });
    }

    let batch_size = request.batch_size.unwrap_or(DEFAULT_SCAN_BATCH_SIZE);
    if batch_size == 0 {
        return Err(CommandError {
            code: "rescan_batch_size_invalid",
            message: "扫描批次大小必须大于零。",
        });
    }

    let data_directory = app.path().app_data_dir().map_err(|_| CommandError {
        code: "app_data_directory",
        message: "无法定位本地数据目录。",
    })?;
    std::fs::create_dir_all(&data_directory).map_err(|_| CommandError {
        code: "app_data_directory_create",
        message: "无法准备本地数据目录。",
    })?;

    let control = RescanControl::new();
    let scan_id = state
        .next_id
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1);
    let manager = state.inner().clone();

    manager.reserve(scan_id, control.clone())?;

    let database_path = data_directory.join("nexus.sqlite3");
    let root_path = PathBuf::from(request.root_path);
    let options = ScanOptions {
        ignored_paths: request
            .ignored_paths
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        follow_symlinks: request.follow_symlinks,
    };

    let task = RescanTask {
        id: scan_id,
        control,
        database_path,
        root_path,
        options,
        batch_size,
    };
    let cleanup_manager = manager.clone();
    thread::Builder::new()
        .name(format!("nexus-rescan-{scan_id}"))
        .spawn(move || run_rescan(app, manager, task))
        .map_err(|_| {
            cleanup_manager.clear(scan_id);
            CommandError {
                code: "rescan_thread_start",
                message: "无法启动手动重扫。",
            }
        })?;

    Ok(StartRescanResponse { scan_id })
}

#[tauri::command]
fn cancel_rescan(
    state: State<'_, RescanManager>,
    request: RescanIdRequest,
) -> Result<CancelRescanResponse, CommandError> {
    let active = state.active.lock().map_err(|_| CommandError {
        code: "rescan_state_unavailable",
        message: "无法取消手动重扫。",
    })?;

    match active.as_ref() {
        Some(scan) if scan.id == request.scan_id => {
            scan.control.cancel();
            Ok(CancelRescanResponse {
                scan_id: request.scan_id,
                accepted: true,
            })
        }
        Some(_) => Err(CommandError {
            code: "rescan_not_active",
            message: "指定的手动重扫已不是当前任务。",
        }),
        None => Err(CommandError {
            code: "rescan_not_active",
            message: "当前没有正在进行的手动重扫。",
        }),
    }
}

fn run_rescan(app: AppHandle, manager: RescanManager, task: RescanTask) {
    let RescanTask {
        id: scan_id,
        control,
        database_path,
        root_path,
        options,
        batch_size,
    } = task;
    let progress_app = app.clone();
    let progress_manager = manager.clone();
    let result = rescan_directory_with_control(
        &database_path,
        &root_path,
        options,
        batch_size,
        control,
        move |progress| {
            progress_manager.update_progress(scan_id, progress);
            let _ = progress_app.emit(
                RESCAN_PROGRESS_EVENT,
                RescanProgressEvent {
                    scan_id,
                    progress: progress.into(),
                },
            );
        },
    );

    let event = match result {
        Ok(summary) => RescanFinishedEvent {
            scan_id,
            status: "completed",
            message: "手动重扫完成。".to_owned(),
            summary: Some(summary.into()),
            error_kind: None,
        },
        Err(error) => RescanFinishedEvent {
            scan_id,
            status: if matches!(&error, RescanError::Cancelled) {
                "cancelled"
            } else {
                "failed"
            },
            message: error.user_message().to_owned(),
            summary: None,
            error_kind: Some(error.kind()),
        },
    };

    manager.clear(scan_id);
    let _ = app.emit(RESCAN_FINISHED_EVENT, event);
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
            app.manage(RescanManager::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_startup_status,
            get_rescan_status,
            start_rescan,
            cancel_rescan
        ])
        .run(tauri::generate_context!());

    if result.is_err() {
        eprintln!("Nexus 桌面壳层启动失败。");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::{ActiveRescan, RescanManager};
    use nexus_core::{RescanControl, RescanProgress};

    #[test]
    fn rejects_a_second_running_rescan() {
        let manager = RescanManager::default();
        manager
            .reserve(1, RescanControl::new())
            .expect("首次重扫占位失败");

        let error = manager
            .reserve(2, RescanControl::new())
            .expect_err("重复启动重扫不应成功");

        assert_eq!(error.code, "rescan_already_running");
    }

    #[test]
    fn updates_and_clears_only_the_matching_rescan() {
        let manager = RescanManager::default();
        manager
            .reserve(7, RescanControl::new())
            .expect("创建重扫占位失败");
        let progress = RescanProgress {
            processed: 3,
            files_succeeded: 2,
            files_failed: 1,
            paths_skipped: 0,
        };

        manager.update_progress(8, progress);
        let active = manager.active.lock().expect("读取重扫状态失败");
        assert_eq!(active.as_ref().expect("缺少活动重扫").id, 7);
        assert_eq!(
            active.as_ref().expect("缺少活动重扫").progress,
            RescanProgress::default()
        );
        drop(active);

        manager.update_progress(7, progress);
        assert_eq!(
            manager
                .active
                .lock()
                .expect("读取更新后的重扫状态失败")
                .as_ref()
                .expect("缺少更新后的重扫")
                .progress,
            progress
        );

        manager.clear(8);
        assert!(manager.active.lock().expect("读取重扫状态失败").is_some());
        manager.clear(7);
        assert!(manager.active.lock().expect("读取重扫状态失败").is_none());
    }

    #[test]
    fn active_rescan_contains_a_shared_cancel_controller() {
        let manager = RescanManager::default();
        let control = RescanControl::new();
        manager
            .reserve(3, control.clone())
            .expect("创建取消控制测试重扫失败");

        control.cancel();
        let active = manager.active.lock().expect("读取取消控制测试状态失败");
        assert!(active
            .as_ref()
            .map(|scan: &ActiveRescan| scan.control.is_cancelled())
            .unwrap_or(false));
    }
}
