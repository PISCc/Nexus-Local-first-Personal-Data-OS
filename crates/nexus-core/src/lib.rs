//! Nexus 本地优先核心边界。
//!
//! 核心层负责组织本地数据基础设施、文件流式扫描和统一文档模型，但不依赖
//! Tauri 或前端。平台层只负责提供路径、记录安全状态，并把结果传给界面。

#![forbid(unsafe_code)]

use std::{
    error::Error,
    fmt,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use nexus_db::{
    begin_file_metadata_rescan, initialize_database, normalize_path, DatabaseError,
    FileMetadataError,
};

mod document;
mod scanner;

pub use document::{Document, DocumentError, DocumentId, DocumentLocation, DocumentSource};
pub use scanner::{
    scan_directory, FileScanner, ScanError, ScanItem, ScanOptions, ScanStartError, SkipReason,
};

/// 初始化 Nexus 本地核心。
///
/// 数据库连接在本次启动检查结束后由调用方释放；后续里程碑再决定运行时
/// 数据库服务的持有方式。这里保留核心层的初始化边界，避免 UI 直接操作数据库。
pub fn initialize<P: AsRef<Path>>(database_path: P) -> Result<(), CoreError> {
    let _connection = initialize_database(database_path).map_err(CoreError::Database)?;
    Ok(())
}

/// 一次手动重扫的结果统计。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RescanSummary {
    pub files_succeeded: usize,
    pub files_failed: usize,
    pub paths_skipped: usize,
    pub records_removed: usize,
    pub batches_committed: usize,
}

/// 可在线程之间共享的手动重扫取消控制器。
#[derive(Clone, Debug)]
pub struct RescanControl {
    cancelled: Arc<AtomicBool>,
}

impl RescanControl {
    /// 创建一个未取消的控制器。
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 请求当前重扫在下一个安全检查点停止。
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// 返回是否已经收到取消请求。
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

impl Default for RescanControl {
    fn default() -> Self {
        Self::new()
    }
}

/// 手动重扫过程中的确定性进度快照。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RescanProgress {
    pub processed: usize,
    pub files_succeeded: usize,
    pub files_failed: usize,
    pub paths_skipped: usize,
}

/// 从指定目录开始一次不依赖 watcher 的手动重扫。
///
/// `database_path` 和 `root` 都由调用方提供；扫描器逐条产出结果，数据库边界
/// 以有界批次写入，并在扫描完成后移除本次确认不存在的旧记录。失败或跳过的
/// 路径及其子路径会被保护，不会因为一次不完整的扫描而误删已有记录。
pub fn rescan_directory<D, R>(
    database_path: D,
    root: R,
    options: ScanOptions,
    batch_size: usize,
) -> Result<RescanSummary, RescanError>
where
    D: AsRef<Path>,
    R: AsRef<Path>,
{
    rescan_directory_with_control(
        database_path,
        root,
        options,
        batch_size,
        RescanControl::default(),
        |_| {},
    )
}

/// 执行一次支持取消和进度回调的手动重扫。
pub fn rescan_directory_with_control<D, R, F>(
    database_path: D,
    root: R,
    options: ScanOptions,
    batch_size: usize,
    control: RescanControl,
    mut on_progress: F,
) -> Result<RescanSummary, RescanError>
where
    D: AsRef<Path>,
    R: AsRef<Path>,
    F: FnMut(RescanProgress),
{
    if batch_size == 0 {
        return Err(RescanError::Persistence(
            FileMetadataError::InvalidBatchSize,
        ));
    }

    let root =
        normalize_path(root.as_ref()).map_err(|source| RescanError::InvalidRoot { source })?;
    let ignored_paths = options
        .ignored_paths
        .iter()
        .map(|path| {
            normalize_path(path).map_err(|source| RescanError::InvalidIgnoredPath { source })
        })
        .collect::<Result<Vec<_>, _>>()?;

    if ignored_paths
        .iter()
        .any(|ignored| root == *ignored || root.starts_with(ignored))
    {
        return Ok(RescanSummary::default());
    }

    let scanner = scan_directory(&root, options).map_err(RescanError::ScanStart)?;
    let mut connection = initialize_database(database_path).map_err(RescanError::Database)?;
    let mut persistence = begin_file_metadata_rescan(&mut connection, &root, batch_size)
        .map_err(RescanError::Persistence)?;
    let mut progress = RescanProgress::default();

    for item in scanner {
        if control.is_cancelled() {
            return Err(RescanError::Cancelled);
        }

        match item {
            ScanItem::File(metadata) => persistence
                .record_file(metadata)
                .map_err(RescanError::Persistence)
                .map(|()| progress.files_succeeded += 1)?,
            ScanItem::Skipped { path, .. } => persistence
                .record_skip(path)
                .map_err(RescanError::Persistence)
                .map(|()| progress.paths_skipped += 1)?,
            ScanItem::Failed { path, .. } => persistence
                .record_failure(path)
                .map_err(RescanError::Persistence)
                .map(|()| progress.files_failed += 1)?,
        }

        progress.processed += 1;
        on_progress(progress);
    }

    if control.is_cancelled() {
        return Err(RescanError::Cancelled);
    }

    persistence
        .finish()
        .map(|summary| RescanSummary {
            files_succeeded: summary.files_succeeded,
            files_failed: summary.files_failed,
            paths_skipped: summary.paths_skipped,
            records_removed: summary.records_removed,
            batches_committed: summary.batches_committed,
        })
        .map_err(RescanError::Persistence)
}

/// 手动重扫错误。
#[derive(Debug)]
pub enum RescanError {
    InvalidRoot { source: FileMetadataError },
    InvalidIgnoredPath { source: FileMetadataError },
    ScanStart(ScanStartError),
    Database(DatabaseError),
    Persistence(FileMetadataError),
    Cancelled,
}

impl RescanError {
    /// 返回不包含路径、文件名或原始错误内容的安全分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidRoot { .. } => "rescan_root_invalid",
            Self::InvalidIgnoredPath { .. } => "rescan_ignored_path_invalid",
            Self::ScanStart(error) => error.kind(),
            Self::Database(error) => error.kind(),
            Self::Persistence(error) => error.kind(),
            Self::Cancelled => "rescan_cancelled",
        }
    }

    /// 返回可以直接展示给用户的非敏感中文说明。
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::InvalidRoot { .. } | Self::InvalidIgnoredPath { .. } => "扫描路径无效。",
            Self::ScanStart(_) => "无法开始手动重扫。",
            Self::Database(_) => "本地数据存储暂时不可用。",
            Self::Persistence(_) => "无法保存手动重扫结果。",
            Self::Cancelled => "手动重扫已取消。",
        }
    }
}

impl fmt::Display for RescanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "手动重扫失败: {}", self.kind())
    }
}

impl Error for RescanError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRoot { source } | Self::InvalidIgnoredPath { source } => Some(source),
            Self::ScanStart(source) => Some(source),
            Self::Database(source) => Some(source),
            Self::Persistence(source) => Some(source),
            Self::Cancelled => None,
        }
    }
}

/// 核心初始化错误。
#[derive(Debug)]
pub enum CoreError {
    /// 本地数据库无法打开、读取或迁移。
    Database(DatabaseError),
}

impl CoreError {
    /// 返回不携带用户路径、内容或原始错误的安全分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Database(error) => error.kind(),
        }
    }

    /// 返回可以直接展示给用户的非敏感中文说明。
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::Database(
                DatabaseError::InvalidSchemaVersion { .. }
                | DatabaseError::UnsupportedSchemaVersion { .. },
            ) => "本地数据存储版本不受支持。",
            Self::Database(_) => "本地数据存储暂时不可用。",
        }
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "核心初始化失败: {}", self.kind())
    }
}

impl Error for CoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(source) => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::{Path, PathBuf},
        process,
        sync::atomic::{AtomicUsize, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use nexus_db::{get_file_metadata, initialize_database, DatabaseError};

    use super::{
        initialize, rescan_directory, rescan_directory_with_control, CoreError, RescanControl,
        RescanError, ScanOptions,
    };

    static TEMP_DIRECTORY_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TemporaryDirectory {
        path: PathBuf,
    }

    impl TemporaryDirectory {
        fn new() -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let counter = TEMP_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);

            for attempt in 0..100 {
                let path = env::temp_dir().join(format!(
                    "nexus-core-test-{}-{timestamp}-{counter}-{attempt}",
                    process::id()
                ));

                match fs::create_dir(&path) {
                    Ok(()) => return Self { path },
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("创建核心测试临时目录失败: {error}"),
                }
            }

            panic!("无法创建唯一核心测试临时目录")
        }

        fn database_path(&self) -> PathBuf {
            self.path.join("nexus.sqlite3")
        }

        fn child_path(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_file(path: &Path, contents: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("创建重扫测试父目录失败");
        }
        fs::write(path, contents).expect("写入重扫测试文件失败");
    }

    #[test]
    fn initializes_core_against_local_database() {
        let temporary_directory = TemporaryDirectory::new();

        initialize(temporary_directory.database_path()).expect("核心初始化失败");
    }

    #[test]
    fn maps_database_failure_to_safe_user_message() {
        let temporary_directory = TemporaryDirectory::new();
        let blocker_path = temporary_directory.child_path("not-a-directory");
        fs::write(&blocker_path, b"not a directory").expect("创建路径阻断文件失败");
        let database_path = blocker_path.join("nexus.sqlite3");
        let sensitive_path = database_path.display().to_string();

        let error = initialize(&database_path).expect_err("不可访问数据库不应初始化成功");

        assert!(matches!(
            &error,
            CoreError::Database(DatabaseError::Open { .. })
        ));
        assert_eq!(error.kind(), "database_open");
        assert_eq!(error.user_message(), "本地数据存储暂时不可用。");
        assert!(!error.to_string().contains(&sensitive_path));
    }

    #[test]
    fn manually_rescans_and_reconciles_added_updated_and_removed_files() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let root = temporary_directory.child_path("library");
        let sibling_root = temporary_directory.child_path("library-old");
        let kept_file = root.join("kept.txt");
        let removed_file = root.join("removed.txt");
        let sibling_file = sibling_root.join("must-stay.txt");
        write_file(&kept_file, b"keep");
        write_file(&removed_file, b"remove");
        write_file(&sibling_file, b"sibling");

        let first = rescan_directory(&database_path, &root, ScanOptions::default(), 2)
            .expect("首次手动重扫失败");
        assert_eq!(first.files_succeeded, 2);
        assert_eq!(first.files_failed, 0);
        assert_eq!(first.paths_skipped, 0);
        assert_eq!(first.records_removed, 0);
        assert_eq!(first.batches_committed, 1);

        rescan_directory(&database_path, &sibling_root, ScanOptions::default(), 2)
            .expect("建立相邻目录测试索引失败");

        write_file(&kept_file, b"updated");
        fs::remove_file(&removed_file).expect("删除重扫测试文件失败");
        let added_file = root.join("added.md");
        write_file(&added_file, b"new");

        let second = rescan_directory(&database_path, &root, ScanOptions::default(), 2)
            .expect("第二次手动重扫失败");
        assert_eq!(second.files_succeeded, 2);
        assert_eq!(second.files_failed, 0);
        assert_eq!(second.paths_skipped, 0);
        assert_eq!(second.records_removed, 1);
        assert_eq!(second.batches_committed, 1);

        let connection = initialize_database(&database_path).expect("打开重扫结果数据库失败");
        let loaded_kept = get_file_metadata(&connection, &kept_file)
            .expect("读取更新文件失败")
            .expect("找不到更新文件");
        assert_eq!(loaded_kept.size_bytes, 7);
        assert!(get_file_metadata(&connection, &removed_file)
            .expect("读取已删除文件失败")
            .is_none());
        assert!(get_file_metadata(&connection, &added_file)
            .expect("读取新增文件失败")
            .is_some());
        assert!(get_file_metadata(&connection, &sibling_file)
            .expect("读取相邻目录文件失败")
            .is_some());

        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM file_metadata", [], |row| row.get(0))
            .expect("统计重扫结果失败");
        assert_eq!(count, 3);
    }

    #[test]
    fn preserves_records_under_ignored_paths_during_rescan() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let root = temporary_directory.child_path("library");
        let kept_file = root.join("kept.txt");
        let ignored_directory = root.join("ignored");
        let ignored_file = ignored_directory.join("old.txt");
        write_file(&kept_file, b"keep");
        write_file(&ignored_file, b"old");

        rescan_directory(&database_path, &root, ScanOptions::default(), 2)
            .expect("建立忽略路径测试初始索引失败");

        let options = ScanOptions {
            ignored_paths: vec![ignored_directory],
            follow_symlinks: false,
        };
        let summary =
            rescan_directory(&database_path, &root, options, 2).expect("带忽略路径的手动重扫失败");

        assert_eq!(summary.files_succeeded, 1);
        assert_eq!(summary.files_failed, 0);
        assert_eq!(summary.paths_skipped, 1);
        assert_eq!(summary.records_removed, 0);

        let connection = initialize_database(&database_path).expect("打开忽略路径测试数据库失败");
        assert!(get_file_metadata(&connection, &ignored_file)
            .expect("读取忽略路径文件失败")
            .is_some());
    }

    #[test]
    fn rejects_zero_batch_size_before_starting_rescan() {
        let temporary_directory = TemporaryDirectory::new();
        let error = rescan_directory(
            temporary_directory.database_path(),
            temporary_directory.child_path("library"),
            ScanOptions::default(),
            0,
        )
        .expect_err("零批次大小不应开始手动重扫");

        assert!(matches!(error, RescanError::Persistence(_)));
        assert_eq!(error.kind(), "file_metadata_batch_size_invalid");
        assert_eq!(error.user_message(), "无法保存手动重扫结果。");
    }

    #[test]
    fn cancelling_rescan_keeps_persistent_data_unchanged() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let root = temporary_directory.child_path("library");
        let existing_file = root.join("existing.txt");
        let added_file = root.join("added.txt");
        write_file(&existing_file, b"existing");

        rescan_directory(&database_path, &root, ScanOptions::default(), 1)
            .expect("建立取消测试初始索引失败");
        write_file(&added_file, b"added");

        let control = RescanControl::new();
        let callback_control = control.clone();
        let error = rescan_directory_with_control(
            &database_path,
            &root,
            ScanOptions::default(),
            1,
            control,
            |progress| {
                if progress.processed >= 1 {
                    callback_control.cancel();
                }
            },
        )
        .expect_err("收到取消请求后重扫不应报告成功");

        assert!(matches!(error, RescanError::Cancelled));
        assert_eq!(error.kind(), "rescan_cancelled");
        assert_eq!(error.user_message(), "手动重扫已取消。");

        let connection = initialize_database(&database_path).expect("打开取消测试数据库失败");
        assert!(get_file_metadata(&connection, &existing_file)
            .expect("读取取消测试既有记录失败")
            .is_some());
        assert!(get_file_metadata(&connection, &added_file)
            .expect("读取取消测试新增记录失败")
            .is_none());
    }

    #[cfg(unix)]
    #[test]
    fn preserves_a_failed_symlink_record_and_removes_confirmed_missing_file() {
        use std::os::unix::fs::symlink;

        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let root = temporary_directory.child_path("library");
        fs::create_dir_all(&root).expect("创建符号链接重扫根目录失败");
        let target_file = root.join("target.txt");
        let link_file = root.join("link.txt");
        write_file(&target_file, b"target");
        symlink(&target_file, &link_file).expect("创建重扫测试符号链接失败");

        let options = ScanOptions {
            ignored_paths: Vec::new(),
            follow_symlinks: true,
        };
        let first = rescan_directory(&database_path, &root, options.clone(), 2)
            .expect("首次符号链接重扫失败");
        assert_eq!(first.files_succeeded, 2);

        fs::remove_file(&target_file).expect("删除符号链接目标失败");
        let second = rescan_directory(&database_path, &root, options, 2)
            .expect("目标删除后的符号链接重扫不应全局失败");

        assert!(second.files_failed >= 1);
        assert_eq!(second.records_removed, 1);
        let connection = initialize_database(&database_path).expect("打开符号链接测试数据库失败");
        assert!(get_file_metadata(&connection, &target_file)
            .expect("读取已确认删除的目标文件失败")
            .is_none());
        assert!(get_file_metadata(&connection, &link_file)
            .expect("读取失败符号链接记录失败")
            .is_some());
    }
}
