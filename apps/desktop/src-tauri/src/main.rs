#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
};

#[cfg(not(target_os = "windows"))]
use std::process::Command as ProcessCommand;

use nexus_core::{
    index_directory_with_control, index_document_embeddings, initialize,
    rescan_directory_with_control, CoreError, EmbeddingIndexError, EmbeddingIndexOptions,
    EmbeddingProvider, IncrementalIndexOptions, LocalFeatureEmbedding, RescanControl, RescanError,
    RescanProgress, RescanSummary, ScanOptions,
};
use nexus_db::{
    extract_search_text, get_document, get_index_statistics, initialize_database,
    search_documents as run_document_search, search_documents_hybrid, DatabaseError,
    DocumentStoreError, HybridSearchResult, IndexStatistics, IndexStatisticsError, SearchError,
    SearchResult, DEFAULT_SEARCH_LIMIT,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
use tracing::{error, info, warn};

mod watch;

use watch::{WatchManager, WatchManagerError, WatchRestartConfig, WatchStatusResponse};

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
    last_finished: Arc<Mutex<Option<RescanFinishedEvent>>>,
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
    restart_watch: Option<WatchRestartConfig>,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceConfigResponse {
    root_path: Option<String>,
}

fn read_source_root(app: &AppHandle) -> Result<Option<PathBuf>, CommandError> {
    let path = watch_root_file_path(app)?;
    load_watch_root_file(&path).map_err(|_| CommandError {
        code: "source_config_read",
        message: "无法读取已保存的来源配置。",
    })
}

#[tauri::command]
fn get_source_config(app: AppHandle) -> Result<SourceConfigResponse, CommandError> {
    let root_path = read_source_root(&app)?;

    Ok(SourceConfigResponse {
        root_path: root_path.map(|path| path.to_string_lossy().into_owned()),
    })
}

fn validate_rescan_root(root_path: &Path, follow_symlinks: bool) -> Result<(), CommandError> {
    let link_metadata = std::fs::symlink_metadata(root_path).map_err(|_| CommandError {
        code: "rescan_root_unavailable",
        message: "扫描目录不存在或无法访问。",
    })?;

    if link_metadata.file_type().is_symlink() && !follow_symlinks {
        return Err(CommandError {
            code: "rescan_root_symlink",
            message: "扫描根目录是符号链接；请明确开启跟随符号链接。",
        });
    }

    let metadata = std::fs::metadata(root_path).map_err(|_| CommandError {
        code: "rescan_root_unavailable",
        message: "扫描目录不存在或无法访问。",
    })?;
    if !metadata.is_dir() {
        return Err(CommandError {
            code: "rescan_root_not_directory",
            message: "扫描路径必须是目录。",
        });
    }

    Ok(())
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

fn index_statistics_command_error(error: IndexStatisticsError) -> CommandError {
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
fn windows_shell_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(target_os = "windows")]
fn spawn_open_command(path: &Path) -> Result<(), std::io::Error> {
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::{
        Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK},
        System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED},
        UI::Shell::{Common::ITEMIDLIST, SHOpenFolderAndSelectItems, SHParseDisplayName},
    };

    let wide_path = windows_shell_path(path);
    let mut item_id_list = null_mut::<ITEMIDLIST>();
    let initialize_result = unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
    let should_uninitialize = matches!(initialize_result, S_OK | S_FALSE);

    if initialize_result != S_OK
        && initialize_result != S_FALSE
        && initialize_result != RPC_E_CHANGED_MODE
    {
        return Err(std::io::Error::other("无法初始化 Windows Shell。"));
    }

    let parse_result = unsafe {
        SHParseDisplayName(
            wide_path.as_ptr(),
            null_mut(),
            &mut item_id_list,
            0,
            null_mut(),
        )
    };
    let selection_result = if parse_result == S_OK {
        // cidl=0 表示 item_id_list 是待选中的完整项目 ID，Shell 会打开父目录并选中该文件。
        unsafe { SHOpenFolderAndSelectItems(item_id_list, 0, null(), 0) }
    } else {
        parse_result
    };

    if !item_id_list.is_null() {
        unsafe { CoTaskMemFree(item_id_list.cast()) };
    }
    if should_uninitialize {
        unsafe { CoUninitialize() };
    }

    if selection_result == S_OK {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "Windows Shell 定位失败（HRESULT 0x{:08X}）。",
            selection_result as u32
        )))
    }
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

    fn status(&self) -> Result<RescanStatusResponse, CommandError> {
        let active = self.active.lock().map_err(|_| CommandError {
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

    fn finish(&self, event: RescanFinishedEvent) {
        let Ok(mut active) = self.active.lock() else {
            return;
        };
        if !active.as_ref().is_some_and(|scan| scan.id == event.scan_id) {
            return;
        }

        if let Ok(mut last_finished) = self.last_finished.lock() {
            *last_finished = Some(event);
        }
        *active = None;
    }

    fn last_finished(&self) -> Result<Option<RescanFinishedEvent>, CommandError> {
        self.last_finished
            .lock()
            .map(|event| event.clone())
            .map_err(|_| CommandError {
                code: "rescan_state_unavailable",
                message: "无法读取最近一次索引结果。",
            })
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
    state.status()
}

#[tauri::command]
fn get_watch_status(state: State<'_, WatchManager>) -> Result<WatchStatusResponse, CommandError> {
    state.status().map_err(watch_command_error)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IndexHealthDecision {
    state: &'static str,
    message: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct IndexHealthResponse {
    state: &'static str,
    message: &'static str,
    root_path: Option<String>,
    files_indexed: u64,
    documents_indexed: u64,
    watch_state: &'static str,
    scan_id: Option<u64>,
    progress: Option<ProgressPayload>,
}

fn derive_index_health(
    source_configured: bool,
    rescan_running: bool,
    last_finished: Option<&RescanFinishedEvent>,
    watch_running: bool,
) -> IndexHealthDecision {
    if rescan_running {
        return IndexHealthDecision {
            state: "indexing",
            message: "正在建立或刷新本地索引，已提交的内容仍可搜索。",
        };
    }

    if let Some(last_finished) = last_finished {
        match last_finished.status {
            "failed" => {
                return IndexHealthDecision {
                    state: "failed",
                    message: "最近一次索引未完成；已有索引仍保留，请在下方重试。",
                };
            }
            "cancelled" => {
                return IndexHealthDecision {
                    state: "cancelled",
                    message: "最近一次索引已取消；已有索引未被破坏，可随时重新开始。",
                };
            }
            _ => {}
        }
    }

    if !source_configured {
        return IndexHealthDecision {
            state: "not-configured",
            message: "还没有配置本地来源；完成一次索引后即可搜索并自动同步。",
        };
    }

    if let Some(last_finished) = last_finished {
        let scan_incomplete = match last_finished.summary.as_ref() {
            Some(summary) => summary.files_failed > 0 || summary.documents_failed > 0,
            None => last_finished.status == "completed",
        };
        if scan_incomplete || last_finished.error_kind.is_some() {
            return IndexHealthDecision {
                state: "degraded",
                message: "索引可以使用，但最近一次更新存在未处理内容，请检查任务结果。",
            };
        }
    }

    if !watch_running {
        return IndexHealthDecision {
            state: "degraded",
            message: "现有索引可以搜索，但文件自动同步未运行，请重新索引以恢复。",
        };
    }

    IndexHealthDecision {
        state: "ready",
        message: "本地索引可用，文件变化会自动同步。",
    }
}

fn read_index_statistics(database_path: PathBuf) -> Result<IndexStatistics, CommandError> {
    let connection = initialize_database(database_path).map_err(database_command_error)?;
    get_index_statistics(&connection).map_err(index_statistics_command_error)
}

#[tauri::command]
async fn get_index_health(
    app: AppHandle,
    rescan_state: State<'_, RescanManager>,
    watch_state: State<'_, WatchManager>,
) -> Result<IndexHealthResponse, CommandError> {
    let source_root = read_source_root(&app)?;
    let rescan_status = rescan_state.status()?;
    let last_finished = rescan_state.last_finished()?;
    let watch_status = watch_state.status().map_err(watch_command_error)?;
    let database_path = app_database_path(&app)?;
    let statistics =
        tauri::async_runtime::spawn_blocking(move || read_index_statistics(database_path))
            .await
            .map_err(|_| CommandError {
                code: "index_health_thread",
                message: "无法读取本地索引健康状态。",
            })??;
    let decision = derive_index_health(
        source_root.is_some(),
        rescan_status.state == "running",
        last_finished.as_ref(),
        watch_status.state == "running",
    );

    Ok(IndexHealthResponse {
        state: decision.state,
        message: decision.message,
        root_path: source_root.map(|path| path.to_string_lossy().into_owned()),
        files_indexed: statistics.files_indexed,
        documents_indexed: statistics.documents_indexed,
        watch_state: watch_status.state,
        scan_id: rescan_status.scan_id,
        progress: rescan_status.progress,
    })
}

#[tauri::command]
async fn search_documents(
    app: AppHandle,
    request: SearchRequest,
) -> Result<SearchResponse, CommandError> {
    let database_path = app_database_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        search_documents_from_database(database_path, request)
    })
    .await
    .map_err(|_| CommandError {
        code: "search_thread",
        message: "本地搜索暂时不可用。",
    })?
}

fn search_documents_from_database(
    database_path: PathBuf,
    request: SearchRequest,
) -> Result<SearchResponse, CommandError> {
    if request.query.trim().is_empty() {
        return Err(CommandError {
            code: "search_query_empty",
            message: "搜索条件不能为空。",
        });
    }

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

    let root_path = PathBuf::from(request.root_path.trim());
    validate_rescan_root(&root_path, request.follow_symlinks)?;

    let database_path = app_database_path(&app)?;

    let control = RescanControl::new();
    let scan_id = state.next_scan_id();
    let manager = state.inner().clone();
    let watch_manager = watch_state.inner().clone();

    manager.reserve(scan_id, control.clone())?;

    let options = ScanOptions {
        ignored_paths: request
            .ignored_paths
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        follow_symlinks: request.follow_symlinks,
    };

    let restart_watch = match watch_state.pause() {
        Ok(config) => config,
        Err(error) => {
            manager.clear(scan_id);
            return Err(watch_command_error(error));
        }
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
        restart_watch,
    };
    let cleanup_manager = manager.clone();
    let restart_watch_on_spawn_failure = task.restart_watch.clone();
    let restart_manager = watch_manager.clone();
    let restart_app = app.clone();
    thread::Builder::new()
        .name(format!("nexus-rescan-{scan_id}"))
        .spawn(move || run_rescan(app, manager, watch_manager, task))
        .map_err(|_| {
            cleanup_manager.clear(scan_id);
            resume_watch_if_possible(
                &restart_manager,
                &restart_app,
                restart_watch_on_spawn_failure,
                false,
            );
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
        mut restart_watch,
    } = task;
    let embedding_control = control.clone();
    let watch_scan_options = options.clone();
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
            let mut embedding_error_kind = None;
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
                    embedding_error_kind = Some(error.kind());
                    log_embedding_error(&error);
                }
            }

            let watch_options = watch_options_for_scan(&watch_scan_options);
            if start_watch {
                let can_resume = restart_watch.as_ref().is_some_and(|config| {
                    config.matches(&database_path, &root_path, &watch_options)
                });
                if can_resume {
                    resume_watch_if_possible(&watch_manager, &app, restart_watch.take(), true);
                } else {
                    if restart_watch.is_some() {
                        let _ = watch_manager.stop();
                    }
                    if let Err(error) = watch_manager.start_after_rescan(
                        app.clone(),
                        database_path.clone(),
                        root_path.clone(),
                        watch_options,
                    ) {
                        warn!(error_kind = error.kind(), "无法启动文件自动同步");
                        if let Some(config) = restart_watch.take() {
                            restart_watch_if_possible(&watch_manager, &app, config);
                        }
                    }
                }
            } else if let Some(config) = restart_watch.take() {
                resume_watch_if_possible(&watch_manager, &app, Some(config), false);
            }

            RescanFinishedEvent {
                scan_id,
                status: "completed",
                message: if embedding_error_kind.is_some() {
                    "手动重扫完成，但语义索引未完全更新。".to_owned()
                } else {
                    "手动重扫完成。".to_owned()
                },
                summary: Some(summary.into()),
                error_kind: embedding_error_kind,
            }
        }
        Err(error) => {
            if let Some(config) = restart_watch.take() {
                resume_watch_if_possible(&watch_manager, &app, Some(config), false);
            }

            RescanFinishedEvent {
                scan_id,
                status: if matches!(&error, RescanError::Cancelled) {
                    "cancelled"
                } else {
                    "failed"
                },
                message: error.user_message().to_owned(),
                summary: None,
                error_kind: Some(error.kind()),
            }
        }
    };

    manager.finish(event.clone());
    let _ = app.emit(RESCAN_FINISHED_EVENT, event);
}

fn restart_watch_if_possible(
    watch_manager: &WatchManager,
    app: &AppHandle,
    config: WatchRestartConfig,
) {
    start_watch_if_possible(watch_manager, app, config, false);
}

fn watch_options_for_scan(scan_options: &ScanOptions) -> IncrementalIndexOptions {
    IncrementalIndexOptions {
        scan_options: scan_options.clone(),
        ..IncrementalIndexOptions::default()
    }
}

fn restart_watch_after_rescan_if_possible(
    watch_manager: &WatchManager,
    app: &AppHandle,
    config: WatchRestartConfig,
) {
    start_watch_if_possible(watch_manager, app, config, true);
}

fn start_watch_if_possible(
    watch_manager: &WatchManager,
    app: &AppHandle,
    config: WatchRestartConfig,
    after_rescan: bool,
) {
    let result = if after_rescan {
        watch_manager.start_after_rescan(
            app.clone(),
            config.database_path,
            config.root_path,
            config.options,
        )
    } else {
        watch_manager.start(
            app.clone(),
            config.database_path,
            config.root_path,
            config.options,
        )
    };
    if let Err(error) = result {
        warn!(error_kind = error.kind(), "无法恢复文件自动同步");
    }
}

fn resume_watch_if_possible(
    watch_manager: &WatchManager,
    app: &AppHandle,
    fallback: Option<WatchRestartConfig>,
    after_rescan: bool,
) {
    match watch_manager.resume() {
        Ok(true) => {}
        Ok(false) => {
            if let Some(config) = fallback {
                if after_rescan {
                    restart_watch_after_rescan_if_possible(watch_manager, app, config);
                } else {
                    restart_watch_if_possible(watch_manager, app, config);
                }
            }
        }
        Err(error) => {
            warn!(error_kind = error.kind(), "无法恢复文件自动同步");
            if let Some(config) = fallback {
                if after_rescan {
                    restart_watch_after_rescan_if_possible(watch_manager, app, config);
                } else {
                    restart_watch_if_possible(watch_manager, app, config);
                }
            }
        }
    }
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
        restart_watch: None,
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
            get_source_config,
            get_index_health,
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
        database_command_error, derive_index_health, load_watch_root_file, save_watch_root_file,
        search_documents_from_database, validate_rescan_root, watch_options_for_scan, ActiveRescan,
        RescanFinishedEvent, RescanManager, SearchRequest, SearchResultPayload, SummaryPayload,
    };
    use nexus_core::{RescanControl, RescanProgress, ScanOptions};
    use nexus_db::{
        initialize_database, upsert_document, DatabaseError, DocumentRecord, SearchResult,
    };

    static WATCH_ROOT_TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);
    static SEARCH_TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[cfg(target_os = "windows")]
    #[test]
    fn preserves_unicode_when_encoding_windows_shell_path() {
        let path = PathBuf::from(r"C:\有机记忆场\原文件.md");
        let wide_path = super::windows_shell_path(&path);

        assert_eq!(wide_path.last(), Some(&0));
        assert_eq!(
            String::from_utf16(&wide_path[..wide_path.len() - 1])
                .expect("Windows Shell 路径应可还原"),
            path.to_string_lossy().as_ref(),
        );
    }

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
    fn retains_the_last_finished_rescan_for_health_reporting() {
        let manager = RescanManager::default();
        manager
            .reserve(9, RescanControl::new())
            .expect("创建完成结果测试重扫失败");
        manager.finish(RescanFinishedEvent {
            scan_id: 9,
            status: "completed",
            message: "手动重扫完成。".to_owned(),
            summary: Some(SummaryPayload {
                files_succeeded: 2,
                files_failed: 0,
                paths_skipped: 0,
                documents_succeeded: 2,
                documents_failed: 0,
                documents_skipped: 0,
                records_removed: 0,
                batches_committed: 1,
            }),
            error_kind: None,
        });

        assert_eq!(manager.status().expect("读取重扫状态失败").state, "idle");
        let finished = manager
            .last_finished()
            .expect("读取最近索引结果失败")
            .expect("缺少最近索引结果");
        assert_eq!(finished.scan_id, 9);
        assert_eq!(finished.status, "completed");
    }

    #[test]
    fn derives_actionable_index_health_states() {
        let clean = RescanFinishedEvent {
            scan_id: 1,
            status: "completed",
            message: "手动重扫完成。".to_owned(),
            summary: Some(SummaryPayload {
                files_succeeded: 2,
                files_failed: 0,
                paths_skipped: 0,
                documents_succeeded: 2,
                documents_failed: 0,
                documents_skipped: 0,
                records_removed: 0,
                batches_committed: 1,
            }),
            error_kind: None,
        };
        let partial = RescanFinishedEvent {
            summary: Some(SummaryPayload {
                files_failed: 1,
                ..clean.summary.expect("缺少健康状态测试汇总")
            }),
            ..clean.clone()
        };
        let failed = RescanFinishedEvent {
            status: "failed",
            summary: None,
            error_kind: Some("rescan_failed"),
            ..clean.clone()
        };
        let cancelled = RescanFinishedEvent {
            status: "cancelled",
            summary: None,
            error_kind: Some("rescan_cancelled"),
            ..clean.clone()
        };

        assert_eq!(
            derive_index_health(false, true, None, false).state,
            "indexing"
        );
        assert_eq!(
            derive_index_health(false, false, None, false).state,
            "not-configured"
        );
        assert_eq!(
            derive_index_health(true, false, Some(&clean), true).state,
            "ready"
        );
        assert_eq!(
            derive_index_health(true, false, Some(&clean), false).state,
            "degraded"
        );
        assert_eq!(
            derive_index_health(true, false, Some(&partial), true).state,
            "degraded"
        );
        assert_eq!(
            derive_index_health(true, false, Some(&failed), true).state,
            "failed"
        );
        assert_eq!(
            derive_index_health(true, false, Some(&cancelled), true).state,
            "cancelled"
        );
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
    fn runs_search_from_a_database_worker_boundary() {
        let sequence = SEARCH_TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let directory = env::temp_dir().join(format!(
            "nexus-desktop-search-test-{}-{sequence}",
            process::id()
        ));
        fs::create_dir_all(&directory).expect("创建搜索命令测试目录失败");
        let database_path = directory.join("nexus.sqlite3");
        let document_path = directory.join("notes.md");
        let connection = initialize_database(&database_path).expect("初始化搜索命令测试数据库失败");
        upsert_document(
            &connection,
            &DocumentRecord {
                id: "file:search-worker".to_owned(),
                source_path: document_path,
                title: "项目计划".to_owned(),
                body: "本地搜索工作边界".to_owned(),
                line_start: None,
                line_end: None,
            },
        )
        .expect("写入搜索命令测试文档失败");
        drop(connection);

        let response = search_documents_from_database(
            database_path,
            SearchRequest {
                query: "项目计划".to_owned(),
                limit: Some(10),
            },
        )
        .expect("执行搜索命令 worker 测试失败");

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].document_id, "file:search-worker");
        fs::remove_dir_all(directory).expect("清理搜索命令测试目录失败");
    }

    #[test]
    fn carries_rescan_scan_options_into_the_watch_configuration() {
        let scan_options = ScanOptions {
            ignored_paths: vec![PathBuf::from("ignored")],
            follow_symlinks: true,
        };

        let watch_options = watch_options_for_scan(&scan_options);

        assert_eq!(watch_options.scan_options, scan_options);
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

    #[test]
    fn validates_a_directory_before_starting_a_rescan() {
        let directory = env::temp_dir().join(format!(
            "nexus-rescan-root-validation-{}-{}",
            process::id(),
            WATCH_ROOT_TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).expect("创建重扫根目录校验测试目录失败");

        validate_rescan_root(&directory, false).expect("有效目录不应被拒绝");

        let file = directory.join("not-a-directory.txt");
        fs::write(&file, "content").expect("写入重扫根目录校验文件失败");
        let error = validate_rescan_root(&file, false).expect_err("文件路径不应作为扫描根目录");
        assert_eq!(error.code, "rescan_root_not_directory");

        fs::remove_dir_all(directory).expect("清理重扫根目录校验测试目录失败");
    }
}
