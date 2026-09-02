#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
};

use nexus_core::{
    index_directory_with_control, index_document_embeddings, initialize,
    rescan_directory_with_control, CoreError, EmbeddingIndexError, EmbeddingIndexOptions,
    EmbeddingProvider, IncrementalIndexOptions, LocalFeatureEmbedding, RescanControl, RescanError,
    RescanProgress, RescanSummary, ScanOptions,
};
use nexus_db::{
    extract_search_text, get_document, initialize_database,
    search_documents as run_document_search, search_documents_hybrid, DatabaseError,
    DocumentStoreError, HybridSearchResult, SearchError, SearchResult, DEFAULT_SEARCH_LIMIT,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
use tracing::{error, info, warn};

mod watch;

use watch::{WatchManager, WatchManagerError, WatchStatusResponse};

const DEFAULT_SCAN_BATCH_SIZE: usize = 512;
const DATABASE_FILE_NAME: &str = "nexus.sqlite3";
const WATCH_ROOT_FILE_NAME: &str = "nexus-watch-root.txt";
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
    index_content: bool,
    start_watch: bool,
    persist_watch_root: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandError {
    code: &'static str,
    message: &'static str,
}

fn app_database_path(app: &AppHandle) -> Result<PathBuf, CommandError> {
    let data_directory = app.path().app_data_dir().map_err(|_| CommandError {
        code: "app_data_directory",
        message: "无法定位本地数据目录。",
    })?;
    std::fs::create_dir_all(&data_directory).map_err(|_| CommandError {
        code: "app_data_directory_create",
        message: "无法准备本地数据目录。",
    })?;

    Ok(data_directory.join(DATABASE_FILE_NAME))
}

fn watch_root_file_path(app: &AppHandle) -> Result<PathBuf, CommandError> {
    Ok(app_database_path(app)?.with_file_name(WATCH_ROOT_FILE_NAME))
}

fn save_watch_root_file(path: &Path, root: &Path) -> Result<(), std::io::Error> {
    let root = root.to_string_lossy();
    std::fs::write(path, root.as_bytes())
}

fn load_watch_root_file(path: &Path) -> Result<Option<PathBuf>, std::io::Error> {
    match std::fs::read_to_string(path) {
        Ok(root) => {
            let root = root.trim();
            if root.is_empty() {
                Ok(None)
            } else {
                Ok(Some(PathBuf::from(root)))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn save_watch_root(app: &AppHandle, root: &Path) -> Result<(), CommandError> {
    let path = watch_root_file_path(app)?;
    save_watch_root_file(&path, root).map_err(|_| CommandError {
        code: "watch_root_persist",
        message: "无法保存自动同步目录。",
    })
}

fn load_watch_root(app: &AppHandle) -> Option<PathBuf> {
    let path = match watch_root_file_path(app) {
        Ok(path) => path,
        Err(_) => return None,
    };

    match load_watch_root_file(&path) {
        Ok(root) => root,
        Err(_) => {
            warn!(error_kind = "watch_root_read", "无法读取自动同步目录配置");
            None
        }
    }
}

fn database_command_error(error: DatabaseError) -> CommandError {
    let message = match &error {
        DatabaseError::InvalidSchemaVersion { .. }
        | DatabaseError::UnsupportedSchemaVersion { .. } => "本地数据存储版本不受支持。",
        DatabaseError::Open { .. }
        | DatabaseError::ReadSchemaVersion { .. }
        | DatabaseError::Migration { .. } => "本地数据存储暂时不可用。",
    };

    CommandError {
        code: error.kind(),
        message,
    }
}

fn search_command_error(error: SearchError) -> CommandError {
    CommandError {
        code: error.kind(),
        message: error.user_message(),
    }
}

fn embedding_command_error(error: nexus_core::EmbeddingError) -> CommandError {
    CommandError {
        code: error.kind(),
        message: error.user_message(),
    }
}

fn document_command_error(error: DocumentStoreError) -> CommandError {
    CommandError {
        code: error.kind(),
        message: error.user_message(),
    }
}

fn watch_command_error(error: WatchManagerError) -> CommandError {
    CommandError {
        code: error.kind(),
        message: error.user_message(),
    }
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
    #[serde(default)]
    index_content: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RescanIdRequest {
    scan_id: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchRequest {
    query: String,
    limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResultPayload {
    document_id: String,
    source_path: String,
    title: String,
    file_name: Option<String>,
    extension: Option<String>,
    file_type: Option<String>,
    modified_at: Option<i64>,
    created_at: Option<i64>,
    accessed_at: Option<i64>,
    line_start: Option<u64>,
    line_end: Option<u64>,
    relevance: Option<f64>,
    snippet: Option<String>,
    semantic_similarity: Option<f32>,
    fusion_score: Option<f64>,
    lexical_rank: Option<usize>,
    semantic_rank: Option<usize>,
}

impl From<SearchResult> for SearchResultPayload {
    fn from(result: SearchResult) -> Self {
        Self {
            document_id: result.document_id,
            source_path: result.source_path.to_string_lossy().into_owned(),
            title: result.title,
            file_name: result.file_name,
            extension: result.extension,
            file_type: result.file_type,
            modified_at: result.modified_at,
            created_at: result.created_at,
            accessed_at: result.accessed_at,
            line_start: result.line_start,
            line_end: result.line_end,
            relevance: result.relevance,
            snippet: result.snippet,
            semantic_similarity: None,
            fusion_score: None,
            lexical_rank: None,
            semantic_rank: None,
        }
    }
}

impl From<HybridSearchResult> for SearchResultPayload {
    fn from(hybrid: HybridSearchResult) -> Self {
        let payload = Self::from(hybrid.result);
        Self {
            semantic_similarity: hybrid.semantic_similarity,
            fusion_score: Some(hybrid.fusion_score),
            lexical_rank: hybrid.lexical_rank,
            semantic_rank: hybrid.semantic_rank,
            ..payload
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResponse {
    results: Vec<SearchResultPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenDocumentRequest {
    document_id: String,
}

fn open_local_document(path: &Path) -> Result<(), CommandError> {
    if !path.is_file() {
        return Err(CommandError {
            code: "document_missing",
            message: "原始文件已不存在或不可访问。",
        });
    }

    spawn_open_command(path).map_err(|_| CommandError {
        code: "document_open",
        message: "无法定位原始文件。",
    })
}

#[cfg(target_os = "windows")]
fn spawn_open_command(path: &Path) -> Result<(), std::io::Error> {
    let mut selection = OsString::from("/select,");
    selection.push(path);
    ProcessCommand::new("explorer.exe")
        .arg(selection)
        .spawn()
        .map(|_| ())
}

#[cfg(target_os = "macos")]
fn spawn_open_command(path: &Path) -> Result<(), std::io::Error> {
    ProcessCommand::new("open")
        .arg("-R")
        .arg(path)
        .spawn()
        .map(|_| ())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn spawn_open_command(path: &Path) -> Result<(), std::io::Error> {
    ProcessCommand::new("xdg-open")
        .arg(path)
        .spawn()
        .map(|_| ())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
fn spawn_open_command(_path: &Path) -> Result<(), std::io::Error> {
    Err(std::io::Error::other("unsupported platform"))
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
    documents_succeeded: usize,
    documents_failed: usize,
    documents_skipped: usize,
}

impl From<RescanProgress> for ProgressPayload {
    fn from(progress: RescanProgress) -> Self {
        Self {
            processed: progress.processed,
            files_succeeded: progress.files_succeeded,
            files_failed: progress.files_failed,
            paths_skipped: progress.paths_skipped,
            documents_succeeded: progress.documents_succeeded,
            documents_failed: progress.documents_failed,
            documents_skipped: progress.documents_skipped,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct SummaryPayload {
    files_succeeded: usize,
    files_failed: usize,
    paths_skipped: usize,
    documents_succeeded: usize,
    documents_failed: usize,
    documents_skipped: usize,
    records_removed: usize,
    batches_committed: usize,
}

impl From<RescanSummary> for SummaryPayload {
    fn from(summary: RescanSummary) -> Self {
        Self {
            files_succeeded: summary.files_succeeded,
            files_failed: summary.files_failed,
            paths_skipped: summary.paths_skipped,
            documents_succeeded: summary.documents_succeeded,
            documents_failed: summary.documents_failed,
            documents_skipped: summary.documents_skipped,
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
    fn next_scan_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed).wrapping_add(1)
    }

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

    fn cancel(&self) {
        if let Ok(active) = self.active.lock() {
            if let Some(scan) = active.as_ref() {
                scan.control.cancel();
            }
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
fn get_watch_status(state: State<'_, WatchManager>) -> Result<WatchStatusResponse, CommandError> {
    state.status().map_err(watch_command_error)
}

#[tauri::command]
fn search_documents(
    app: AppHandle,
    request: SearchRequest,
) -> Result<SearchResponse, CommandError> {
    if request.query.trim().is_empty() {
        return Err(CommandError {
            code: "search_query_empty",
            message: "搜索条件不能为空。",
        });
    }

    let database_path = app_database_path(&app)?;
    let connection = initialize_database(database_path).map_err(database_command_error)?;
    let limit = request.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
    let results = match extract_search_text(&request.query).map_err(search_command_error)? {
        Some(text) => {
            let provider = LocalFeatureEmbedding::new();
            let query_vector = provider.embed(&text).map_err(embedding_command_error)?;
            search_documents_hybrid(
                &connection,
                &request.query,
                query_vector.as_slice(),
                provider.model_id(),
                provider.model_version(),
                limit,
            )
            .map_err(search_command_error)?
            .into_iter()
            .map(SearchResultPayload::from)
            .collect()
        }
        None => run_document_search(&connection, &request.query, limit)
            .map_err(search_command_error)?
            .into_iter()
            .map(SearchResultPayload::from)
            .collect(),
    };

    Ok(SearchResponse { results })
}

#[tauri::command]
fn open_document(app: AppHandle, request: OpenDocumentRequest) -> Result<(), CommandError> {
    if request.document_id.trim().is_empty() {
        return Err(CommandError {
            code: "document_id_invalid",
            message: "文档标识不能为空。",
        });
    }

    let database_path = app_database_path(&app)?;
    let connection = initialize_database(database_path).map_err(database_command_error)?;
    let document = get_document(&connection, request.document_id.trim())
        .map_err(document_command_error)?
        .ok_or(CommandError {
            code: "document_not_found",
            message: "原始文档记录不存在。",
        })?;

    open_local_document(&document.source_path)
}

#[tauri::command]
fn start_rescan(
    app: AppHandle,
    state: State<'_, RescanManager>,
    watch_state: State<'_, WatchManager>,
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

    watch_state.stop();
    let database_path = app_database_path(&app)?;

    let control = RescanControl::new();
    let scan_id = state.next_scan_id();
    let manager = state.inner().clone();
    let watch_manager = watch_state.inner().clone();

    manager.reserve(scan_id, control.clone())?;

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
        index_content: request.index_content,
        start_watch: request.index_content,
        persist_watch_root: request.index_content,
    };
    let cleanup_manager = manager.clone();
    thread::Builder::new()
        .name(format!("nexus-rescan-{scan_id}"))
        .spawn(move || run_rescan(app, manager, watch_manager, task))
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

fn run_rescan(
    app: AppHandle,
    manager: RescanManager,
    watch_manager: WatchManager,
    task: RescanTask,
) {
    let RescanTask {
        id: scan_id,
        control,
        database_path,
        root_path,
        options,
        batch_size,
        index_content,
        start_watch,
        persist_watch_root,
    } = task;
    let embedding_control = control.clone();
    let result = if index_content {
        let progress_app = app.clone();
        let progress_manager = manager.clone();
        index_directory_with_control(
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
        )
    } else {
        let progress_app = app.clone();
        let progress_manager = manager.clone();
        rescan_directory_with_control(
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
        )
    };

    let event = match result {
        Ok(summary) => {
            if persist_watch_root {
                if let Err(error) = save_watch_root(&app, &root_path) {
                    warn!(error_kind = error.code, "无法保存自动同步目录");
                }
            }

            if index_content {
                let provider = LocalFeatureEmbedding::new();
                if let Err(error) = index_document_embeddings(
                    &database_path,
                    &provider,
                    EmbeddingIndexOptions::default(),
                    &embedding_control,
                ) {
                    log_embedding_error(&error);
                }
            }

            if start_watch {
                if let Err(error) = watch_manager.start(
                    app.clone(),
                    database_path.clone(),
                    root_path.clone(),
                    IncrementalIndexOptions::default(),
                ) {
                    warn!(error_kind = error.kind(), "无法启动文件自动同步");
                }
            }

            RescanFinishedEvent {
                scan_id,
                status: "completed",
                message: "手动重扫完成。".to_owned(),
                summary: Some(summary.into()),
                error_kind: None,
            }
        }
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

fn log_embedding_error(error: &EmbeddingIndexError) {
    warn!(error_kind = error.kind(), "初始 embedding 索引未能完成");
}

fn start_startup_recovery(app: &AppHandle, manager: &RescanManager, watch_manager: &WatchManager) {
    let Some(root_path) = load_watch_root(app) else {
        return;
    };
    let database_path = match app_database_path(app) {
        Ok(path) => path,
        Err(error) => {
            warn!(error_kind = error.code, "无法准备启动恢复任务");
            return;
        }
    };
    let control = RescanControl::new();
    let scan_id = manager.next_scan_id();
    if let Err(error) = manager.reserve(scan_id, control.clone()) {
        warn!(error_kind = error.code, "无法占用启动恢复任务");
        return;
    }

    let task = RescanTask {
        id: scan_id,
        control,
        database_path,
        root_path,
        options: ScanOptions::default(),
        batch_size: DEFAULT_SCAN_BATCH_SIZE,
        index_content: true,
        start_watch: true,
        persist_watch_root: false,
    };
    let manager = manager.clone();
    let watch_manager = watch_manager.clone();
    let cleanup_manager = manager.clone();
    let app = app.clone();
    if thread::Builder::new()
        .name(format!("nexus-recovery-{scan_id}"))
        .spawn(move || run_rescan(app, manager, watch_manager, task))
        .is_err()
    {
        cleanup_manager.clear(scan_id);
        warn!(error_kind = "rescan_thread_start", "无法启动启动恢复任务");
    }
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

    let database_path = data_directory.join(DATABASE_FILE_NAME);

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
            let should_recover = matches!(&status.phase, StartupPhase::Ready);
            let rescan_manager = RescanManager::default();
            let watch_manager = WatchManager::default();
            app.manage(status);
            app.manage(rescan_manager.clone());
            app.manage(watch_manager.clone());
            if should_recover {
                start_startup_recovery(app.handle(), &rescan_manager, &watch_manager);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_startup_status,
            get_rescan_status,
            get_watch_status,
            search_documents,
            open_document,
            start_rescan,
            cancel_rescan
        ])
        .build(tauri::generate_context!())
        .map(|app| {
            app.run(|app_handle, event| {
                if matches!(event, RunEvent::ExitRequested { .. } | RunEvent::Exit) {
                    if let Some(rescan_manager) = app_handle.try_state::<RescanManager>() {
                        rescan_manager.cancel();
                    }
                    if let Some(watch_manager) = app_handle.try_state::<WatchManager>() {
                        watch_manager.stop();
                    }
                }
            });
        });

    if result.is_err() {
        eprintln!("Nexus 桌面壳层启动失败。");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::PathBuf,
        process,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::{
        database_command_error, load_watch_root_file, save_watch_root_file, ActiveRescan,
        RescanManager, SearchResultPayload,
    };
    use nexus_core::{RescanControl, RescanProgress};
    use nexus_db::{DatabaseError, SearchResult};

    static WATCH_ROOT_TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn watch_root_test_path() -> PathBuf {
        let sequence = WATCH_ROOT_TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        env::temp_dir().join(format!(
            "nexus-watch-root-test-{}-{}.txt",
            process::id(),
            sequence
        ))
    }

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
            documents_succeeded: 1,
            documents_failed: 1,
            documents_skipped: 0,
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

    #[test]
    fn maps_database_version_errors_to_safe_command_errors() {
        let error = database_command_error(DatabaseError::UnsupportedSchemaVersion {
            path: PathBuf::from("private.sqlite3"),
            found: 99,
            supported: 4,
        });

        assert_eq!(error.code, "database_schema_unsupported");
        assert_eq!(error.message, "本地数据存储版本不受支持。");
        assert!(!error.message.contains("private.sqlite3"));
    }

    #[test]
    fn maps_search_results_without_copying_full_document_body() {
        let result = SearchResult {
            document_id: "document-1".to_owned(),
            source_path: PathBuf::from("C:\\Nexus\\notes.md"),
            title: "项目计划".to_owned(),
            file_name: Some("notes.md".to_owned()),
            extension: Some("md".to_owned()),
            file_type: Some("text/markdown".to_owned()),
            modified_at: Some(1_700_000_000_000),
            created_at: None,
            accessed_at: None,
            line_start: None,
            line_end: None,
            relevance: Some(1.5),
            snippet: Some("⟦项目⟧ 计划".to_owned()),
        };

        let payload = SearchResultPayload::from(result);
        assert_eq!(payload.document_id, "document-1");
        assert_eq!(payload.source_path, "C:\\Nexus\\notes.md");
        assert_eq!(payload.snippet.as_deref(), Some("⟦项目⟧ 计划"));
    }

    #[test]
    fn persists_and_restores_watch_root_for_startup_recovery() {
        let config_path = watch_root_test_path();
        let root = PathBuf::from("C:\\Nexus\\资料");

        save_watch_root_file(&config_path, &root).expect("保存监听目录配置失败");
        assert_eq!(
            load_watch_root_file(&config_path).expect("读取监听目录配置失败"),
            Some(root)
        );

        fs::remove_file(config_path).expect("清理监听目录配置测试文件失败");
    }

    #[test]
    fn treats_missing_or_blank_watch_root_as_no_startup_recovery() {
        let config_path = watch_root_test_path();
        assert_eq!(
            load_watch_root_file(&config_path).expect("读取缺失监听配置失败"),
            None
        );

        fs::write(&config_path, b" \r\n").expect("写入空监听目录配置失败");
        assert_eq!(
            load_watch_root_file(&config_path).expect("读取空监听配置失败"),
            None
        );

        fs::remove_file(config_path).expect("清理空监听目录配置测试文件失败");
    }
}
