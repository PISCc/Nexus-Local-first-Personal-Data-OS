//! M4.3 桌面后台文件监听任务。
//!
//! 本模块只负责把核心 watcher、事件归并和增量处理放到独立线程，并管理取消与关闭。
//! 文件解析、批量写入和重试策略仍由 `nexus-core` 负责；UI 通过 Tauri 事件接收安全
//! 状态，不直接接触数据库或文件正文。

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use nexus_core::{
    apply_incremental_batch_with_retry, index_document_embeddings,
    refresh_document_embeddings_for_paths, watch_directory, EmbeddingIndexError,
    EmbeddingIndexOptions, EventBatchError, EventBatchOptions, EventBatcher, FileEvent,
    FileWatchError, IncrementalBatch, IncrementalBatchSummary, IncrementalChange,
    IncrementalIndexError, IncrementalIndexOptions, LocalFeatureEmbedding, RescanControl,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tracing::{error, warn};

pub const WATCH_STATUS_EVENT: &str = "watch-status";
pub const INCREMENTAL_FINISHED_EVENT: &str = "incremental-finished";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchStatusEvent {
    pub watch_id: u64,
    pub state: &'static str,
    pub message: &'static str,
    pub error_kind: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncrementalFinishedEvent {
    pub watch_id: u64,
    pub changes_received: usize,
    pub files_updated: usize,
    pub files_removed: usize,
    pub files_failed: usize,
    pub documents_updated: usize,
    pub documents_removed: usize,
    pub retries: usize,
    pub full_rescan: bool,
}

impl IncrementalFinishedEvent {
    fn from_summary(watch_id: u64, summary: IncrementalBatchSummary) -> Self {
        Self {
            watch_id,
            changes_received: summary.changes_received,
            files_updated: summary.files_updated,
            files_removed: summary.files_removed,
            files_failed: summary.files_failed,
            documents_updated: summary.documents_updated,
            documents_removed: summary.documents_removed,
            retries: summary.retries,
            full_rescan: summary.full_rescan,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchStatusResponse {
    pub state: &'static str,
    pub watch_id: Option<u64>,
}

#[derive(Debug)]
pub enum WatchManagerError {
    StateUnavailable,
    AlreadyRunning,
    ThreadStart,
}

impl WatchManagerError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::StateUnavailable => "watch_state_unavailable",
            Self::AlreadyRunning => "watch_already_running",
            Self::ThreadStart => "watch_thread_start",
        }
    }

    pub fn user_message(&self) -> &'static str {
        match self {
            Self::StateUnavailable => "无法读取文件监听状态。",
            Self::AlreadyRunning => "已有文件监听正在运行。",
            Self::ThreadStart => "无法启动文件监听任务。",
        }
    }
}

struct ActiveWatch {
    id: u64,
    control: RescanControl,
    shutdown: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    config: WatchRestartConfig,
}

#[derive(Clone)]
pub(crate) struct WatchRestartConfig {
    pub database_path: std::path::PathBuf,
    pub root_path: std::path::PathBuf,
    pub options: IncrementalIndexOptions,
}

impl WatchRestartConfig {
    pub(crate) fn matches(
        &self,
        database_path: &std::path::Path,
        root_path: &std::path::Path,
        options: &IncrementalIndexOptions,
    ) -> bool {
        self.database_path == database_path
            && self.root_path == root_path
            && self.options == *options
    }
}

#[derive(Clone, Default)]
pub struct WatchManager {
    active: Arc<Mutex<Option<ActiveWatch>>>,
    next_id: Arc<Mutex<u64>>,
}

struct WatchTask {
    id: u64,
    app: AppHandle,
    manager: WatchManager,
    control: RescanControl,
    shutdown: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    database_path: std::path::PathBuf,
    root_path: std::path::PathBuf,
    options: IncrementalIndexOptions,
    initial_rescan: bool,
}

impl WatchManager {
    pub fn start(
        &self,
        app: AppHandle,
        database_path: std::path::PathBuf,
        root_path: std::path::PathBuf,
        options: IncrementalIndexOptions,
    ) -> Result<u64, WatchManagerError> {
        self.start_internal(app, database_path, root_path, options, false)
    }

    pub fn start_after_rescan(
        &self,
        app: AppHandle,
        database_path: std::path::PathBuf,
        root_path: std::path::PathBuf,
        options: IncrementalIndexOptions,
    ) -> Result<u64, WatchManagerError> {
        self.start_internal(app, database_path, root_path, options, true)
    }

    fn start_internal(
        &self,
        app: AppHandle,
        database_path: std::path::PathBuf,
        root_path: std::path::PathBuf,
        options: IncrementalIndexOptions,
        initial_rescan: bool,
    ) -> Result<u64, WatchManagerError> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| WatchManagerError::StateUnavailable)?;
        if active.is_some() {
            return Err(WatchManagerError::AlreadyRunning);
        }

        let id = {
            let mut next_id = self
                .next_id
                .lock()
                .map_err(|_| WatchManagerError::StateUnavailable)?;
            *next_id = next_id.wrapping_add(1);
            if *next_id == 0 {
                *next_id = 1;
            }
            *next_id
        };
        let control = RescanControl::new();
        let shutdown = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let config = WatchRestartConfig {
            database_path: database_path.clone(),
            root_path: root_path.clone(),
            options: options.clone(),
        };
        let task = WatchTask {
            id,
            app,
            manager: self.clone(),
            control: control.clone(),
            shutdown: shutdown.clone(),
            paused: paused.clone(),
            database_path,
            root_path,
            options,
            initial_rescan,
        };

        *active = Some(ActiveWatch {
            id,
            control,
            shutdown,
            paused,
            join: None,
            config,
        });

        let thread_result = thread::Builder::new()
            .name(format!("nexus-watch-{id}"))
            .spawn(move || run_watch(task));
        let join = match thread_result {
            Ok(join) => join,
            Err(_) => {
                *active = None;
                return Err(WatchManagerError::ThreadStart);
            }
        };

        if let Some(watch) = active.as_mut() {
            watch.join = Some(join);
        }
        drop(active);
        Ok(id)
    }

    pub fn pause(&self) -> Result<Option<WatchRestartConfig>, WatchManagerError> {
        let active = self
            .active
            .lock()
            .map_err(|_| WatchManagerError::StateUnavailable)?;
        let Some(active) = active.as_ref() else {
            return Ok(None);
        };

        active.paused.store(true, Ordering::Release);
        Ok(Some(active.config.clone()))
    }

    pub fn resume(&self) -> Result<bool, WatchManagerError> {
        let active = self
            .active
            .lock()
            .map_err(|_| WatchManagerError::StateUnavailable)?;
        let Some(active) = active.as_ref() else {
            return Ok(false);
        };

        active.paused.store(false, Ordering::Release);
        Ok(true)
    }

    pub fn stop(&self) -> Option<WatchRestartConfig> {
        let active = self.active.lock().ok().and_then(|mut active| active.take());
        let mut active = active?;

        let config = active.config.clone();
        active.control.cancel();
        active.shutdown.store(true, Ordering::Release);
        if let Some(join) = active.join.take() {
            let _ = join.join();
        }
        Some(config)
    }

    pub fn status(&self) -> Result<WatchStatusResponse, WatchManagerError> {
        let active = self
            .active
            .lock()
            .map_err(|_| WatchManagerError::StateUnavailable)?;
        Ok(match active.as_ref() {
            Some(active) => WatchStatusResponse {
                state: "running",
                watch_id: Some(active.id),
            },
            None => WatchStatusResponse {
                state: "idle",
                watch_id: None,
            },
        })
    }

    fn clear(&self, id: u64) {
        if let Ok(mut active) = self.active.lock() {
            if active.as_ref().is_some_and(|active| active.id == id) {
                *active = None;
            }
        }
    }
}

fn run_watch(task: WatchTask) {
    emit_status(
        &task.app,
        WatchStatusEvent {
            watch_id: task.id,
            state: "starting",
            message: "正在准备文件自动同步。",
            error_kind: None,
        },
    );

    let watcher = match watch_directory(&task.root_path) {
        Ok(watcher) => watcher,
        Err(error) => {
            task.manager.clear(task.id);
            emit_status(
                &task.app,
                WatchStatusEvent {
                    watch_id: task.id,
                    state: "failed",
                    message: error.user_message(),
                    error_kind: Some(error.kind()),
                },
            );
            error!(error_kind = error.kind(), "文件监听启动失败");
            return;
        }
    };

    let mut batcher = match EventBatcher::new(&task.root_path, EventBatchOptions::default()) {
        Ok(batcher) => batcher,
        Err(error) => {
            task.manager.clear(task.id);
            emit_status(
                &task.app,
                WatchStatusEvent {
                    watch_id: task.id,
                    state: "failed",
                    message: error.user_message(),
                    error_kind: Some(error.kind()),
                },
            );
            error!(error_kind = error.kind(), "文件事件归并器启动失败");
            return;
        }
    };

    emit_status(
        &task.app,
        WatchStatusEvent {
            watch_id: task.id,
            state: "watching",
            message: "文件自动同步已开启。",
            error_kind: None,
        },
    );

    if task.initial_rescan {
        if let Err(error) = queue_full_rescan(&mut batcher) {
            warn!(error_kind = error.kind(), "无法安排监听启动后的追赶重扫");
        }
    }

    let mut retry_batch: Option<(IncrementalBatch, Instant)> = None;
    let mut terminal_error: Option<FileWatchError> = None;
    loop {
        if is_stopping(&task) {
            break;
        }

        if task.paused.load(Ordering::Acquire) {
            match watcher.recv_timeout(Duration::from_millis(50)) {
                Ok(Some(_event)) => {
                    if let Err(error) = queue_full_rescan(&mut batcher) {
                        warn!(error_kind = error.kind(), "忽略暂停期间的无效文件事件");
                    }
                }
                Ok(None) => {}
                Err(error @ FileWatchError::ChannelClosed) => {
                    terminal_error = Some(error);
                    break;
                }
                Err(error) => warn!(error_kind = error.kind(), "文件监听报告暂时性错误"),
            }
            continue;
        }

        if let Some((batch, retry_at)) = retry_batch.take() {
            if Instant::now() >= retry_at {
                match apply_batch_and_refresh(&task, &batch) {
                    Ok(summary) => emit_summary(&task.app, task.id, summary),
                    Err(_error) if is_stopping(&task) => break,
                    Err(error) => {
                        log_incremental_error(&error);
                        retry_batch = Some((batch, Instant::now() + retry_delay(&task.options)));
                    }
                }
            } else {
                retry_batch = Some((batch, retry_at));
            }
        }

        if retry_batch.is_none() && batcher.should_flush(Instant::now()) {
            if let Some(batch) = batcher.flush() {
                match apply_batch_and_refresh(&task, &batch) {
                    Ok(summary) => emit_summary(&task.app, task.id, summary),
                    Err(_error) if is_stopping(&task) => break,
                    Err(error) => {
                        log_incremental_error(&error);
                        retry_batch = Some((batch, Instant::now() + retry_delay(&task.options)));
                    }
                }
            }
        }

        match watcher.recv_timeout(Duration::from_millis(50)) {
            Ok(Some(event)) => {
                if is_stopping(&task) {
                    break;
                }
                if let Err(error) = batcher.push(event, Instant::now()) {
                    warn!(error_kind = error.kind(), "忽略无效的增量事件");
                }
            }
            Ok(None) => {}
            Err(error @ FileWatchError::ChannelClosed) => {
                terminal_error = Some(error);
                break;
            }
            Err(error) => warn!(error_kind = error.kind(), "文件监听报告暂时性错误"),
        }
    }

    let (state, message, error_kind) = terminal_watch_status(terminal_error.as_ref());
    task.manager.clear(task.id);
    emit_status(
        &task.app,
        WatchStatusEvent {
            watch_id: task.id,
            state,
            message,
            error_kind,
        },
    );
}

fn queue_full_rescan(batcher: &mut EventBatcher) -> Result<(), EventBatchError> {
    batcher.push(
        FileEvent::RescanRequired {
            root: batcher.root().to_path_buf(),
        },
        Instant::now(),
    )
}

fn terminal_watch_status(
    error: Option<&FileWatchError>,
) -> (&'static str, &'static str, Option<&'static str>) {
    match error {
        Some(error) => (
            "failed",
            "文件自动同步已失效，需要重新启动。",
            Some(error.kind()),
        ),
        None => ("stopped", "文件自动同步已停止。", None),
    }
}

fn apply_batch(
    task: &WatchTask,
    batch: &IncrementalBatch,
) -> Result<IncrementalBatchSummary, IncrementalIndexError> {
    apply_incremental_batch_with_retry(&task.database_path, batch, &task.options, &task.control)
}

fn apply_batch_and_refresh(
    task: &WatchTask,
    batch: &IncrementalBatch,
) -> Result<IncrementalBatchSummary, IncrementalIndexError> {
    let summary = apply_batch(task, batch)?;
    refresh_embeddings(task, batch);
    Ok(summary)
}

fn refresh_embeddings(task: &WatchTask, batch: &IncrementalBatch) {
    let provider = LocalFeatureEmbedding::new();
    let result = match batch {
        IncrementalBatch::Changes { changes } => {
            let paths = changes
                .iter()
                .filter_map(|change| match change {
                    IncrementalChange::Upsert { path } => Some(path.clone()),
                    IncrementalChange::Remove { .. } => None,
                })
                .collect::<Vec<_>>();
            if paths.is_empty() {
                return;
            }
            refresh_document_embeddings_for_paths(
                &task.database_path,
                &paths,
                &provider,
                EmbeddingIndexOptions::default(),
                &task.control,
            )
        }
        IncrementalBatch::RescanRequired { .. } => index_document_embeddings(
            &task.database_path,
            &provider,
            EmbeddingIndexOptions::default(),
            &task.control,
        ),
    };

    if let Err(error) = result {
        log_embedding_error(&error);
    }
}

fn emit_status(app: &AppHandle, event: WatchStatusEvent) {
    let _ = app.emit(WATCH_STATUS_EVENT, event);
}

fn emit_summary(app: &AppHandle, watch_id: u64, summary: IncrementalBatchSummary) {
    let _ = app.emit(
        INCREMENTAL_FINISHED_EVENT,
        IncrementalFinishedEvent::from_summary(watch_id, summary),
    );
}

fn log_incremental_error(error: &IncrementalIndexError) {
    warn!(error_kind = error.kind(), "增量批次暂时未能提交，稍后重试");
}

fn log_embedding_error(error: &EmbeddingIndexError) {
    warn!(error_kind = error.kind(), "增量 embedding 暂时未能更新");
}

fn retry_delay(options: &IncrementalIndexOptions) -> Duration {
    if options.retry_delay.is_zero() {
        Duration::from_millis(250)
    } else {
        options.retry_delay
    }
}

fn is_stopping(task: &WatchTask) -> bool {
    task.shutdown.load(Ordering::Acquire) || task.control.is_cancelled()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{queue_full_rescan, retry_delay, terminal_watch_status, WatchManager};
    use nexus_core::{
        EventBatchOptions, FileWatchError, IncrementalBatch, IncrementalIndexOptions,
    };

    #[test]
    fn manager_starts_idle_and_stop_is_safe() {
        let manager = WatchManager::default();

        let status = manager.status().expect("读取空闲监听状态失败");
        assert_eq!(status.state, "idle");
        assert_eq!(status.watch_id, None);

        manager.stop();
        let status = manager.status().expect("读取停止后监听状态失败");
        assert_eq!(status.state, "idle");
    }

    #[test]
    fn zero_retry_delay_uses_a_bounded_background_retry_delay() {
        let options = IncrementalIndexOptions {
            retry_delay: Duration::ZERO,
            ..IncrementalIndexOptions::default()
        };

        assert_eq!(retry_delay(&options), Duration::from_millis(250));
    }

    #[test]
    fn channel_closure_is_reported_as_a_failed_watch() {
        let status = terminal_watch_status(Some(&FileWatchError::ChannelClosed));

        assert_eq!(
            status,
            (
                "failed",
                "文件自动同步已失效，需要重新启动。",
                Some("file_watch_channel_closed")
            )
        );
    }

    #[test]
    fn paused_events_are_coalesced_into_one_catch_up_rescan() {
        let root = std::env::temp_dir().join("nexus-paused-watch-test");
        let mut batcher = nexus_core::EventBatcher::new(&root, EventBatchOptions::default())
            .expect("创建事件归并器失败");
        let normalized_root = batcher.root().to_path_buf();

        queue_full_rescan(&mut batcher).expect("记录暂停期间的第一次变化失败");
        queue_full_rescan(&mut batcher).expect("记录暂停期间的第二次变化失败");

        assert_eq!(
            batcher.flush(),
            Some(IncrementalBatch::RescanRequired {
                root: normalized_root,
            })
        );
    }
}
