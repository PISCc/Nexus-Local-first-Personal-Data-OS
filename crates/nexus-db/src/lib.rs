//! Nexus 本地数据库边界。
//!
//! M1.0–M1.2 在本地数据库边界提供文件元数据模型、单条 upsert 和批量持久化入口。

#![forbid(unsafe_code)]

use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

use rusqlite::{params, Connection, OptionalExtension};

const FOUNDATION_MIGRATION_SQL: &str = include_str!("../migrations/0001_foundation.sql");
const FILE_METADATA_MIGRATION_SQL: &str = include_str!("../migrations/0002_file_metadata.sql");
const STAGED_FILE_METADATA_SQL: &str = "INSERT INTO temp.nexus_scan_pending (
    path_key,
    path_display,
    file_name,
    extension,
    size_bytes,
    modified_at,
    created_at,
    accessed_at,
    file_type
)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
ON CONFLICT(path_key) DO UPDATE SET
    path_display = excluded.path_display,
    file_name = excluded.file_name,
    extension = excluded.extension,
    size_bytes = excluded.size_bytes,
    modified_at = excluded.modified_at,
    created_at = excluded.created_at,
    accessed_at = excluded.accessed_at,
    file_type = excluded.file_type";
const APPLY_STAGED_FILE_METADATA_SQL: &str = "INSERT INTO file_metadata (
    path_key,
    path_display,
    file_name,
    extension,
    size_bytes,
    modified_at,
    created_at,
    accessed_at,
    file_type
)
SELECT
    path_key,
    path_display,
    file_name,
    extension,
    size_bytes,
    modified_at,
    created_at,
    accessed_at,
    file_type
FROM temp.nexus_scan_pending
WHERE 1
ON CONFLICT(path_key) DO UPDATE SET
    path_display = excluded.path_display,
    file_name = excluded.file_name,
    extension = excluded.extension,
    size_bytes = excluded.size_bytes,
    modified_at = excluded.modified_at,
    created_at = excluded.created_at,
    accessed_at = excluded.accessed_at,
    file_type = excluded.file_type";

/// 当前数据库 schema 版本。
pub const CURRENT_SCHEMA_VERSION: u32 = 2;

/// 初始化指定路径上的 Nexus 本地数据库。
///
/// 路径由调用方传入，因此本 crate 不假设 Tauri、用户目录或任何平台路径。
/// 新数据库会在一个事务中执行所有待处理迁移；已是当前版本的数据库保持不变。
pub fn initialize_database<P: AsRef<Path>>(path: P) -> Result<Connection, DatabaseError> {
    let path = path.as_ref().to_path_buf();
    let mut connection = Connection::open(&path).map_err(|source| DatabaseError::Open {
        path: path.clone(),
        source,
    })?;

    let version = read_schema_version(&connection, &path)?;

    if version > CURRENT_SCHEMA_VERSION {
        return Err(DatabaseError::UnsupportedSchemaVersion {
            path,
            found: version,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }

    let mut version = version;
    while version < CURRENT_SCHEMA_VERSION {
        let next_version = version + 1;
        let Some(sql) = migration_sql(next_version) else {
            return Err(DatabaseError::UnsupportedSchemaVersion {
                path,
                found: version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        };

        apply_migration(&mut connection, &path, next_version, sql)?;
        version = next_version;
    }

    Ok(connection)
}

fn migration_sql(version: u32) -> Option<&'static str> {
    match version {
        1 => Some(FOUNDATION_MIGRATION_SQL),
        2 => Some(FILE_METADATA_MIGRATION_SQL),
        _ => None,
    }
}

fn read_schema_version(connection: &Connection, path: &Path) -> Result<u32, DatabaseError> {
    let version = connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(|source| DatabaseError::ReadSchemaVersion {
            path: path.to_path_buf(),
            source,
        })?;

    u32::try_from(version).map_err(|_| DatabaseError::InvalidSchemaVersion {
        path: path.to_path_buf(),
        value: version,
    })
}

fn apply_migration(
    connection: &mut Connection,
    path: &Path,
    version: u32,
    sql: &str,
) -> Result<(), DatabaseError> {
    let transaction = connection
        .transaction()
        .map_err(|source| DatabaseError::Migration {
            path: path.to_path_buf(),
            version,
            source,
        })?;

    transaction
        .execute_batch(sql)
        .map_err(|source| DatabaseError::Migration {
            path: path.to_path_buf(),
            version,
            source,
        })?;

    transaction
        .pragma_update(None, "user_version", version)
        .map_err(|source| DatabaseError::Migration {
            path: path.to_path_buf(),
            version,
            source,
        })?;

    transaction
        .commit()
        .map_err(|source| DatabaseError::Migration {
            path: path.to_path_buf(),
            version,
            source,
        })
}

/// 本地文件的最小元数据记录。
///
/// 时间字段统一使用 Unix epoch 毫秒；`None` 表示当前平台或文件系统无法提供
/// 对应时间。`file_type` 只保存调用方提供的可选类型标签，M1 不读取文件内容做
/// MIME 探测。文件路径的真实值由 `path` 保存，数据库内部使用平台相关的无损键。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadata {
    pub path: PathBuf,
    pub file_name: String,
    pub extension: Option<String>,
    pub size_bytes: u64,
    pub modified_at: Option<i64>,
    pub created_at: Option<i64>,
    pub accessed_at: Option<i64>,
    pub file_type: Option<String>,
}

impl FileMetadata {
    /// 根据文件路径生成一条最小元数据记录。
    ///
    /// 路径会被绝对化，但不会执行 `canonicalize`，因此不会解析或跟随符号链接。
    /// 扩展名不包含点号，并统一转换为 ASCII 小写；无法表示为 UTF-8 的文件名
    /// 使用无损路径保存、使用替代字符生成展示文本。
    pub fn from_path(
        path: PathBuf,
        size_bytes: u64,
        modified_at: Option<i64>,
        created_at: Option<i64>,
        accessed_at: Option<i64>,
        file_type: Option<String>,
    ) -> Result<Self, FileMetadataError> {
        let path = normalize_path(&path)?;
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or(FileMetadataError::MissingFileName)?;
        let extension = path
            .extension()
            .map(|extension| extension.to_string_lossy().to_ascii_lowercase());

        Ok(Self {
            path,
            file_name,
            extension,
            size_bytes,
            modified_at,
            created_at,
            accessed_at,
            file_type,
        })
    }
}

/// 一次批量写入的结果统计。
///
/// `received` 统计输入迭代器产生的所有记录，`written` 统计已提交的文件元数据
/// upsert，`failed` 统计输入错误或校验失败的记录，`batches` 统计成功提交的事务数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileMetadataBatchSummary {
    pub received: usize,
    pub written: usize,
    pub failed: usize,
    pub batches: usize,
}

/// 一次手动重扫的持久化结果统计。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileMetadataRescanSummary {
    pub files_succeeded: usize,
    pub files_failed: usize,
    pub paths_skipped: usize,
    pub records_removed: usize,
    pub batches_committed: usize,
}

/// 将路径转换为绝对路径，但不访问文件系统、不解析符号链接，也不进行大小写折叠。
pub fn normalize_path(path: &Path) -> Result<PathBuf, FileMetadataError> {
    if path.as_os_str().is_empty() {
        return Err(FileMetadataError::EmptyPath);
    }

    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    std::env::current_dir()
        .map(|current_directory| current_directory.join(path))
        .map_err(|source| FileMetadataError::CurrentDirectory { source })
}

/// 插入或更新一条文件元数据记录。
pub fn upsert_file_metadata(
    connection: &Connection,
    metadata: &FileMetadata,
) -> Result<(), FileMetadataError> {
    let prepared = prepare_file_metadata(metadata)?;
    execute_file_metadata_upsert(connection, metadata, &prepared)
}

/// 以有界内存分批写入文件元数据。
///
/// 每个批次都在独立事务中提交。输入中的单条错误或元数据校验失败会计入
/// `failed` 并继续处理；SQLite 事务或连接错误仍会返回错误，因为这类错误表示
/// 持久化边界本身不可用。调用方可以把扫描期间无法读取的文件映射为输入错误，
/// 从而避免把单个文件变化升级为整个扫描失败。
pub fn upsert_file_metadata_batch<I>(
    connection: &mut Connection,
    records: I,
    batch_size: usize,
) -> Result<FileMetadataBatchSummary, FileMetadataError>
where
    I: IntoIterator<Item = Result<FileMetadata, FileMetadataError>>,
{
    if batch_size == 0 {
        return Err(FileMetadataError::InvalidBatchSize);
    }

    let mut summary = FileMetadataBatchSummary::default();
    let mut batch = Vec::with_capacity(batch_size.min(1024));

    for record in records {
        summary.received += 1;

        let metadata = match record {
            Ok(metadata) => metadata,
            Err(_) => {
                summary.failed += 1;
                continue;
            }
        };

        let prepared = match prepare_file_metadata(&metadata) {
            Ok(prepared) => prepared,
            Err(_) => {
                summary.failed += 1;
                continue;
            }
        };

        batch.push((metadata, prepared));
        if batch.len() == batch_size {
            write_file_metadata_batch(connection, &batch)?;
            summary.written += batch.len();
            summary.batches += 1;
            batch.clear();
        }
    }

    if !batch.is_empty() {
        write_file_metadata_batch(connection, &batch)?;
        summary.written += batch.len();
        summary.batches += 1;
    }

    Ok(summary)
}

/// 开始一次针对指定目录的文件元数据重扫。
///
/// 重扫使用连接级临时表记录本次扫描见到的路径和需要保护的失败/跳过路径，
/// 因此不会修改 schema，也不需要把完整扫描结果保存在调用方内存中。调用方
/// 应逐条调用 [`FileMetadataRescan::record_file`]、`record_failure` 或
/// `record_skip`，最后调用 [`FileMetadataRescan::finish`] 完成旧记录清理。
pub fn begin_file_metadata_rescan<'connection>(
    connection: &'connection mut Connection,
    root: &Path,
    batch_size: usize,
) -> Result<FileMetadataRescan<'connection>, FileMetadataError> {
    if batch_size == 0 {
        return Err(FileMetadataError::InvalidBatchSize);
    }

    let root = normalize_path(root)?;
    connection
        .execute_batch(
            "DROP TABLE IF EXISTS temp.nexus_scan_seen;
             DROP TABLE IF EXISTS temp.nexus_scan_protected;
             DROP TABLE IF EXISTS temp.nexus_scan_pending;
             CREATE TEMP TABLE nexus_scan_seen (
                 path_key BLOB PRIMARY KEY NOT NULL
             );
             CREATE TEMP TABLE nexus_scan_protected (
                 path_key BLOB PRIMARY KEY NOT NULL
             );
             CREATE TEMP TABLE nexus_scan_pending (
                 path_key BLOB PRIMARY KEY NOT NULL,
                 path_display TEXT NOT NULL,
                 file_name TEXT NOT NULL,
                 extension TEXT,
                 size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
                 modified_at INTEGER,
                 created_at INTEGER,
                 accessed_at INTEGER,
                 file_type TEXT
             );",
        )
        .map_err(|source| FileMetadataError::Query {
            operation: "rescan_begin",
            source,
        })?;

    Ok(FileMetadataRescan {
        connection,
        root_key: scope_path_key(&root),
        batch_size,
        pending: Vec::with_capacity(batch_size.min(1024)),
        files_succeeded: 0,
        files_failed: 0,
        paths_skipped: 0,
        batches_committed: 0,
    })
}

/// 手动重扫的数据库写入会话。
pub struct FileMetadataRescan<'connection> {
    connection: &'connection mut Connection,
    root_key: Vec<u8>,
    batch_size: usize,
    pending: Vec<(FileMetadata, PreparedFileMetadata)>,
    files_succeeded: usize,
    files_failed: usize,
    paths_skipped: usize,
    batches_committed: usize,
}

impl FileMetadataRescan<'_> {
    /// 记录一条成功提取的文件元数据，并在达到批次大小时提交事务。
    ///
    /// 单条元数据校验失败会计入失败数量并继续，不会让整个重扫失败；数据库
    /// 事务错误仍会返回调用方。
    pub fn record_file(&mut self, metadata: FileMetadata) -> Result<(), FileMetadataError> {
        let prepared = match prepare_file_metadata(&metadata) {
            Ok(prepared) => prepared,
            Err(_) => {
                if normalize_path(&metadata.path).is_ok() {
                    self.protect_path(&metadata.path, "rescan_protect_failure")?;
                }
                self.files_failed += 1;
                return Ok(());
            }
        };

        self.pending.push((metadata, prepared));
        self.files_succeeded += 1;

        if self.pending.len() == self.batch_size {
            self.flush_pending()?;
        }

        Ok(())
    }

    /// 记录一个扫描失败路径，并保护该路径及其子路径不被本次重扫删除。
    pub fn record_failure<P: AsRef<Path>>(&mut self, path: P) -> Result<(), FileMetadataError> {
        self.protect_path(path.as_ref(), "rescan_protect_failure")?;
        self.files_failed += 1;
        Ok(())
    }

    /// 记录一个跳过路径，并保护该路径及其子路径不被本次重扫删除。
    pub fn record_skip<P: AsRef<Path>>(&mut self, path: P) -> Result<(), FileMetadataError> {
        self.protect_path(path.as_ref(), "rescan_protect_skip")?;
        self.paths_skipped += 1;
        Ok(())
    }

    /// 提交剩余批次、删除本次重扫确认不存在的旧记录，并清理临时表。
    pub fn finish(mut self) -> Result<FileMetadataRescanSummary, FileMetadataError> {
        let result = self.finish_inner();
        let cleanup_result = self.cleanup_temp_tables();

        match (result, cleanup_result) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(summary), Ok(())) => Ok(summary),
        }
    }

    fn finish_inner(&mut self) -> Result<FileMetadataRescanSummary, FileMetadataError> {
        self.flush_pending()?;
        let records_removed = self.apply_staged_records_and_delete_stale()?;

        Ok(FileMetadataRescanSummary {
            files_succeeded: self.files_succeeded,
            files_failed: self.files_failed,
            paths_skipped: self.paths_skipped,
            records_removed,
            batches_committed: self.batches_committed,
        })
    }

    fn protect_path(
        &mut self,
        path: &Path,
        operation: &'static str,
    ) -> Result<(), FileMetadataError> {
        let path = normalize_path(path)?;
        let path_key = scope_path_key(&path);

        self.connection
            .execute(
                "INSERT OR IGNORE INTO temp.nexus_scan_protected (path_key) VALUES (?1)",
                params![path_key],
            )
            .map_err(|source| FileMetadataError::Query { operation, source })?;

        Ok(())
    }

    fn flush_pending(&mut self) -> Result<(), FileMetadataError> {
        if self.pending.is_empty() {
            return Ok(());
        }

        let transaction =
            self.connection
                .transaction()
                .map_err(|source| FileMetadataError::Query {
                    operation: "rescan_batch_begin",
                    source,
                })?;

        for (metadata, prepared) in &self.pending {
            transaction
                .execute(
                    STAGED_FILE_METADATA_SQL,
                    params![
                        &prepared.path_key,
                        &prepared.path_display,
                        &metadata.file_name,
                        &metadata.extension,
                        prepared.size_bytes,
                        metadata.modified_at,
                        metadata.created_at,
                        metadata.accessed_at,
                        &metadata.file_type,
                    ],
                )
                .map_err(|source| FileMetadataError::Query {
                    operation: "rescan_stage",
                    source,
                })?;

            transaction
                .execute(
                    "INSERT OR IGNORE INTO temp.nexus_scan_seen (path_key) VALUES (?1)",
                    params![&prepared.path_key],
                )
                .map_err(|source| FileMetadataError::Query {
                    operation: "rescan_seen",
                    source,
                })?;
        }

        transaction
            .commit()
            .map_err(|source| FileMetadataError::Query {
                operation: "rescan_batch_commit",
                source,
            })?;

        self.pending.clear();
        self.batches_committed += 1;
        Ok(())
    }

    fn apply_staged_records_and_delete_stale(&mut self) -> Result<usize, FileMetadataError> {
        let transaction =
            self.connection
                .transaction()
                .map_err(|source| FileMetadataError::Query {
                    operation: "rescan_cleanup_begin",
                    source,
                })?;

        transaction
            .execute(APPLY_STAGED_FILE_METADATA_SQL, [])
            .map_err(|source| FileMetadataError::Query {
                operation: "rescan_apply",
                source,
            })?;

        let records_removed = transaction
            .execute(delete_stale_records_sql(), params![&self.root_key])
            .map_err(|source| FileMetadataError::Query {
                operation: "rescan_remove",
                source,
            })?;

        transaction
            .commit()
            .map_err(|source| FileMetadataError::Query {
                operation: "rescan_cleanup_commit",
                source,
            })?;

        Ok(records_removed)
    }

    fn cleanup_temp_tables(&mut self) -> Result<(), FileMetadataError> {
        self.connection
            .execute_batch(
                "DROP TABLE IF EXISTS temp.nexus_scan_seen;
                 DROP TABLE IF EXISTS temp.nexus_scan_protected;
                 DROP TABLE IF EXISTS temp.nexus_scan_pending;",
            )
            .map_err(|source| FileMetadataError::Query {
                operation: "rescan_cleanup_tables",
                source,
            })
    }
}

impl Drop for FileMetadataRescan<'_> {
    fn drop(&mut self) {
        let _ = self.cleanup_temp_tables();
    }
}

struct PreparedFileMetadata {
    path_key: Vec<u8>,
    path_display: String,
    size_bytes: i64,
}

fn prepare_file_metadata(
    metadata: &FileMetadata,
) -> Result<PreparedFileMetadata, FileMetadataError> {
    if metadata.file_name.is_empty() {
        return Err(FileMetadataError::EmptyFileName);
    }

    let path = normalize_path(&metadata.path)?;
    let size_bytes =
        i64::try_from(metadata.size_bytes).map_err(|_| FileMetadataError::SizeOutOfRange {
            value: metadata.size_bytes,
        })?;

    Ok(PreparedFileMetadata {
        path_key: path_key(&path),
        path_display: path.to_string_lossy().into_owned(),
        size_bytes,
    })
}

fn execute_file_metadata_upsert(
    connection: &Connection,
    metadata: &FileMetadata,
    prepared: &PreparedFileMetadata,
) -> Result<(), FileMetadataError> {
    connection
        .execute(
            "INSERT INTO file_metadata (
                 path_key,
                 path_display,
                 file_name,
                 extension,
                 size_bytes,
                 modified_at,
                 created_at,
                 accessed_at,
                 file_type
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(path_key) DO UPDATE SET
                 path_display = excluded.path_display,
                 file_name = excluded.file_name,
                 extension = excluded.extension,
                 size_bytes = excluded.size_bytes,
                 modified_at = excluded.modified_at,
                 created_at = excluded.created_at,
                 accessed_at = excluded.accessed_at,
                 file_type = excluded.file_type",
            params![
                &prepared.path_key,
                &prepared.path_display,
                metadata.file_name,
                metadata.extension,
                prepared.size_bytes,
                metadata.modified_at,
                metadata.created_at,
                metadata.accessed_at,
                metadata.file_type,
            ],
        )
        .map_err(|source| FileMetadataError::Query {
            operation: "upsert",
            source,
        })?;

    Ok(())
}

fn write_file_metadata_batch(
    connection: &mut Connection,
    batch: &[(FileMetadata, PreparedFileMetadata)],
) -> Result<(), FileMetadataError> {
    let transaction = connection
        .transaction()
        .map_err(|source| FileMetadataError::Query {
            operation: "batch_begin",
            source,
        })?;

    for (metadata, prepared) in batch {
        execute_file_metadata_upsert(&transaction, metadata, prepared).map_err(
            |error| match error {
                FileMetadataError::Query { source, .. } => FileMetadataError::Query {
                    operation: "batch_upsert",
                    source,
                },
                other => other,
            },
        )?;
    }

    transaction
        .commit()
        .map_err(|source| FileMetadataError::Query {
            operation: "batch_commit",
            source,
        })
}

/// 按路径读取一条文件元数据记录。
pub fn get_file_metadata<P: AsRef<Path>>(
    connection: &Connection,
    path: P,
) -> Result<Option<FileMetadata>, FileMetadataError> {
    let path = normalize_path(path.as_ref())?;
    let path_key = path_key(&path);
    let row = connection
        .query_row(
            "SELECT file_name, extension, size_bytes, modified_at,
                    created_at, accessed_at, file_type
             FROM file_metadata
             WHERE path_key = ?1",
            params![path_key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .optional()
        .map_err(|source| FileMetadataError::Query {
            operation: "get",
            source,
        })?;

    let Some((file_name, extension, size_bytes, modified_at, created_at, accessed_at, file_type)) =
        row
    else {
        return Ok(None);
    };

    let size_bytes = u64::try_from(size_bytes)
        .map_err(|_| FileMetadataError::InvalidStoredSize { value: size_bytes })?;

    Ok(Some(FileMetadata {
        path,
        file_name,
        extension,
        size_bytes,
        modified_at,
        created_at,
        accessed_at,
        file_type,
    }))
}

#[cfg(unix)]
fn path_key(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().to_vec()
}

#[cfg(windows)]
fn path_key(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(not(any(unix, windows)))]
fn path_key(path: &Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

fn scope_path_key(path: &Path) -> Vec<u8> {
    let mut key = path_key(path);

    #[cfg(unix)]
    while key.last() == Some(&b'/') {
        key.pop();
    }

    #[cfg(windows)]
    while key.len() > 2 && (key.ends_with(&[b'/', 0]) || key.ends_with(&[b'\\', 0])) {
        key.truncate(key.len() - 2);
    }

    #[cfg(not(any(unix, windows)))]
    while key.last() == Some(&b'/') {
        key.pop();
    }

    key
}

#[cfg(unix)]
fn delete_stale_records_sql() -> &'static str {
    "DELETE FROM file_metadata
     WHERE (
         length(?1) = 0
         OR file_metadata.path_key = ?1
         OR (
             length(file_metadata.path_key) > length(?1)
             AND substr(file_metadata.path_key, 1, length(?1)) = ?1
             AND substr(file_metadata.path_key, length(?1) + 1, 1) = X'2F'
         )
     )
     AND NOT EXISTS (
         SELECT 1
         FROM temp.nexus_scan_seen AS seen
         WHERE seen.path_key = file_metadata.path_key
     )
     AND NOT EXISTS (
         SELECT 1
         FROM temp.nexus_scan_protected AS protected
         WHERE length(protected.path_key) = 0
            OR protected.path_key = file_metadata.path_key
            OR (
                length(file_metadata.path_key) > length(protected.path_key)
                AND substr(
                    file_metadata.path_key,
                    1,
                    length(protected.path_key)
                ) = protected.path_key
                AND substr(
                    file_metadata.path_key,
                    length(protected.path_key) + 1,
                    1
                ) = X'2F'
            )
     )"
}

#[cfg(windows)]
fn delete_stale_records_sql() -> &'static str {
    "DELETE FROM file_metadata
     WHERE (
         length(?1) = 0
         OR file_metadata.path_key = ?1
         OR (
             length(file_metadata.path_key) > length(?1)
             AND substr(file_metadata.path_key, 1, length(?1)) = ?1
             AND (
                 substr(file_metadata.path_key, length(?1) + 1, 2) = X'5C00'
                 OR substr(file_metadata.path_key, length(?1) + 1, 2) = X'2F00'
             )
         )
     )
     AND NOT EXISTS (
         SELECT 1
         FROM temp.nexus_scan_seen AS seen
         WHERE seen.path_key = file_metadata.path_key
     )
     AND NOT EXISTS (
         SELECT 1
         FROM temp.nexus_scan_protected AS protected
         WHERE length(protected.path_key) = 0
            OR protected.path_key = file_metadata.path_key
            OR (
                length(file_metadata.path_key) > length(protected.path_key)
                AND substr(
                    file_metadata.path_key,
                    1,
                    length(protected.path_key)
                ) = protected.path_key
                AND (
                    substr(
                        file_metadata.path_key,
                        length(protected.path_key) + 1,
                        2
                    ) = X'5C00'
                    OR substr(
                        file_metadata.path_key,
                        length(protected.path_key) + 1,
                        2
                    ) = X'2F00'
                )
            )
     )"
}

#[cfg(not(any(unix, windows)))]
fn delete_stale_records_sql() -> &'static str {
    "DELETE FROM file_metadata
     WHERE (
         length(?1) = 0
         OR file_metadata.path_key = ?1
         OR (
             length(file_metadata.path_key) > length(?1)
             AND substr(file_metadata.path_key, 1, length(?1)) = ?1
             AND substr(file_metadata.path_key, length(?1) + 1, 1) = X'2F'
         )
     )
     AND NOT EXISTS (
         SELECT 1
         FROM temp.nexus_scan_seen AS seen
         WHERE seen.path_key = file_metadata.path_key
     )
     AND NOT EXISTS (
         SELECT 1
         FROM temp.nexus_scan_protected AS protected
         WHERE length(protected.path_key) = 0
            OR protected.path_key = file_metadata.path_key
            OR (
                length(file_metadata.path_key) > length(protected.path_key)
                AND substr(
                    file_metadata.path_key,
                    1,
                    length(protected.path_key)
                ) = protected.path_key
                AND substr(
                    file_metadata.path_key,
                    length(protected.path_key) + 1,
                    1
                ) = X'2F'
            )
     )"
}

/// 文件元数据持久化错误。
#[derive(Debug)]
pub enum FileMetadataError {
    EmptyPath,
    EmptyFileName,
    MissingFileName,
    InvalidBatchSize,
    CurrentDirectory {
        source: std::io::Error,
    },
    SizeOutOfRange {
        value: u64,
    },
    InvalidStoredSize {
        value: i64,
    },
    Query {
        operation: &'static str,
        source: rusqlite::Error,
    },
}

impl FileMetadataError {
    /// 返回不包含路径、文件名或原始数据的安全错误分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::EmptyPath | Self::MissingFileName => "file_metadata_path_invalid",
            Self::EmptyFileName => "file_metadata_name_invalid",
            Self::InvalidBatchSize => "file_metadata_batch_size_invalid",
            Self::CurrentDirectory { .. } => "file_metadata_current_directory",
            Self::SizeOutOfRange { .. } => "file_metadata_size_invalid",
            Self::InvalidStoredSize { .. } => "file_metadata_size_corrupt",
            Self::Query { operation, .. } => match *operation {
                "upsert" => "file_metadata_write",
                "get" => "file_metadata_read",
                "batch_begin" | "batch_upsert" | "batch_commit" => "file_metadata_batch_write",
                _ => "file_metadata_query",
            },
        }
    }
}

impl fmt::Display for FileMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath => write!(formatter, "文件路径不能为空。"),
            Self::EmptyFileName => write!(formatter, "文件名不能为空。"),
            Self::MissingFileName => write!(formatter, "文件路径不包含文件名。"),
            Self::InvalidBatchSize => write!(formatter, "文件元数据批次大小必须大于零。"),
            Self::CurrentDirectory { .. } => write!(formatter, "无法确定当前工作目录。"),
            Self::SizeOutOfRange { .. } => {
                write!(formatter, "文件大小超过本地数据库支持范围。")
            }
            Self::InvalidStoredSize { .. } => write!(formatter, "数据库中的文件大小无效。"),
            Self::Query { operation, .. } => {
                write!(formatter, "文件元数据数据库操作失败（{operation}）。")
            }
        }
    }
}

impl Error for FileMetadataError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CurrentDirectory { source } => Some(source),
            Self::Query { source, .. } => Some(source),
            Self::EmptyPath
            | Self::EmptyFileName
            | Self::MissingFileName
            | Self::InvalidBatchSize
            | Self::SizeOutOfRange { .. }
            | Self::InvalidStoredSize { .. } => None,
        }
    }
}

/// 数据库初始化和迁移错误。
#[derive(Debug)]
pub enum DatabaseError {
    /// 无法打开调用方指定的数据库路径。
    Open {
        path: PathBuf,
        source: rusqlite::Error,
    },
    /// 无法读取 SQLite schema 版本。
    ReadSchemaVersion {
        path: PathBuf,
        source: rusqlite::Error,
    },
    /// 数据库中的版本值超出当前 Rust 类型允许范围。
    InvalidSchemaVersion { path: PathBuf, value: i64 },
    /// 数据库版本不是当前版本或已知的迁移起点。
    UnsupportedSchemaVersion {
        path: PathBuf,
        found: u32,
        supported: u32,
    },
    /// 迁移事务中的任一步骤失败。
    Migration {
        path: PathBuf,
        version: u32,
        source: rusqlite::Error,
    },
}

impl DatabaseError {
    /// 返回可安全写入日志的错误分类，不包含路径、SQL 或 SQLite 原始信息。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Open { .. } => "database_open",
            Self::ReadSchemaVersion { .. } => "database_schema_read",
            Self::InvalidSchemaVersion { .. } => "database_schema_invalid",
            Self::UnsupportedSchemaVersion { .. } => "database_schema_unsupported",
            Self::Migration { .. } => "database_migration",
        }
    }
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open { path, source } => {
                write!(formatter, "无法打开数据库 {}: {source}", path.display())
            }
            Self::ReadSchemaVersion { path, source } => write!(
                formatter,
                "无法读取数据库 {} 的 schema 版本: {source}",
                path.display()
            ),
            Self::InvalidSchemaVersion { path, value } => write!(
                formatter,
                "数据库 {} 的 schema 版本无效: {value}",
                path.display()
            ),
            Self::UnsupportedSchemaVersion {
                path,
                found,
                supported,
            } => write!(
                formatter,
                "数据库 {} 的 schema 版本 {found} 不受支持，当前支持版本为 {supported}",
                path.display()
            ),
            Self::Migration {
                path,
                version,
                source,
            } => write!(
                formatter,
                "数据库 {} 执行迁移 {version} 失败: {source}",
                path.display()
            ),
        }
    }
}

impl Error for DatabaseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Open { source, .. }
            | Self::ReadSchemaVersion { source, .. }
            | Self::Migration { source, .. } => Some(source),
            Self::InvalidSchemaVersion { .. } | Self::UnsupportedSchemaVersion { .. } => None,
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

    use rusqlite::{params, Connection};

    use super::{
        begin_file_metadata_rescan, get_file_metadata, initialize_database, normalize_path,
        upsert_file_metadata, upsert_file_metadata_batch, DatabaseError, FileMetadata,
        FileMetadataError, CURRENT_SCHEMA_VERSION, FOUNDATION_MIGRATION_SQL,
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
                    "nexus-db-test-{}-{timestamp}-{counter}-{attempt}",
                    process::id()
                ));

                match fs::create_dir(&path) {
                    Ok(()) => return Self { path },
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("创建测试临时目录失败: {error}"),
                }
            }

            panic!("无法创建唯一测试临时目录")
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

    fn schema_version(connection: &Connection) -> i64 {
        connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("读取测试数据库版本失败")
    }

    fn table_exists(connection: &Connection, table_name: &str) -> bool {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                [table_name],
                |row| row.get(0),
            )
            .expect("检查测试表失败")
    }

    fn synthetic_metadata(root: &Path, index: usize) -> FileMetadata {
        FileMetadata {
            path: root.join(format!("synthetic-{index:06}.txt")),
            file_name: format!("synthetic-{index:06}.txt"),
            extension: Some("txt".to_owned()),
            size_bytes: index as u64,
            modified_at: Some(index as i64),
            created_at: None,
            accessed_at: None,
            file_type: Some("text/plain".to_owned()),
        }
    }

    #[test]
    fn initializes_new_database_with_foundation_schema() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();

        let connection = initialize_database(&database_path).expect("初始化新数据库失败");

        assert_eq!(
            schema_version(&connection),
            i64::from(CURRENT_SCHEMA_VERSION)
        );
        assert!(table_exists(&connection, "nexus_metadata"));
        assert!(table_exists(&connection, "file_metadata"));
        assert!(database_path.is_file());
    }

    #[test]
    fn migrates_existing_foundation_database_to_file_metadata_schema() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();

        let connection = Connection::open(&database_path).expect("创建迁移测试数据库失败");
        connection
            .execute_batch(FOUNDATION_MIGRATION_SQL)
            .expect("创建基础 schema 失败");
        connection
            .pragma_update(None, "user_version", 1)
            .expect("设置基础 schema 版本失败");
        connection
            .execute(
                "INSERT INTO nexus_metadata (key, value) VALUES (?1, ?2)",
                params!["migration", "preserved"],
            )
            .expect("写入迁移测试元数据失败");
        drop(connection);

        let connection = initialize_database(&database_path).expect("执行文件元数据迁移失败");

        assert_eq!(
            schema_version(&connection),
            i64::from(CURRENT_SCHEMA_VERSION)
        );
        assert!(table_exists(&connection, "file_metadata"));
        let value: String = connection
            .query_row(
                "SELECT value FROM nexus_metadata WHERE key = ?1",
                ["migration"],
                |row| row.get(0),
            )
            .expect("读取迁移后的元数据失败");
        assert_eq!(value, "preserved");
    }

    #[test]
    fn initialization_is_idempotent_and_preserves_metadata() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();

        let connection = initialize_database(&database_path).expect("首次初始化数据库失败");
        connection
            .execute(
                "INSERT INTO nexus_metadata (key, value) VALUES (?1, ?2)",
                params!["workspace", "nexus"],
            )
            .expect("写入测试元数据失败");
        drop(connection);

        let connection = initialize_database(&database_path).expect("重复初始化数据库失败");
        let value: String = connection
            .query_row(
                "SELECT value FROM nexus_metadata WHERE key = ?1",
                ["workspace"],
                |row| row.get(0),
            )
            .expect("读取持久化元数据失败");

        assert_eq!(value, "nexus");
        assert_eq!(
            schema_version(&connection),
            i64::from(CURRENT_SCHEMA_VERSION)
        );
    }

    #[test]
    fn rejects_unknown_schema_version_without_migrating() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let unknown_version = CURRENT_SCHEMA_VERSION + 1;

        let connection = Connection::open(&database_path).expect("创建版本测试数据库失败");
        connection
            .pragma_update(None, "user_version", unknown_version)
            .expect("写入未知 schema 版本失败");
        drop(connection);

        match initialize_database(&database_path) {
            Err(DatabaseError::UnsupportedSchemaVersion {
                found, supported, ..
            }) => {
                assert_eq!(found, unknown_version);
                assert_eq!(supported, CURRENT_SCHEMA_VERSION);
            }
            Err(error) => panic!("返回了错误类型: {error}"),
            Ok(_) => panic!("未知 schema 版本不应初始化成功"),
        }
    }

    #[test]
    fn reports_context_for_inaccessible_database_path() {
        let temporary_directory = TemporaryDirectory::new();
        let blocker_path = temporary_directory.child_path("not-a-directory");
        fs::write(&blocker_path, b"not a directory").expect("创建路径阻断文件失败");
        let database_path = blocker_path.join("nexus.sqlite3");

        match initialize_database(&database_path) {
            Err(DatabaseError::Open { path, .. }) => assert_eq!(path, database_path),
            Err(error) => panic!("返回了错误类型: {error}"),
            Ok(_) => panic!("不可访问路径不应初始化成功"),
        }
    }

    #[test]
    fn builds_metadata_with_absolute_path_and_normalized_extension() {
        let metadata = FileMetadata::from_path(
            PathBuf::from("notes/Meeting.MD"),
            128,
            Some(1_700_000_000_000),
            None,
            None,
            Some("text/markdown".to_owned()),
        )
        .expect("从路径生成文件元数据失败");

        assert!(metadata.path.is_absolute());
        assert_eq!(metadata.file_name, "Meeting.MD");
        assert_eq!(metadata.extension.as_deref(), Some("md"));
        assert_eq!(metadata.size_bytes, 128);
        assert_eq!(metadata.modified_at, Some(1_700_000_000_000));
    }

    #[test]
    fn upserts_and_reads_file_metadata_by_normalized_path() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let connection = initialize_database(&database_path).expect("初始化元数据测试数据库失败");
        let file_path = temporary_directory.child_path("notes/Meeting.MD");

        let metadata = FileMetadata {
            path: file_path.clone(),
            file_name: "Meeting.MD".to_owned(),
            extension: Some("md".to_owned()),
            size_bytes: 128,
            modified_at: Some(1_700_000_000_000),
            created_at: None,
            accessed_at: Some(1_700_000_000_100),
            file_type: Some("text/markdown".to_owned()),
        };
        upsert_file_metadata(&connection, &metadata).expect("首次写入文件元数据失败");

        let loaded = get_file_metadata(&connection, &file_path)
            .expect("读取文件元数据失败")
            .expect("找不到刚写入的文件元数据");
        assert_eq!(loaded, metadata);

        let updated = FileMetadata {
            size_bytes: 256,
            modified_at: Some(1_700_000_001_000),
            ..metadata.clone()
        };
        upsert_file_metadata(&connection, &updated).expect("更新文件元数据失败");

        let loaded = get_file_metadata(&connection, &file_path)
            .expect("读取更新后的文件元数据失败")
            .expect("找不到更新后的文件元数据");
        assert_eq!(loaded, updated);

        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM file_metadata", [], |row| row.get(0))
            .expect("统计文件元数据失败");
        assert_eq!(count, 1);
    }

    #[test]
    fn rejects_invalid_file_metadata_without_database_write() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let connection = initialize_database(&database_path).expect("初始化元数据测试数据库失败");

        let error = upsert_file_metadata(
            &connection,
            &FileMetadata {
                path: PathBuf::new(),
                file_name: "invalid.txt".to_owned(),
                extension: Some("txt".to_owned()),
                size_bytes: 1,
                modified_at: None,
                created_at: None,
                accessed_at: None,
                file_type: None,
            },
        )
        .expect_err("空路径不应写入数据库");

        assert_eq!(error.kind(), "file_metadata_path_invalid");
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM file_metadata", [], |row| row.get(0))
            .expect("统计无效写入结果失败");
        assert_eq!(count, 0);
    }

    #[test]
    fn writes_metadata_in_bounded_transactions() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let mut connection = initialize_database(&database_path).expect("初始化批量测试数据库失败");
        let root = temporary_directory.path.clone();

        let records = (0..5).map(|index| Ok(synthetic_metadata(&root, index)));
        let summary = upsert_file_metadata_batch(&mut connection, records, 2)
            .expect("批量写入文件元数据失败");

        assert_eq!(summary.received, 5);
        assert_eq!(summary.written, 5);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.batches, 3);

        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM file_metadata", [], |row| row.get(0))
            .expect("统计批量写入结果失败");
        assert_eq!(count, 5);
    }

    #[test]
    fn dropping_rescan_session_does_not_apply_staged_records() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let mut connection = initialize_database(&database_path).expect("初始化取消测试数据库失败");
        let root = temporary_directory.path.clone();
        let existing = synthetic_metadata(&root, 0);
        let staged = synthetic_metadata(&root, 1);
        upsert_file_metadata(&connection, &existing).expect("写入取消测试既有记录失败");

        {
            let mut session = begin_file_metadata_rescan(&mut connection, &root, 1)
                .expect("创建取消测试重扫会话失败");
            session
                .record_file(staged.clone())
                .expect("暂存取消测试记录失败");
        }

        assert!(get_file_metadata(&connection, &existing.path)
            .expect("读取取消测试既有记录失败")
            .is_some());
        assert!(get_file_metadata(&connection, &staged.path)
            .expect("读取取消测试暂存记录失败")
            .is_none());
    }

    #[test]
    fn counts_record_failures_and_continues_with_valid_records() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let mut connection =
            initialize_database(&database_path).expect("初始化失败统计测试数据库失败");
        let root = temporary_directory.path.clone();
        let invalid_size = FileMetadata {
            size_bytes: u64::MAX,
            ..synthetic_metadata(&root, 1)
        };

        let records = vec![
            Ok(synthetic_metadata(&root, 0)),
            Err(FileMetadataError::EmptyPath),
            Ok(invalid_size),
            Ok(synthetic_metadata(&root, 2)),
        ];
        let summary = upsert_file_metadata_batch(&mut connection, records, 2)
            .expect("单条失败不应终止批量写入");

        assert_eq!(summary.received, 4);
        assert_eq!(summary.written, 2);
        assert_eq!(summary.failed, 2);
        assert_eq!(summary.batches, 1);

        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM file_metadata", [], |row| row.get(0))
            .expect("统计失败记录测试结果失败");
        assert_eq!(count, 2);
    }

    #[test]
    fn rejects_zero_batch_size_before_consuming_records() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let mut connection =
            initialize_database(&database_path).expect("初始化批次大小测试数据库失败");
        let records = std::iter::from_fn(|| -> Option<Result<FileMetadata, FileMetadataError>> {
            panic!("零批次大小不应消费输入记录")
        });

        let error = upsert_file_metadata_batch(&mut connection, records, 0)
            .expect_err("零批次大小不应开始写入");

        assert!(matches!(error, FileMetadataError::InvalidBatchSize));
    }

    #[test]
    fn writes_one_hundred_thousand_synthetic_records() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let mut connection =
            initialize_database(&database_path).expect("初始化大批量测试数据库失败");
        let root = temporary_directory.path.clone();

        let records = (0..100_000).map(|index| Ok(synthetic_metadata(&root, index)));
        let summary = upsert_file_metadata_batch(&mut connection, records, 1_024)
            .expect("写入十万条合成文件元数据失败");

        assert_eq!(summary.received, 100_000);
        assert_eq!(summary.written, 100_000);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.batches, 98);

        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM file_metadata", [], |row| row.get(0))
            .expect("统计十万条写入结果失败");
        assert_eq!(count, 100_000);
    }

    #[test]
    fn normalization_makes_path_absolute_without_filesystem_lookup() {
        let path = PathBuf::from("link-to-notes/meeting.md");

        let normalized = normalize_path(&path).expect("规范化路径失败");

        assert!(normalized.is_absolute());
        assert!(normalized.ends_with(PathBuf::from("link-to-notes/meeting.md")));
    }

    #[cfg(unix)]
    #[test]
    fn normalization_does_not_resolve_symlink() {
        use std::os::unix::fs::symlink;

        let temporary_directory = TemporaryDirectory::new();
        let target_directory = temporary_directory.child_path("target");
        let link_directory = temporary_directory.child_path("link");
        fs::create_dir(&target_directory).expect("创建符号链接目标目录失败");
        symlink(&target_directory, &link_directory).expect("创建符号链接失败");
        let linked_file = link_directory.join("meeting.md");

        let normalized = normalize_path(&linked_file).expect("规范化符号链接路径失败");

        assert_eq!(normalized, linked_file);
        assert_ne!(normalized, target_directory.join("meeting.md"));
    }

    #[cfg(unix)]
    #[test]
    fn stores_non_utf8_path_without_panicking() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let connection =
            initialize_database(&database_path).expect("初始化非 UTF-8 路径测试数据库失败");
        let path = PathBuf::from(OsString::from_vec(vec![b'n', b'o', b't', b'e', 0xff]));
        let metadata = FileMetadata {
            path: path.clone(),
            file_name: "不可解码文件名".to_owned(),
            extension: None,
            size_bytes: 7,
            modified_at: None,
            created_at: None,
            accessed_at: None,
            file_type: None,
        };

        upsert_file_metadata(&connection, &metadata).expect("写入非 UTF-8 路径失败");
        let loaded = get_file_metadata(&connection, &path)
            .expect("读取非 UTF-8 路径失败")
            .expect("找不到非 UTF-8 路径记录");

        assert_eq!(loaded.path, path);
        assert_eq!(loaded.size_bytes, 7);
    }
}
