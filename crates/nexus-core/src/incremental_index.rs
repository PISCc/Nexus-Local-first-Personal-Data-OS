//! M4.2 增量事件归并与文件索引更新。
//!
//! 本模块把 M4.1 的文件事件压缩为有限批次，并在读取文件正文前再次确认文件状态。
//! 批次写入通过 `nexus-db` 的单事务边界完成；线程、取消和应用关闭由桌面层负责。

use std::{
    collections::BTreeMap,
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use nexus_db::{
    apply_file_mutations, initialize_database, DatabaseError, DocumentRecord, FileMetadata,
    FileMetadataError, FileMutation, FileMutationError, FileMutationUpsert,
};

use super::{
    document_id_for_path, index_directory_with_control, parse_file, DocumentError, FileEvent,
    FileSnapshot, ParseError, ParseOptions, RescanControl, RescanError, RescanSummary, ScanOptions,
};

/// M4.2 事件归并参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventBatchOptions {
    /// 最后一次事件之后需要等待的安静窗口。
    pub debounce_window: Duration,
    /// 一个批次最多包含多少个不同路径；达到后立即允许提交。
    pub max_paths: usize,
}

impl Default for EventBatchOptions {
    fn default() -> Self {
        Self {
            debounce_window: Duration::from_millis(250),
            max_paths: 128,
        }
    }
}

/// 归并后的单路径增量操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncrementalChange {
    /// 重新读取路径当前状态，并在支持时更新正文。
    Upsert { path: PathBuf },
    /// 删除该路径对应的元数据和正文记录。
    Remove { path: PathBuf },
}

/// 一个等待交给增量索引器的有限批次。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncrementalBatch {
    Changes { changes: Vec<IncrementalChange> },
    RescanRequired { root: PathBuf },
}

/// 文件事件归并时的输入错误。
#[derive(Debug)]
pub enum EventBatchError {
    InvalidRoot { source: FileMetadataError },
    InvalidMaxPaths,
    EmptyPath,
    OutsideRoot,
}

impl EventBatchError {
    /// 返回不包含路径或原始错误的安全分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidRoot { .. } => "event_batch_root_invalid",
            Self::InvalidMaxPaths => "event_batch_size_invalid",
            Self::EmptyPath => "event_batch_path_empty",
            Self::OutsideRoot => "event_batch_path_outside_root",
        }
    }

    /// 返回可以直接展示给用户的非敏感说明。
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::InvalidRoot { .. } => "增量索引监听目录无效。",
            Self::InvalidMaxPaths => "增量索引批次大小无效。",
            Self::EmptyPath => "增量索引收到空路径。",
            Self::OutsideRoot => "增量索引收到监听范围外的路径。",
        }
    }
}

impl fmt::Display for EventBatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "增量事件归并失败: {}", self.kind())
    }
}

impl Error for EventBatchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRoot { source } => Some(source),
            Self::InvalidMaxPaths | Self::EmptyPath | Self::OutsideRoot => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingOperation {
    Upsert,
    Remove,
}

/// 在安静窗口结束或达到路径上限后输出一个归并批次。
#[derive(Debug)]
pub struct EventBatcher {
    root: PathBuf,
    options: EventBatchOptions,
    pending: BTreeMap<PathBuf, PendingOperation>,
    pending_rescan: Option<PathBuf>,
    last_event_at: Option<Instant>,
}

impl EventBatcher {
    /// 创建一个只接受指定根目录及其子路径的事件归并器。
    pub fn new<P: AsRef<Path>>(
        root: P,
        options: EventBatchOptions,
    ) -> Result<Self, EventBatchError> {
        if options.max_paths == 0 {
            return Err(EventBatchError::InvalidMaxPaths);
        }

        let root = nexus_db::normalize_path(root.as_ref())
            .map_err(|source| EventBatchError::InvalidRoot { source })?;
        Ok(Self {
            root,
            options,
            pending: BTreeMap::new(),
            pending_rescan: None,
            last_event_at: None,
        })
    }

    /// 返回归并器监听的根目录。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 把一个底层事件加入待处理集合。
    ///
    /// 同一路径只保留最终操作：创建/修改最终归并为 `Upsert`，删除最终归并为
    /// `Remove`。重命名会同时记录旧路径删除和新路径更新。完整重扫信号会覆盖
    /// 同一批次内尚未提交的局部事件。
    pub fn push(&mut self, event: FileEvent, now: Instant) -> Result<(), EventBatchError> {
        if self.pending_rescan.is_some() {
            return Ok(());
        }

        match event {
            FileEvent::Created { path } | FileEvent::Modified { path } => {
                self.record_path(path, PendingOperation::Upsert)?;
            }
            FileEvent::Removed { path } => {
                self.record_path(path, PendingOperation::Remove)?;
            }
            FileEvent::Renamed { from, to } => {
                if from == to {
                    self.record_path(to, PendingOperation::Upsert)?;
                } else {
                    self.record_path(from, PendingOperation::Remove)?;
                    self.record_path(to, PendingOperation::Upsert)?;
                }
            }
            FileEvent::RescanRequired { root } => {
                let root = self.normalize_path(root)?;
                if root != self.root {
                    return Err(EventBatchError::OutsideRoot);
                }
                self.pending.clear();
                self.pending_rescan = Some(root);
            }
        }

        self.last_event_at = Some(now);
        Ok(())
    }

    /// 判断当前批次是否已经可以提交。
    pub fn should_flush(&self, now: Instant) -> bool {
        if self.pending_rescan.is_some() || self.pending.len() >= self.options.max_paths {
            return true;
        }

        let Some(last_event_at) = self.last_event_at else {
            return false;
        };
        let Some(deadline) = last_event_at.checked_add(self.options.debounce_window) else {
            return true;
        };
        now >= deadline
    }

    /// 取出当前批次；没有待处理事件时返回 `None`。
    pub fn flush(&mut self) -> Option<IncrementalBatch> {
        self.last_event_at = None;

        if let Some(root) = self.pending_rescan.take() {
            self.pending.clear();
            return Some(IncrementalBatch::RescanRequired { root });
        }

        if self.pending.is_empty() {
            return None;
        }

        let changes = std::mem::take(&mut self.pending)
            .into_iter()
            .map(|(path, operation)| match operation {
                PendingOperation::Upsert => IncrementalChange::Upsert { path },
                PendingOperation::Remove => IncrementalChange::Remove { path },
            })
            .collect();
        Some(IncrementalBatch::Changes { changes })
    }

    fn record_path(
        &mut self,
        path: PathBuf,
        operation: PendingOperation,
    ) -> Result<(), EventBatchError> {
        let path = self.normalize_path(path)?;
        self.pending.insert(path, operation);
        Ok(())
    }

    fn normalize_path(&self, path: PathBuf) -> Result<PathBuf, EventBatchError> {
        if path.as_os_str().is_empty() {
            return Err(EventBatchError::EmptyPath);
        }

        let path = nexus_db::normalize_path(&path).map_err(|_| EventBatchError::EmptyPath)?;
        if path == self.root || path.starts_with(&self.root) {
            Ok(path)
        } else {
            Err(EventBatchError::OutsideRoot)
        }
    }
}

/// 增量索引处理参数。
#[derive(Debug, Clone)]
pub struct IncrementalIndexOptions {
    /// 完整重扫使用的扫描选项。
    pub scan_options: ScanOptions,
    /// 完整重扫使用的数据库批次大小。
    pub batch_size: usize,
    /// 单文件正文解析边界。
    pub parse_options: ParseOptions,
    /// 解析前后最多重新确认多少次文件状态。
    pub stability_checks: usize,
    /// 两次稳定性确认之间的等待时间。
    pub stability_delay: Duration,
    /// 数据库事务失败后的最多重试次数。
    pub max_retries: usize,
    /// 数据库事务重试之间的等待时间。
    pub retry_delay: Duration,
}

impl Default for IncrementalIndexOptions {
    fn default() -> Self {
        Self {
            scan_options: ScanOptions::default(),
            batch_size: 512,
            parse_options: ParseOptions::default(),
            stability_checks: 2,
            stability_delay: Duration::from_millis(50),
            max_retries: 2,
            retry_delay: Duration::from_millis(100),
        }
    }
}

/// 一次增量批次的结果统计。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IncrementalBatchSummary {
    pub changes_received: usize,
    pub files_updated: usize,
    pub files_removed: usize,
    pub files_failed: usize,
    pub documents_updated: usize,
    pub documents_removed: usize,
    pub retries: usize,
    pub full_rescan: bool,
}

/// 增量索引任务错误。
#[derive(Debug)]
pub enum IncrementalIndexError {
    InvalidBatchSize,
    Database { source: DatabaseError },
    Mutation { source: FileMutationError },
    Document { source: DocumentError },
    Rescan { source: RescanError },
    Cancelled,
}

impl IncrementalIndexError {
    /// 返回不包含路径、正文或底层错误文本的安全分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidBatchSize => "incremental_batch_size_invalid",
            Self::Database { source } => source.kind(),
            Self::Mutation { source } => source.kind(),
            Self::Document { source } => source.kind(),
            Self::Rescan { source } => source.kind(),
            Self::Cancelled => "incremental_cancelled",
        }
    }

    /// 返回可以直接展示给用户的非敏感说明。
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::InvalidBatchSize => "增量索引批次大小无效。",
            Self::Database { .. } | Self::Mutation { .. } => "增量索引暂时无法保存。",
            Self::Document { source } => source.user_message(),
            Self::Rescan { source } => source.user_message(),
            Self::Cancelled => "增量索引已取消。",
        }
    }

    fn retryable(&self) -> bool {
        matches!(self, Self::Database { .. } | Self::Mutation { .. })
    }
}

impl fmt::Display for IncrementalIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "增量索引失败: {}", self.kind())
    }
}

impl Error for IncrementalIndexError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database { source } => Some(source),
            Self::Mutation { source } => Some(source),
            Self::Document { source } => Some(source),
            Self::Rescan { source } => Some(source),
            Self::InvalidBatchSize | Self::Cancelled => None,
        }
    }
}

/// 对一个已归并批次执行有界、可重试的增量写入。
///
/// 文件内容和元数据只在当前批次的数据库事务提交前准备；单文件状态不稳定或解析
/// 失败时会保留旧正文并统计失败，不会把不完整正文写入 canonical 表。数据库事务
/// 失败时整个批次回滚，并按选项重试。
pub fn apply_incremental_batch_with_retry<D: AsRef<Path>>(
    database_path: D,
    batch: &IncrementalBatch,
    options: &IncrementalIndexOptions,
    control: &RescanControl,
) -> Result<IncrementalBatchSummary, IncrementalIndexError> {
    if options.batch_size == 0 {
        return Err(IncrementalIndexError::InvalidBatchSize);
    }

    for attempt in 0..=options.max_retries {
        if control.is_cancelled() {
            return Err(IncrementalIndexError::Cancelled);
        }

        match apply_incremental_batch_once(database_path.as_ref(), batch, options, control) {
            Ok(mut summary) => {
                summary.retries = attempt;
                return Ok(summary);
            }
            Err(error)
                if error.retryable()
                    && attempt < options.max_retries
                    && !control.is_cancelled() =>
            {
                sleep_with_cancel(options.retry_delay, control)?;
            }
            Err(error) => return Err(error),
        }
    }

    Err(IncrementalIndexError::Cancelled)
}

fn apply_incremental_batch_once(
    database_path: &Path,
    batch: &IncrementalBatch,
    options: &IncrementalIndexOptions,
    control: &RescanControl,
) -> Result<IncrementalBatchSummary, IncrementalIndexError> {
    match batch {
        IncrementalBatch::RescanRequired { root } => {
            let summary = index_directory_with_control(
                database_path,
                root,
                options.scan_options.clone(),
                options.batch_size,
                control.clone(),
                |_| {},
            )
            .map_err(|source| {
                if matches!(source, RescanError::Cancelled) {
                    IncrementalIndexError::Cancelled
                } else {
                    IncrementalIndexError::Rescan { source }
                }
            })?;
            Ok(summary_from_rescan(summary))
        }
        IncrementalBatch::Changes { changes } => {
            apply_incremental_changes(database_path, changes, options, control)
        }
    }
}

fn apply_incremental_changes(
    database_path: &Path,
    changes: &[IncrementalChange],
    options: &IncrementalIndexOptions,
    control: &RescanControl,
) -> Result<IncrementalBatchSummary, IncrementalIndexError> {
    if changes.is_empty() {
        return Ok(IncrementalBatchSummary::default());
    }

    let mut connection = initialize_database(database_path)
        .map_err(|source| IncrementalIndexError::Database { source })?;
    let mut mutations = Vec::with_capacity(changes.len());
    let mut summary = IncrementalBatchSummary {
        changes_received: changes.len(),
        ..IncrementalBatchSummary::default()
    };

    for change in changes {
        if control.is_cancelled() {
            return Err(IncrementalIndexError::Cancelled);
        }

        match prepare_change(change, options, control)? {
            PreparedChange::Mutation(mutation) => {
                match &mutation {
                    FileMutation::Upsert(mutation) => {
                        summary.files_updated += 1;
                        if mutation.document.is_some() {
                            summary.documents_updated += 1;
                        }
                    }
                    FileMutation::Remove { .. } => summary.files_removed += 1,
                }
                mutations.push(mutation);
            }
            PreparedChange::Failed => summary.files_failed += 1,
        }
    }

    if mutations.is_empty() {
        return Ok(summary);
    }

    let mutation_summary = apply_file_mutations(&mut connection, &mutations)
        .map_err(|source| IncrementalIndexError::Mutation { source })?;
    summary.documents_removed = mutation_summary.documents_removed;
    Ok(summary)
}

#[derive(Debug)]
enum PreparedChange {
    Mutation(FileMutation),
    Failed,
}

fn prepare_change(
    change: &IncrementalChange,
    options: &IncrementalIndexOptions,
    control: &RescanControl,
) -> Result<PreparedChange, IncrementalIndexError> {
    match change {
        IncrementalChange::Remove { path } => Ok(PreparedChange::Mutation(FileMutation::Remove {
            path: path.clone(),
        })),
        IncrementalChange::Upsert { path } => prepare_upsert(path, options, control),
    }
}

fn prepare_upsert(
    path: &Path,
    options: &IncrementalIndexOptions,
    control: &RescanControl,
) -> Result<PreparedChange, IncrementalIndexError> {
    for attempt in 0..=options.stability_checks {
        if control.is_cancelled() {
            return Err(IncrementalIndexError::Cancelled);
        }

        let Some(before) = (match read_current_file_metadata(path) {
            Ok(metadata) => metadata,
            Err(_) => return Ok(PreparedChange::Failed),
        }) else {
            return Ok(PreparedChange::Mutation(FileMutation::Remove {
                path: path.to_path_buf(),
            }));
        };

        let document = match parse_file(
            document_id_for_path(&before.path)
                .map_err(|source| IncrementalIndexError::Document { source })?,
            &before.path,
            options.parse_options,
        ) {
            Ok(document) => Some(document_record(document)),
            Err(ParseError::UnsupportedExtension) => None,
            Err(_error) => {
                if attempt < options.stability_checks {
                    sleep_with_cancel(options.stability_delay, control)?;
                    continue;
                }
                break;
            }
        };

        let Some(after) = (match read_current_file_metadata(path) {
            Ok(metadata) => metadata,
            Err(_) => return Ok(PreparedChange::Failed),
        }) else {
            return Ok(PreparedChange::Mutation(FileMutation::Remove {
                path: path.to_path_buf(),
            }));
        };

        if same_snapshot(&before, &after) {
            return Ok(PreparedChange::Mutation(FileMutation::Upsert(Box::new(
                FileMutationUpsert {
                    metadata: after,
                    document,
                },
            ))));
        }

        if attempt < options.stability_checks {
            sleep_with_cancel(options.stability_delay, control)?;
        }
    }

    Ok(PreparedChange::Failed)
}

#[derive(Debug, Clone, Copy)]
enum ItemFailure {
    Metadata,
}

fn read_current_file_metadata(path: &Path) -> Result<Option<FileMetadata>, ItemFailure> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ItemFailure::Metadata),
    };

    if !metadata.file_type().is_file() {
        return Ok(None);
    }

    FileMetadata::from_path(
        path.to_path_buf(),
        metadata.len(),
        timestamp_millis(metadata.modified()),
        timestamp_millis(metadata.created()),
        timestamp_millis(metadata.accessed()),
        None,
    )
    .map(Some)
    .map_err(|_| ItemFailure::Metadata)
}

fn document_record(document: super::Document) -> DocumentRecord {
    DocumentRecord {
        id: document.id.as_str().to_owned(),
        source_path: document.source.path().to_path_buf(),
        title: document.title,
        body: document.body,
        line_start: document.location.line_start(),
        line_end: document.location.line_end(),
    }
}

fn same_snapshot(before: &FileMetadata, after: &FileMetadata) -> bool {
    FileSnapshot::from_metadata(before).size_bytes == FileSnapshot::from_metadata(after).size_bytes
        && FileSnapshot::from_metadata(before).modified_at
            == FileSnapshot::from_metadata(after).modified_at
}

fn timestamp_millis(timestamp: Result<SystemTime, io::Error>) -> Option<i64> {
    let timestamp = timestamp.ok()?;
    match timestamp.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).ok(),
        Err(error) => i64::try_from(error.duration().as_millis())
            .ok()
            .and_then(|millis| millis.checked_neg()),
    }
}

fn sleep_with_cancel(
    duration: Duration,
    control: &RescanControl,
) -> Result<(), IncrementalIndexError> {
    if duration.is_zero() {
        return if control.is_cancelled() {
            Err(IncrementalIndexError::Cancelled)
        } else {
            Ok(())
        };
    }

    let started_at = Instant::now();
    let mut remaining = duration;
    while !remaining.is_zero() {
        if control.is_cancelled() {
            return Err(IncrementalIndexError::Cancelled);
        }
        thread::sleep(remaining.min(Duration::from_millis(20)));
        remaining = duration.saturating_sub(started_at.elapsed());
    }
    Ok(())
}

fn summary_from_rescan(summary: RescanSummary) -> IncrementalBatchSummary {
    IncrementalBatchSummary {
        changes_received: summary.files_succeeded + summary.files_failed + summary.paths_skipped,
        files_updated: summary.files_succeeded,
        files_removed: summary.records_removed,
        files_failed: summary.files_failed,
        documents_updated: summary.documents_succeeded,
        documents_removed: 0,
        retries: 0,
        full_rescan: true,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::{Path, PathBuf},
        process,
        sync::atomic::{AtomicUsize, Ordering},
        time::{Duration, Instant},
    };

    use nexus_db::{get_document, get_file_metadata, initialize_database};

    use super::{
        apply_incremental_batch_with_retry, document_id_for_path, EventBatchError,
        EventBatchOptions, EventBatcher, IncrementalBatch, IncrementalBatchSummary,
        IncrementalChange, IncrementalIndexOptions,
    };
    use crate::{FileEvent, RescanControl};

    static TEMP_DIRECTORY_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TemporaryDirectory {
        path: PathBuf,
    }

    impl TemporaryDirectory {
        fn new() -> Self {
            let sequence = TEMP_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "nexus-incremental-test-{}-{}",
                process::id(),
                sequence
            ));
            fs::create_dir_all(&path).expect("创建增量索引测试目录失败");
            Self { path }
        }

        fn child_path(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }

        fn database_path(&self) -> PathBuf {
            self.path.join("nexus.sqlite3")
        }
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn batcher(root: &Path) -> EventBatcher {
        EventBatcher::new(
            root,
            EventBatchOptions {
                debounce_window: Duration::from_millis(100),
                max_paths: 8,
            },
        )
        .expect("创建事件归并器失败")
    }

    #[test]
    fn coalesces_repeated_events_and_flushes_after_quiet_window() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("notes.md");
        let start = Instant::now();
        let mut batcher = batcher(&temporary_directory.path);

        batcher
            .push(FileEvent::Created { path: path.clone() }, start)
            .expect("加入创建事件失败");
        batcher
            .push(
                FileEvent::Modified { path: path.clone() },
                start + Duration::from_millis(20),
            )
            .expect("加入重复修改事件失败");

        assert!(!batcher.should_flush(start + Duration::from_millis(99)));
        assert!(batcher.should_flush(start + Duration::from_millis(120)));
        assert_eq!(
            batcher.flush(),
            Some(IncrementalBatch::Changes {
                changes: vec![IncrementalChange::Upsert { path }]
            })
        );
    }

    #[test]
    fn final_path_operation_wins_for_out_of_order_events() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("notes.md");
        let start = Instant::now();
        let mut batcher = batcher(&temporary_directory.path);

        batcher
            .push(FileEvent::Removed { path: path.clone() }, start)
            .expect("加入删除事件失败");
        batcher
            .push(FileEvent::Created { path: path.clone() }, start)
            .expect("加入恢复事件失败");

        assert_eq!(
            batcher.flush(),
            Some(IncrementalBatch::Changes {
                changes: vec![IncrementalChange::Upsert { path }]
            })
        );
    }

    #[test]
    fn coalesces_rename_into_old_remove_and_new_upsert() {
        let temporary_directory = TemporaryDirectory::new();
        let from = temporary_directory.child_path("old.md");
        let to = temporary_directory.child_path("new.md");
        let mut batcher = batcher(&temporary_directory.path);

        batcher
            .push(
                FileEvent::Renamed {
                    from: from.clone(),
                    to: to.clone(),
                },
                Instant::now(),
            )
            .expect("加入重命名事件失败");

        assert_eq!(
            batcher.flush(),
            Some(IncrementalBatch::Changes {
                changes: vec![
                    IncrementalChange::Upsert { path: to },
                    IncrementalChange::Remove { path: from },
                ]
            })
        );
    }

    #[test]
    fn rescan_signal_discards_local_events() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("notes.md");
        let mut batcher = batcher(&temporary_directory.path);

        batcher
            .push(FileEvent::Modified { path }, Instant::now())
            .expect("加入修改事件失败");
        batcher
            .push(
                FileEvent::RescanRequired {
                    root: temporary_directory.path.clone(),
                },
                Instant::now(),
            )
            .expect("加入重扫事件失败");

        assert!(batcher.should_flush(Instant::now()));
        assert_eq!(
            batcher.flush(),
            Some(IncrementalBatch::RescanRequired {
                root: temporary_directory.path.clone()
            })
        );
    }

    #[test]
    fn rejects_outside_root_and_zero_batch_size() {
        let temporary_directory = TemporaryDirectory::new();
        let outside = env::temp_dir().join("nexus-incremental-outside.md");
        let invalid_batcher = EventBatcher::new(
            &temporary_directory.path,
            EventBatchOptions {
                debounce_window: Duration::ZERO,
                max_paths: 0,
            },
        )
        .expect_err("零批次大小不应创建归并器");
        assert!(matches!(invalid_batcher, EventBatchError::InvalidMaxPaths));

        let mut batcher = batcher(&temporary_directory.path);
        let error = batcher
            .push(FileEvent::Modified { path: outside }, Instant::now())
            .expect_err("监听范围外路径不应进入归并器");
        assert!(matches!(error, EventBatchError::OutsideRoot));
    }

    #[test]
    fn indexes_create_update_remove_and_duplicate_events_without_duplicate_rows() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let path = temporary_directory.child_path("notes.md");
        fs::write(&path, "first body").expect("写入初始文件失败");
        let mut event_batcher = batcher(&temporary_directory.path);
        let now = Instant::now();

        event_batcher
            .push(FileEvent::Created { path: path.clone() }, now)
            .expect("加入创建事件失败");
        event_batcher
            .push(
                FileEvent::Modified { path: path.clone() },
                now + Duration::from_millis(1),
            )
            .expect("加入重复更新事件失败");
        let batch = event_batcher.flush().expect("缺少创建批次");
        let options = IncrementalIndexOptions {
            stability_checks: 0,
            stability_delay: Duration::ZERO,
            max_retries: 0,
            retry_delay: Duration::ZERO,
            ..IncrementalIndexOptions::default()
        };
        let summary = apply_incremental_batch_with_retry(
            &database_path,
            &batch,
            &options,
            &RescanControl::new(),
        )
        .expect("创建文件增量索引失败");
        assert_eq!(summary.files_updated, 1);
        assert_eq!(summary.documents_updated, 1);

        let connection = initialize_database(&database_path).expect("打开增量测试数据库失败");
        let document = get_document(&connection, "file:missing-after-hash").expect("读取文档失败");
        assert!(document.is_none());
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
            .expect("统计文档数量失败");
        assert_eq!(count, 1);

        fs::write(&path, "second body").expect("更新文件失败");
        let summary = apply_incremental_batch_with_retry(
            &database_path,
            &IncrementalBatch::Changes {
                changes: vec![IncrementalChange::Upsert { path: path.clone() }],
            },
            &options,
            &RescanControl::new(),
        )
        .expect("更新文件增量索引失败");
        assert_eq!(summary.documents_updated, 1);

        let connection = initialize_database(&database_path).expect("重新打开增量测试数据库失败");
        let metadata = get_file_metadata(&connection, &path)
            .expect("读取更新后的元数据失败")
            .expect("缺少更新后的元数据");
        assert_eq!(metadata.file_name, "notes.md");

        fs::remove_file(&path).expect("删除测试文件失败");
        let summary = apply_incremental_batch_with_retry(
            &database_path,
            &IncrementalBatch::Changes {
                changes: vec![IncrementalChange::Remove { path: path.clone() }],
            },
            &options,
            &RescanControl::new(),
        )
        .expect("删除文件增量索引失败");
        assert_eq!(summary.files_removed, 1);

        let connection = initialize_database(&database_path).expect("打开删除后数据库失败");
        assert!(get_file_metadata(&connection, &path)
            .expect("读取删除后的元数据失败")
            .is_none());
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
            .expect("统计删除后文档数量失败");
        assert_eq!(count, 0);
    }

    #[test]
    fn keeps_previous_document_when_file_is_malformed() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let path = temporary_directory.child_path("notes.md");
        fs::write(&path, "valid body").expect("写入有效文件失败");
        let options = IncrementalIndexOptions {
            stability_checks: 0,
            stability_delay: Duration::ZERO,
            max_retries: 0,
            retry_delay: Duration::ZERO,
            ..IncrementalIndexOptions::default()
        };
        apply_incremental_batch_with_retry(
            &database_path,
            &IncrementalBatch::Changes {
                changes: vec![IncrementalChange::Upsert { path: path.clone() }],
            },
            &options,
            &RescanControl::new(),
        )
        .expect("建立有效文件索引失败");

        fs::write(&path, [0xff, 0xfe]).expect("写入损坏文件失败");
        let summary = apply_incremental_batch_with_retry(
            &database_path,
            &IncrementalBatch::Changes {
                changes: vec![IncrementalChange::Upsert { path }],
            },
            &options,
            &RescanControl::new(),
        )
        .expect("损坏文件处理不应终止批次");
        assert_eq!(summary.files_failed, 1);
        assert_eq!(summary.documents_updated, 0);
    }

    #[test]
    fn reaches_final_state_after_move_and_atomic_save() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let old_path = temporary_directory.child_path("old.md");
        let moved_path = temporary_directory.child_path("moved.md");
        let temporary_path = temporary_directory.child_path("atomic.md.tmp");
        let atomic_path = temporary_directory.child_path("atomic.md");
        let options = IncrementalIndexOptions {
            stability_checks: 0,
            stability_delay: Duration::ZERO,
            max_retries: 0,
            retry_delay: Duration::ZERO,
            ..IncrementalIndexOptions::default()
        };

        fs::write(&old_path, "moved body").expect("写入移动前文件失败");
        apply_incremental_batch_with_retry(
            &database_path,
            &IncrementalBatch::Changes {
                changes: vec![IncrementalChange::Upsert {
                    path: old_path.clone(),
                }],
            },
            &options,
            &RescanControl::new(),
        )
        .expect("建立移动前索引失败");

        fs::rename(&old_path, &moved_path).expect("移动文件失败");
        let mut move_batcher = batcher(&temporary_directory.path);
        move_batcher
            .push(
                FileEvent::Renamed {
                    from: old_path.clone(),
                    to: moved_path.clone(),
                },
                Instant::now(),
            )
            .expect("加入移动事件失败");
        let move_summary = apply_incremental_batch_with_retry(
            &database_path,
            &move_batcher.flush().expect("缺少移动批次"),
            &options,
            &RescanControl::new(),
        )
        .expect("应用移动事件失败");
        assert_eq!(move_summary.files_updated, 1);
        assert_eq!(move_summary.files_removed, 1);

        let connection = initialize_database(&database_path).expect("打开移动后数据库失败");
        assert!(get_file_metadata(&connection, &old_path)
            .expect("读取移动前元数据失败")
            .is_none());
        assert!(get_file_metadata(&connection, &moved_path)
            .expect("读取移动后元数据失败")
            .is_some());
        let moved_id = document_id_for_path(&moved_path).expect("生成移动后文档标识失败");
        assert_eq!(
            get_document(&connection, moved_id.as_str())
                .expect("读取移动后文档失败")
                .expect("缺少移动后文档")
                .body,
            "moved body"
        );
        drop(connection);

        fs::write(&temporary_path, "atomic body").expect("写入原子保存临时文件失败");
        fs::rename(&temporary_path, &atomic_path).expect("完成原子保存重命名失败");
        let mut atomic_batcher = batcher(&temporary_directory.path);
        atomic_batcher
            .push(
                FileEvent::Renamed {
                    from: temporary_path.clone(),
                    to: atomic_path.clone(),
                },
                Instant::now(),
            )
            .expect("加入原子保存事件失败");
        apply_incremental_batch_with_retry(
            &database_path,
            &atomic_batcher.flush().expect("缺少原子保存批次"),
            &options,
            &RescanControl::new(),
        )
        .expect("应用原子保存事件失败");

        let connection = initialize_database(&database_path).expect("打开原子保存数据库失败");
        assert!(get_file_metadata(&connection, &temporary_path)
            .expect("读取临时文件元数据失败")
            .is_none());
        let atomic_id = document_id_for_path(&atomic_path).expect("生成原子保存文档标识失败");
        assert_eq!(
            get_document(&connection, atomic_id.as_str())
                .expect("读取原子保存文档失败")
                .expect("缺少原子保存文档")
                .body,
            "atomic body"
        );
    }

    #[test]
    fn reopens_database_and_applies_changes_after_listener_restart() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let path = temporary_directory.child_path("restart.md");
        let options = IncrementalIndexOptions {
            stability_checks: 0,
            stability_delay: Duration::ZERO,
            max_retries: 0,
            retry_delay: Duration::ZERO,
            ..IncrementalIndexOptions::default()
        };

        fs::write(&path, "before restart").expect("写入重启前文件失败");
        apply_incremental_batch_with_retry(
            &database_path,
            &IncrementalBatch::Changes {
                changes: vec![IncrementalChange::Upsert { path: path.clone() }],
            },
            &options,
            &RescanControl::new(),
        )
        .expect("建立重启前索引失败");

        fs::write(&path, "after restart").expect("写入重启期间文件变化失败");
        let mut restarted_batcher = batcher(&temporary_directory.path);
        restarted_batcher
            .push(FileEvent::Modified { path: path.clone() }, Instant::now())
            .expect("加入重启后文件事件失败");
        apply_incremental_batch_with_retry(
            &database_path,
            &restarted_batcher.flush().expect("缺少重启后批次"),
            &options,
            &RescanControl::new(),
        )
        .expect("应用重启后文件变化失败");

        let connection = initialize_database(&database_path).expect("重新打开重启测试数据库失败");
        let document_id = document_id_for_path(&path).expect("生成重启后文档标识失败");
        assert_eq!(
            get_document(&connection, document_id.as_str())
                .expect("读取重启后文档失败")
                .expect("缺少重启后文档")
                .body,
            "after restart"
        );
    }

    #[test]
    fn cancellation_stops_before_writing_a_batch() {
        let temporary_directory = TemporaryDirectory::new();
        let control = RescanControl::new();
        control.cancel();
        let result = apply_incremental_batch_with_retry(
            temporary_directory.database_path(),
            &IncrementalBatch::Changes { changes: vec![] },
            &IncrementalIndexOptions::default(),
            &control,
        );
        assert!(matches!(
            result,
            Err(super::IncrementalIndexError::Cancelled)
        ));
    }

    #[test]
    fn exposes_default_summary_as_empty() {
        assert_eq!(IncrementalBatchSummary::default().changes_received, 0);
    }
}
