//! 本地文件的流式递归遍历。

use std::{
    collections::HashSet,
    error::Error,
    fmt,
    fs::{self, DirEntry, ReadDir},
    io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use nexus_db::{normalize_path, FileMetadata, FileMetadataError};

/// 文件扫描选项。
#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    /// 完全跳过这些路径及其子路径。路径会按与扫描根目录相同的规则绝对化。
    pub ignored_paths: Vec<PathBuf>,
    /// 是否明确允许跟随符号链接。默认关闭。
    pub follow_symlinks: bool,
}

/// 扫描过程中可以产出的跳过原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// 路径命中了忽略配置。
    IgnoredPath,
    /// 默认不跟随符号链接。
    SymbolicLink,
    /// 跟随符号链接时，目录目标已经遍历过。
    AlreadyVisitedDirectory,
    /// 路径不是普通文件或目录，例如设备文件。
    UnsupportedFileType,
}

/// 扫描器逐条产出的结果。
#[derive(Debug)]
pub enum ScanItem {
    /// 一条可以交给后续持久化流程的文件元数据。
    File(FileMetadata),
    /// 一个被明确跳过的路径。
    Skipped { path: PathBuf, reason: SkipReason },
    /// 一个失败但不会终止全局扫描的路径或目录操作。
    Failed { path: PathBuf, error: ScanError },
}

impl ScanItem {
    /// 返回当前结果对应的路径。
    pub fn path(&self) -> &Path {
        match self {
            Self::File(metadata) => &metadata.path,
            Self::Skipped { path, .. } | Self::Failed { path, .. } => path,
        }
    }
}

/// 扫描已经开始后，单个路径操作可能产生的错误。
#[derive(Debug)]
pub enum ScanError {
    ReadDirectory { source: io::Error },
    ReadEntryType { source: io::Error },
    ReadEntryMetadata { source: io::Error },
    OpenDirectory { source: io::Error },
    ResolveDirectory { source: io::Error },
    BuildMetadata { source: FileMetadataError },
}

impl ScanError {
    /// 返回不包含路径、文件名或原始错误内容的安全错误分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ReadDirectory { .. } => "scan_read_directory",
            Self::ReadEntryType { .. } => "scan_read_entry_type",
            Self::ReadEntryMetadata { .. } => "scan_read_entry_metadata",
            Self::OpenDirectory { .. } => "scan_open_directory",
            Self::ResolveDirectory { .. } => "scan_resolve_directory",
            Self::BuildMetadata { .. } => "scan_build_metadata",
        }
    }
}

impl fmt::Display for ScanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ReadDirectory { .. } => "读取目录内容失败。",
            Self::ReadEntryType { .. } => "读取路径类型失败。",
            Self::ReadEntryMetadata { .. } => "读取文件元数据失败。",
            Self::OpenDirectory { .. } => "打开子目录失败。",
            Self::ResolveDirectory { .. } => "解析目录身份失败。",
            Self::BuildMetadata { .. } => "生成文件元数据失败。",
        };

        write!(formatter, "{message}")
    }
}

impl Error for ScanError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReadDirectory { source }
            | Self::ReadEntryType { source }
            | Self::ReadEntryMetadata { source }
            | Self::OpenDirectory { source }
            | Self::ResolveDirectory { source } => Some(source),
            Self::BuildMetadata { source } => Some(source),
        }
    }
}

/// 扫描器启动前的错误。
#[derive(Debug)]
pub enum ScanStartError {
    InvalidRoot { source: FileMetadataError },
    InvalidIgnoredPath { source: FileMetadataError },
    RootIsSymlink,
    OpenRoot { source: io::Error },
    ResolveRoot { source: io::Error },
}

impl ScanStartError {
    /// 返回不包含路径或原始错误内容的安全错误分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidRoot { .. } => "scan_root_invalid",
            Self::InvalidIgnoredPath { .. } => "scan_ignored_path_invalid",
            Self::RootIsSymlink => "scan_root_symlink",
            Self::OpenRoot { .. } => "scan_open_root",
            Self::ResolveRoot { .. } => "scan_resolve_root",
        }
    }
}

impl fmt::Display for ScanStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidRoot { .. } => "扫描根目录无效。",
            Self::InvalidIgnoredPath { .. } => "忽略路径无效。",
            Self::RootIsSymlink => "默认不跟随符号链接作为扫描根目录。",
            Self::OpenRoot { .. } => "无法打开扫描根目录。",
            Self::ResolveRoot { .. } => "无法解析扫描根目录。",
        };

        write!(formatter, "{message}")
    }
}

impl Error for ScanStartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRoot { source } | Self::InvalidIgnoredPath { source } => Some(source),
            Self::OpenRoot { source } | Self::ResolveRoot { source } => Some(source),
            Self::RootIsSymlink => None,
        }
    }
}

struct DirectoryFrame {
    path: PathBuf,
    entries: ReadDir,
}

/// 本地文件流式扫描器。
///
/// 扫描器实现 `Iterator`，每次只读取一个目录项或产出一个结果，不会把所有路径
/// 累积到内存中。扫描开始后，单个目录项的异常会作为 `ScanItem::Failed` 产出，
/// 迭代器随后继续处理其他路径。
pub struct FileScanner {
    frames: Vec<DirectoryFrame>,
    ignored_paths: Vec<PathBuf>,
    follow_symlinks: bool,
    visited_directories: HashSet<PathBuf>,
}

/// 创建一个从指定根目录开始的文件扫描器。
pub fn scan_directory<P: AsRef<Path>>(
    root: P,
    options: ScanOptions,
) -> Result<FileScanner, ScanStartError> {
    FileScanner::new(root.as_ref(), options)
}

impl FileScanner {
    /// 创建一个文件扫描器。
    pub fn new(root: &Path, options: ScanOptions) -> Result<Self, ScanStartError> {
        let root = normalize_path(root).map_err(|source| ScanStartError::InvalidRoot { source })?;
        let ignored_paths = options
            .ignored_paths
            .into_iter()
            .map(|path| {
                normalize_path(&path)
                    .map_err(|source| ScanStartError::InvalidIgnoredPath { source })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut scanner = Self {
            frames: Vec::new(),
            ignored_paths,
            follow_symlinks: options.follow_symlinks,
            visited_directories: HashSet::new(),
        };

        if scanner.is_ignored(&root) {
            return Ok(scanner);
        }

        let root_metadata =
            fs::symlink_metadata(&root).map_err(|source| ScanStartError::OpenRoot { source })?;
        if root_metadata.file_type().is_symlink() && !scanner.follow_symlinks {
            return Err(ScanStartError::RootIsSymlink);
        }

        if scanner.follow_symlinks {
            let identity =
                fs::canonicalize(&root).map_err(|source| ScanStartError::ResolveRoot { source })?;
            scanner.visited_directories.insert(identity);
        }

        let entries = fs::read_dir(&root).map_err(|source| ScanStartError::OpenRoot { source })?;
        scanner.frames.push(DirectoryFrame {
            path: root,
            entries,
        });
        Ok(scanner)
    }

    fn is_ignored(&self, path: &Path) -> bool {
        self.ignored_paths
            .iter()
            .any(|ignored| path == ignored || path.starts_with(ignored))
    }

    fn read_entry(&mut self, entry: DirEntry) -> Option<ScanItem> {
        let path = entry.path();

        if self.is_ignored(&path) {
            return Some(ScanItem::Skipped {
                path,
                reason: SkipReason::IgnoredPath,
            });
        }

        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(source) => {
                return Some(ScanItem::Failed {
                    path,
                    error: ScanError::ReadEntryType { source },
                });
            }
        };

        if file_type.is_symlink() && !self.follow_symlinks {
            return Some(ScanItem::Skipped {
                path,
                reason: SkipReason::SymbolicLink,
            });
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(source) => {
                return Some(ScanItem::Failed {
                    path,
                    error: ScanError::ReadEntryMetadata { source },
                });
            }
        };

        if metadata.is_dir() {
            if self.follow_symlinks {
                let identity = match fs::canonicalize(&path) {
                    Ok(identity) => identity,
                    Err(source) => {
                        return Some(ScanItem::Failed {
                            path,
                            error: ScanError::ResolveDirectory { source },
                        });
                    }
                };

                if !self.visited_directories.insert(identity) {
                    return Some(ScanItem::Skipped {
                        path,
                        reason: SkipReason::AlreadyVisitedDirectory,
                    });
                }
            }

            let entries = match fs::read_dir(&path) {
                Ok(entries) => entries,
                Err(source) => {
                    return Some(ScanItem::Failed {
                        path,
                        error: ScanError::OpenDirectory { source },
                    });
                }
            };

            self.frames.push(DirectoryFrame { path, entries });
            return None;
        }

        if !metadata.is_file() {
            return Some(ScanItem::Skipped {
                path,
                reason: SkipReason::UnsupportedFileType,
            });
        }

        let file_metadata = FileMetadata::from_path(
            path.clone(),
            metadata.len(),
            timestamp_millis(metadata.modified()),
            timestamp_millis(metadata.created()),
            timestamp_millis(metadata.accessed()),
            None,
        );

        match file_metadata {
            Ok(file_metadata) => Some(ScanItem::File(file_metadata)),
            Err(source) => Some(ScanItem::Failed {
                path,
                error: ScanError::BuildMetadata { source },
            }),
        }
    }
}

impl Iterator for FileScanner {
    type Item = ScanItem;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (directory_path, entry) = {
                let frame = self.frames.last_mut()?;
                (frame.path.clone(), frame.entries.next())
            };

            match entry {
                None => {
                    self.frames.pop();
                }
                Some(Err(source)) => {
                    return Some(ScanItem::Failed {
                        path: directory_path,
                        error: ScanError::ReadDirectory { source },
                    });
                }
                Some(Ok(entry)) => {
                    if let Some(item) = self.read_entry(entry) {
                        return Some(item);
                    }
                }
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::{Path, PathBuf},
        process,
        sync::atomic::{AtomicUsize, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{scan_directory, ScanItem, ScanOptions, ScanStartError, SkipReason};

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
                    "nexus-scanner-test-{}-{timestamp}-{counter}-{attempt}",
                    process::id()
                ));

                match fs::create_dir(&path) {
                    Ok(()) => return Self { path },
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("创建扫描器测试临时目录失败: {error}"),
                }
            }

            panic!("无法创建唯一扫描器测试临时目录")
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
            fs::create_dir_all(parent).expect("创建扫描器测试父目录失败");
        }
        fs::write(path, contents).expect("写入扫描器测试文件失败");
    }

    #[test]
    fn scans_nested_files_as_streamed_items() {
        let temporary_directory = TemporaryDirectory::new();
        let first_path = temporary_directory.child_path("first.txt");
        let second_path = temporary_directory.child_path("nested/second.md");
        write_file(&first_path, b"first");
        write_file(&second_path, b"second");

        let scanner = scan_directory(&temporary_directory.path, ScanOptions::default())
            .expect("创建扫描器失败");
        let items: Vec<_> = scanner.collect();

        let file_paths: Vec<_> = items
            .iter()
            .filter_map(|item| match item {
                ScanItem::File(metadata) => Some(metadata.path.clone()),
                ScanItem::Skipped { .. } | ScanItem::Failed { .. } => None,
            })
            .collect();
        assert_eq!(file_paths.len(), 2);
        assert!(file_paths.contains(&first_path));
        assert!(file_paths.contains(&second_path));
    }

    #[test]
    fn ignores_configured_path_and_descendants() {
        let temporary_directory = TemporaryDirectory::new();
        let kept_path = temporary_directory.child_path("kept.txt");
        let ignored_path = temporary_directory.child_path("ignored/nested.txt");
        write_file(&kept_path, b"kept");
        write_file(&ignored_path, b"ignored");

        let options = ScanOptions {
            ignored_paths: vec![temporary_directory.child_path("ignored")],
            follow_symlinks: false,
        };
        let scanner = scan_directory(&temporary_directory.path, options).expect("创建扫描器失败");
        let items: Vec<_> = scanner.collect();

        assert!(items.iter().any(|item| {
            matches!(
                item,
                ScanItem::Skipped {
                    reason: SkipReason::IgnoredPath,
                    ..
                }
            )
        }));
        assert!(items.iter().any(|item| {
            matches!(item, ScanItem::File(metadata) if metadata.path == kept_path)
        }));
        assert!(!items.iter().any(|item| item.path() == ignored_path));
    }

    #[test]
    fn returns_error_for_inaccessible_root_without_panicking() {
        let temporary_directory = TemporaryDirectory::new();
        let root_file = temporary_directory.child_path("not-a-directory.txt");
        write_file(&root_file, b"not a directory");

        match scan_directory(&root_file, ScanOptions::default()) {
            Err(ScanStartError::OpenRoot { .. }) => {}
            Err(error) => panic!("返回了错误类型: {error}"),
            Ok(_) => panic!("文件路径不应作为扫描根目录成功打开"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn continues_after_an_inaccessible_directory() {
        use std::os::unix::fs::PermissionsExt;

        let temporary_directory = TemporaryDirectory::new();
        let blocked_directory = temporary_directory.child_path("blocked");
        let readable_file = temporary_directory.child_path("readable.txt");
        fs::create_dir(&blocked_directory).expect("创建不可访问测试目录失败");
        write_file(&blocked_directory.join("hidden.txt"), b"hidden");
        write_file(&readable_file, b"readable");

        let mut permissions = fs::metadata(&blocked_directory)
            .expect("读取不可访问测试目录权限失败")
            .permissions();
        permissions.set_mode(0o000);
        fs::set_permissions(&blocked_directory, permissions).expect("设置不可访问测试目录权限失败");

        let inaccessible = match fs::read_dir(&blocked_directory) {
            Ok(mut entries) => entries.next().is_some_and(|entry| entry.is_err()),
            Err(_) => true,
        };
        if !inaccessible {
            let mut permissions = fs::metadata(&blocked_directory)
                .expect("恢复测试目录权限前读取元数据失败")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&blocked_directory, permissions).expect("恢复测试目录权限失败");
            return;
        }

        let scanner = scan_directory(&temporary_directory.path, ScanOptions::default())
            .expect("创建权限失败扫描器失败");
        let items: Vec<_> = scanner.collect();

        let mut permissions = fs::metadata(&blocked_directory)
            .expect("扫描后读取测试目录权限失败")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&blocked_directory, permissions).expect("扫描后恢复测试目录权限失败");

        assert!(items.iter().any(|item| {
            matches!(item, ScanItem::Failed { path, .. } if path == &blocked_directory)
        }));
        assert!(items.iter().any(|item| {
            matches!(item, ScanItem::File(metadata) if metadata.path == readable_file)
        }));
    }

    #[cfg(unix)]
    #[test]
    fn skips_symbolic_links_by_default() {
        use std::os::unix::fs::symlink;

        let temporary_directory = TemporaryDirectory::new();
        let target_directory = temporary_directory.child_path("target");
        let link_directory = temporary_directory.child_path("link");
        let target_file = target_directory.join("target.txt");
        fs::create_dir(&target_directory).expect("创建符号链接目标目录失败");
        write_file(&target_file, b"target");
        symlink(&target_directory, &link_directory).expect("创建符号链接失败");

        let scanner = scan_directory(&temporary_directory.path, ScanOptions::default())
            .expect("创建扫描器失败");
        let items: Vec<_> = scanner.collect();

        assert!(items.iter().any(|item| {
            matches!(
                item,
                ScanItem::Skipped {
                    path,
                    reason: SkipReason::SymbolicLink
                } if path == &link_directory
            )
        }));
        assert!(!items
            .iter()
            .any(|item| item.path() == link_directory.join("target.txt")));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symbolic_link_as_root_by_default() {
        use std::os::unix::fs::symlink;

        let temporary_directory = TemporaryDirectory::new();
        let target_directory = temporary_directory.child_path("target");
        let link_directory = temporary_directory.child_path("link");
        fs::create_dir(&target_directory).expect("创建符号链接目标目录失败");
        symlink(&target_directory, &link_directory).expect("创建符号链接失败");

        match scan_directory(&link_directory, ScanOptions::default()) {
            Err(ScanStartError::RootIsSymlink) => {}
            Err(error) => panic!("返回了错误类型: {error}"),
            Ok(_) => panic!("默认不应跟随符号链接根目录"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn follows_symbolic_links_only_when_enabled() {
        use std::os::unix::fs::symlink;

        let temporary_directory = TemporaryDirectory::new();
        let root_directory = temporary_directory.child_path("root");
        let target_directory = temporary_directory.child_path("target");
        let link_directory = root_directory.join("link");
        let linked_file = link_directory.join("linked.txt");
        fs::create_dir(&root_directory).expect("创建扫描根目录失败");
        fs::create_dir(&target_directory).expect("创建符号链接目标目录失败");
        write_file(&target_directory.join("linked.txt"), b"linked");
        symlink(&target_directory, &link_directory).expect("创建符号链接失败");

        let options = ScanOptions {
            ignored_paths: Vec::new(),
            follow_symlinks: true,
        };
        let scanner = scan_directory(&root_directory, options).expect("创建扫描器失败");
        let items: Vec<_> = scanner.collect();

        assert!(items.iter().any(|item| {
            matches!(item, ScanItem::File(metadata) if metadata.path == linked_file)
        }));
    }

    #[cfg(unix)]
    #[test]
    fn continues_after_broken_symbolic_link() {
        use std::os::unix::fs::symlink;

        let temporary_directory = TemporaryDirectory::new();
        let broken_link = temporary_directory.child_path("broken-link");
        let kept_file = temporary_directory.child_path("kept.txt");
        symlink("missing-target", &broken_link).expect("创建损坏符号链接失败");
        write_file(&kept_file, b"kept");

        let options = ScanOptions {
            ignored_paths: Vec::new(),
            follow_symlinks: true,
        };
        let scanner = scan_directory(&temporary_directory.path, options).expect("创建扫描器失败");
        let items: Vec<_> = scanner.collect();

        assert!(items
            .iter()
            .any(|item| matches!(item, ScanItem::Failed { .. })));
        assert!(items.iter().any(|item| {
            matches!(item, ScanItem::File(metadata) if metadata.path == kept_file)
        }));
    }

    #[cfg(unix)]
    #[test]
    fn scans_non_utf8_path_without_panicking() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let temporary_directory = TemporaryDirectory::new();
        let file_name = OsString::from_vec(vec![b'n', b'o', b't', b'e', 0xff]);
        let file_path = temporary_directory.path.join(&file_name);
        write_file(&file_path, b"non-utf8");

        let scanner = scan_directory(&temporary_directory.path, ScanOptions::default())
            .expect("创建扫描器失败");
        let items: Vec<_> = scanner.collect();

        assert!(items.iter().any(|item| {
            matches!(item, ScanItem::File(metadata) if metadata.path == file_path)
        }));
    }
}
