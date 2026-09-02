//! M4.0 文件变化判定。
//!
//! 本模块只比较两次文件状态快照，不负责监听文件系统、读取正文或写入数据库。
//! 这样变化判定可以先被独立验证，后续 M4.1 的事件来源和 M4.2 的批处理都可以
//! 复用同一套结果。

use std::{collections::BTreeMap, error::Error, fmt, path::PathBuf};

use nexus_db::FileMetadata;

/// 变化检测输入快照的来源侧。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotSide {
    /// 上一次已知的文件状态。
    Previous,
    /// 本次扫描得到的文件状态。
    Current,
}

/// 用于变化检测的最小文件状态快照。
///
/// 路径是文件身份；大小和修改时间是当前阶段用于判断内容是否需要重新处理的
/// 廉价信号。访问时间、创建时间和其他展示字段不会触发正文重新处理。修改时间
/// 缺失时，模块采取保守策略，将同大小文件判定为 `Modified`，避免把无法确认
/// 稳定性的文件错误地当成未变化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSnapshot {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub modified_at: Option<i64>,
}

impl FileSnapshot {
    /// 从 M1 文件扫描产生的元数据提取变化检测所需字段。
    pub fn from_metadata(metadata: &FileMetadata) -> Self {
        Self {
            path: metadata.path.clone(),
            size_bytes: metadata.size_bytes,
            modified_at: metadata.modified_at,
        }
    }
}

/// 两次快照之间的文件变化分类结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeSet {
    /// 当前快照中出现、上次快照中不存在的文件。
    pub added: Vec<FileSnapshot>,
    /// 两次快照都存在但廉价状态信号发生变化的文件。
    pub modified: Vec<FileSnapshot>,
    /// 两次快照都存在且具有明确相同大小和修改时间的文件。
    pub unchanged: Vec<FileSnapshot>,
    /// 上次快照中存在、本次快照中已经消失的文件。
    pub removed: Vec<FileSnapshot>,
}

impl ChangeSet {
    /// 返回需要后续重新处理的文件数量。
    ///
    /// 删除项不计入该数量；它们由后续增量持久化流程单独处理。
    pub fn files_needing_processing(&self) -> usize {
        self.added.len() + self.modified.len()
    }

    /// 返回本次变化判定覆盖的文件总数。
    pub fn total_files(&self) -> usize {
        self.added.len() + self.modified.len() + self.unchanged.len() + self.removed.len()
    }
}

/// 比较上次和本次文件快照。
///
/// 输入可以来自数据库中的历史元数据和当前扫描器的输出。函数不读取文件内容，
/// 不计算 hash，也不修改任何外部状态。结果按路径排序，确保批处理和测试具有
/// 稳定顺序。相同输入侧出现重复路径会返回错误，而不会静默覆盖其中一条状态。
pub fn detect_file_changes<P, C>(previous: P, current: C) -> Result<ChangeSet, ChangeDetectionError>
where
    P: IntoIterator<Item = FileSnapshot>,
    C: IntoIterator<Item = FileSnapshot>,
{
    let mut previous = collect_snapshots(previous, SnapshotSide::Previous)?;
    let current = collect_snapshots(current, SnapshotSide::Current)?;
    let mut changes = ChangeSet::default();

    for (path, current_snapshot) in current {
        match previous.remove(&path) {
            None => changes.added.push(current_snapshot),
            Some(previous_snapshot)
                if has_stable_signature(&previous_snapshot, &current_snapshot) =>
            {
                changes.unchanged.push(current_snapshot);
            }
            Some(_) => changes.modified.push(current_snapshot),
        }
    }

    changes.removed.extend(previous.into_values());
    Ok(changes)
}

fn collect_snapshots<I>(
    snapshots: I,
    side: SnapshotSide,
) -> Result<BTreeMap<PathBuf, FileSnapshot>, ChangeDetectionError>
where
    I: IntoIterator<Item = FileSnapshot>,
{
    let mut by_path = BTreeMap::new();
    for snapshot in snapshots {
        if snapshot.path.as_os_str().is_empty() {
            return Err(ChangeDetectionError::EmptyPath { side });
        }

        if by_path.insert(snapshot.path.clone(), snapshot).is_some() {
            return Err(ChangeDetectionError::DuplicatePath { side });
        }
    }

    Ok(by_path)
}

fn has_stable_signature(previous: &FileSnapshot, current: &FileSnapshot) -> bool {
    previous.size_bytes == current.size_bytes
        && matches!(
            (previous.modified_at, current.modified_at),
            (Some(previous), Some(current)) if previous == current
        )
}

/// 变化检测输入无效时返回的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeDetectionError {
    /// 输入中包含空路径。
    EmptyPath { side: SnapshotSide },
    /// 同一输入中同一路径出现多次。
    DuplicatePath { side: SnapshotSide },
}

impl ChangeDetectionError {
    /// 返回不包含路径或文件名的安全错误分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::EmptyPath { .. } => "change_detection_empty_path",
            Self::DuplicatePath { .. } => "change_detection_duplicate_path",
        }
    }

    /// 返回可以直接展示给用户的非敏感中文说明。
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::EmptyPath { .. } => "文件变化检测收到无效路径。",
            Self::DuplicatePath { .. } => "文件变化检测收到重复路径。",
        }
    }
}

impl fmt::Display for ChangeDetectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "文件变化检测失败: {}", self.kind())
    }
}

impl Error for ChangeDetectionError {}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use nexus_db::FileMetadata;

    use super::{detect_file_changes, ChangeDetectionError, FileSnapshot, SnapshotSide};

    fn snapshot(path: &str, size_bytes: u64, modified_at: Option<i64>) -> FileSnapshot {
        FileSnapshot {
            path: PathBuf::from(path),
            size_bytes,
            modified_at,
        }
    }

    #[test]
    fn classifies_added_modified_unchanged_and_removed_files() {
        let changes = detect_file_changes(
            vec![
                snapshot("a.txt", 10, Some(100)),
                snapshot("b.txt", 10, Some(100)),
                snapshot("removed.txt", 3, Some(100)),
            ],
            vec![
                snapshot("a.txt", 10, Some(100)),
                snapshot("b.txt", 11, Some(100)),
                snapshot("added.txt", 3, Some(100)),
            ],
        )
        .expect("变化检测失败");

        assert_eq!(changes.unchanged, vec![snapshot("a.txt", 10, Some(100))]);
        assert_eq!(changes.modified, vec![snapshot("b.txt", 11, Some(100))]);
        assert_eq!(changes.added, vec![snapshot("added.txt", 3, Some(100))]);
        assert_eq!(changes.removed, vec![snapshot("removed.txt", 3, Some(100))]);
        assert_eq!(changes.files_needing_processing(), 2);
        assert_eq!(changes.total_files(), 4);
    }

    #[test]
    fn sorts_each_change_category_by_path() {
        let changes = detect_file_changes(
            vec![snapshot("z.txt", 1, Some(1)), snapshot("b.txt", 1, Some(1))],
            vec![
                snapshot("y.txt", 1, Some(1)),
                snapshot("a.txt", 1, Some(2)),
                snapshot("z.txt", 1, Some(1)),
            ],
        )
        .expect("变化检测失败");

        assert_eq!(
            changes.added,
            vec![snapshot("a.txt", 1, Some(2)), snapshot("y.txt", 1, Some(1))]
        );
        assert_eq!(changes.unchanged, vec![snapshot("z.txt", 1, Some(1))]);
        assert_eq!(changes.removed, vec![snapshot("b.txt", 1, Some(1))]);
    }

    #[test]
    fn treats_missing_modified_time_as_modified() {
        let changes = detect_file_changes(
            vec![snapshot("unknown-time.txt", 8, None)],
            vec![snapshot("unknown-time.txt", 8, None)],
        )
        .expect("变化检测失败");

        assert_eq!(
            changes.modified,
            vec![snapshot("unknown-time.txt", 8, None)]
        );
        assert!(changes.unchanged.is_empty());
    }

    #[test]
    fn rejects_duplicate_paths_without_overwriting_input() {
        let error = detect_file_changes(
            vec![
                snapshot("duplicate.txt", 1, Some(1)),
                snapshot("duplicate.txt", 2, Some(2)),
            ],
            Vec::new(),
        )
        .expect_err("重复路径不应被静默覆盖");

        assert_eq!(
            error,
            ChangeDetectionError::DuplicatePath {
                side: SnapshotSide::Previous
            }
        );
        assert_eq!(error.kind(), "change_detection_duplicate_path");
        assert_eq!(error.user_message(), "文件变化检测收到重复路径。");
    }

    #[test]
    fn rejects_empty_paths() {
        let error = detect_file_changes(Vec::new(), vec![snapshot("", 1, Some(1))])
            .expect_err("空路径不应进入变化检测");

        assert_eq!(
            error,
            ChangeDetectionError::EmptyPath {
                side: SnapshotSide::Current
            }
        );
    }

    #[test]
    fn extracts_only_change_detection_fields_from_file_metadata() {
        let metadata = FileMetadata::from_path(
            PathBuf::from("notes.txt"),
            12,
            Some(42),
            Some(7),
            Some(99),
            Some("text/plain".to_owned()),
        )
        .expect("构造文件元数据失败");

        assert_eq!(
            FileSnapshot::from_metadata(&metadata),
            FileSnapshot {
                path: metadata.path,
                size_bytes: 12,
                modified_at: Some(42),
            }
        );
    }
}
