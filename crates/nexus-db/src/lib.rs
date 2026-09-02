//! Nexus 本地数据库边界。
//!
//! M1.0–M1.2 在本地数据库边界提供文件元数据模型、单条 upsert 和批量持久化入口。
//! M3.0 增加统一文档的 canonical 持久化边界；M3.1 在其上维护 SQLite FTS5 索引；
//! M3.2 提供受限查询和基本元数据过滤；M3.3 提供确定性 relevance 和匹配片段。

#![forbid(unsafe_code)]

mod embedding;
mod search;

use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

use rusqlite::{params, Connection, OptionalExtension, Row};

pub use embedding::{
    get_document_embedding, upsert_document_embeddings, DocumentEmbedding, EmbeddingModel,
    EmbeddingStoreError, EmbeddingWriteSummary, EMBEDDING_FINGERPRINT_BYTES,
    MAX_EMBEDDING_DIMENSIONS,
};
pub use search::{
    extract_search_text, search_documents, search_documents_hybrid, HybridSearchResult,
    SearchError, SearchResult, DEFAULT_SEARCH_LIMIT, MAX_SEARCH_LIMIT,
};

const FOUNDATION_MIGRATION_SQL: &str = include_str!("../migrations/0001_foundation.sql");
const FILE_METADATA_MIGRATION_SQL: &str = include_str!("../migrations/0002_file_metadata.sql");
const DOCUMENTS_MIGRATION_SQL: &str = include_str!("../migrations/0003_documents.sql");
const DOCUMENTS_FTS_MIGRATION_SQL: &str = include_str!("../migrations/0004_documents_fts.sql");
const EMBEDDINGS_MIGRATION_SQL: &str = include_str!("../migrations/0005_embeddings.sql");
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
pub const CURRENT_SCHEMA_VERSION: u32 = 5;

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
        3 => Some(DOCUMENTS_MIGRATION_SQL),
        4 => Some(DOCUMENTS_FTS_MIGRATION_SQL),
        5 => Some(EMBEDDINGS_MIGRATION_SQL),
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

    /// 在当前重扫连接上原子写入一条正文文档。
    ///
    /// M3.6 的初始索引复用文件元数据重扫会话，因此不需要打开第二个 SQLite
    /// 连接。文档和其 FTS trigger 更新由同一条数据库写操作保证一致；元数据
    /// 的批次提交和最终清理仍由本会话的 `finish` 负责。
    pub fn upsert_document(&mut self, document: &DocumentRecord) -> Result<(), DocumentStoreError> {
        upsert_document(self.connection, document)
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

/// 可持久化的统一本地文档记录。
///
/// 这是 `nexus-db` 自己的存储边界类型，不依赖 `nexus-core` 的领域类型，
/// 从而保持 `nexus-core → nexus-db` 的单向依赖。正文是 canonical 内容；M3.1
/// 在此表之上维护 FTS5 索引，而不是把 FTS 行当作唯一数据源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRecord {
    pub id: String,
    pub source_path: PathBuf,
    pub title: String,
    pub body: String,
    pub line_start: Option<u64>,
    pub line_end: Option<u64>,
}

/// 一次增量文件更新的数据库操作。
///
/// `Upsert` 会先保存文件元数据；如果 `document` 为 `None`，则同时清理该路径的
/// 旧正文记录。`Remove` 会按来源路径同时清理元数据和所有正文记录。调用方应把
/// 一批相关操作交给 [`apply_file_mutations`]，数据库会在一个事务中提交它们。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileMutation {
    Upsert(Box<FileMutationUpsert>),
    Remove { path: PathBuf },
}

/// 一次增量更新的完整载荷。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMutationUpsert {
    pub metadata: FileMetadata,
    pub document: Option<DocumentRecord>,
}

/// 一次增量数据库事务的结果统计。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileMutationSummary {
    pub metadata_upserted: usize,
    pub metadata_removed: usize,
    pub documents_upserted: usize,
    pub documents_removed: usize,
}

/// 一批增量文件更新的原子数据库边界。
///
/// 所有操作成功后才提交事务；元数据、canonical 文档和 FTS trigger 的更新不会
/// 以半个文件操作的形式对后续查询可见。调用方负责控制 `mutations` 的批次大小。
pub fn apply_file_mutations(
    connection: &mut Connection,
    mutations: &[FileMutation],
) -> Result<FileMutationSummary, FileMutationError> {
    if mutations.is_empty() {
        return Ok(FileMutationSummary::default());
    }

    let transaction = connection
        .transaction()
        .map_err(|source| FileMutationError::TransactionBegin { source })?;
    let mut summary = FileMutationSummary::default();

    for mutation in mutations {
        match mutation {
            FileMutation::Upsert(mutation) => {
                let metadata = &mutation.metadata;
                let document = &mutation.document;
                let prepared_metadata = prepare_file_metadata(metadata)
                    .map_err(|source| FileMutationError::Metadata { source })?;

                if let Some(document) = document {
                    let document_path =
                        normalize_path(&document.source_path).map_err(|source| {
                            FileMutationError::Document {
                                source: DocumentStoreError::InvalidSourcePath { source },
                            }
                        })?;
                    let metadata_path = normalize_path(&metadata.path)
                        .map_err(|source| FileMutationError::Metadata { source })?;
                    if document_path != metadata_path {
                        return Err(FileMutationError::PathMismatch);
                    }

                    upsert_document(&transaction, document)
                        .map_err(|source| FileMutationError::Document { source })?;
                    summary.documents_upserted += 1;
                } else {
                    let documents_removed = transaction
                        .execute(
                            "DELETE FROM documents WHERE source_path_key = ?1",
                            params![&prepared_metadata.path_key],
                        )
                        .map_err(|source| FileMutationError::DocumentDelete { source })?;
                    summary.documents_removed += documents_removed;
                }

                execute_file_metadata_upsert(&transaction, metadata, &prepared_metadata)
                    .map_err(|source| FileMutationError::Metadata { source })?;
                summary.metadata_upserted += 1;
            }
            FileMutation::Remove { path } => {
                let path = normalize_path(path)
                    .map_err(|source| FileMutationError::Metadata { source })?;
                let path_key = path_key(&path);
                let metadata_removed = transaction
                    .execute(
                        "DELETE FROM file_metadata WHERE path_key = ?1",
                        params![&path_key],
                    )
                    .map_err(|source| FileMutationError::Metadata {
                        source: FileMetadataError::Query {
                            operation: "delete",
                            source,
                        },
                    })?;
                let documents_removed = transaction
                    .execute(
                        "DELETE FROM documents WHERE source_path_key = ?1",
                        params![&path_key],
                    )
                    .map_err(|source| FileMutationError::DocumentDelete { source })?;
                summary.metadata_removed += metadata_removed;
                summary.documents_removed += documents_removed;
            }
        }
    }

    transaction
        .commit()
        .map_err(|source| FileMutationError::TransactionCommit { source })?;
    Ok(summary)
}

/// 插入或更新一条统一文档记录。
///
/// 当前 M3.0 只持久化本地文件来源。按 `id` upsert；同一来源是否允许多个
/// 文档由调用方的 ID 决定，数据库不提前施加一文件一文档的约束。
pub fn upsert_document(
    connection: &Connection,
    document: &DocumentRecord,
) -> Result<(), DocumentStoreError> {
    let prepared = prepare_document(document)?;

    connection
        .execute(
            "INSERT INTO documents (
                 document_id,
                 source_kind,
                 source_path_key,
                 source_path_display,
                 title,
                 body,
                 line_start,
                 line_end
             ) VALUES (?1, 'local_file', ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(document_id) DO UPDATE SET
                 source_kind = excluded.source_kind,
                 source_path_key = excluded.source_path_key,
                 source_path_display = excluded.source_path_display,
                 title = excluded.title,
                 body = excluded.body,
                 line_start = excluded.line_start,
                 line_end = excluded.line_end",
            params![
                &document.id,
                &prepared.path_key,
                &prepared.path_display,
                &document.title,
                &document.body,
                prepared.line_start,
                prepared.line_end,
            ],
        )
        .map_err(|source| DocumentStoreError::Query {
            operation: "upsert",
            source,
        })?;

    Ok(())
}

/// 按稳定文档 ID读取一条统一文档记录。
pub fn get_document(
    connection: &Connection,
    id: &str,
) -> Result<Option<DocumentRecord>, DocumentStoreError> {
    validate_document_id(id)?;

    let row = connection
        .query_row(
            "SELECT document_id, source_kind, source_path_key, title, body,
                    line_start, line_end
             FROM documents
             WHERE document_id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                ))
            },
        )
        .optional()
        .map_err(|source| DocumentStoreError::Query {
            operation: "get",
            source,
        })?;

    let Some((document_id, source_kind, source_path_key, title, body, line_start, line_end)) = row
    else {
        return Ok(None);
    };

    if source_kind != "local_file" {
        return Err(DocumentStoreError::UnsupportedStoredSource);
    }

    let source_path = path_from_key(&source_path_key)
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or(DocumentStoreError::InvalidStoredPath)?;
    let (line_start, line_end) = decode_stored_location(line_start, line_end)?;
    let document = DocumentRecord {
        id: document_id,
        source_path,
        title,
        body,
        line_start,
        line_end,
    };

    prepare_document(&document)?;
    Ok(Some(document))
}

/// 按稳定文档 ID 顺序读取一批正文记录。
///
/// embedding 重建使用游标式的 `after_document_id`，避免一次把整个文档库加载到
/// 内存。返回的记录仍然经过与单条读取相同的来源、路径和位置校验。
pub fn list_document_batch(
    connection: &Connection,
    after_document_id: Option<&str>,
    limit: usize,
) -> Result<Vec<DocumentRecord>, DocumentStoreError> {
    if limit == 0 {
        return Err(DocumentStoreError::InvalidBatchSize);
    }
    if let Some(after_document_id) = after_document_id {
        validate_document_id(after_document_id)?;
    }

    let limit = i64::try_from(limit).map_err(|_| DocumentStoreError::InvalidBatchSize)?;
    let mut statement = connection
        .prepare(
            "SELECT document_id, source_kind, source_path_key, title, body,
                    line_start, line_end
             FROM documents
             WHERE (?1 IS NULL OR document_id > ?1)
             ORDER BY document_id COLLATE BINARY ASC
             LIMIT ?2",
        )
        .map_err(|source| DocumentStoreError::Query {
            operation: "list_batch_prepare",
            source,
        })?;
    let rows = statement
        .query_map(params![after_document_id, limit], raw_document_from_row)
        .map_err(|source| DocumentStoreError::Query {
            operation: "list_batch_query",
            source,
        })?;

    rows.map(|row| {
        let raw = row.map_err(|source| DocumentStoreError::Query {
            operation: "list_batch_read",
            source,
        })?;
        raw.into_record()
    })
    .collect()
}

/// 读取一个来源路径下的全部 canonical 文档。
///
/// 当前本地文件通常对应一条文档，但数据库模型允许未来一个来源拆成多个文档，
/// 因此这里返回列表，供增量 embedding 刷新完整处理该路径。
pub fn list_documents_for_path<P: AsRef<Path>>(
    connection: &Connection,
    path: P,
) -> Result<Vec<DocumentRecord>, DocumentStoreError> {
    let path = normalize_path(path.as_ref())
        .map_err(|source| DocumentStoreError::InvalidSourcePath { source })?;
    let path_key = path_key(&path);
    let mut statement = connection
        .prepare(
            "SELECT document_id, source_kind, source_path_key, title, body,
                    line_start, line_end
             FROM documents
             WHERE source_path_key = ?1
             ORDER BY document_id COLLATE BINARY ASC",
        )
        .map_err(|source| DocumentStoreError::Query {
            operation: "list_path_prepare",
            source,
        })?;
    let rows = statement
        .query_map(params![path_key], raw_document_from_row)
        .map_err(|source| DocumentStoreError::Query {
            operation: "list_path_query",
            source,
        })?;

    rows.map(|row| {
        let raw = row.map_err(|source| DocumentStoreError::Query {
            operation: "list_path_read",
            source,
        })?;
        raw.into_record()
    })
    .collect()
}

/// 删除一条统一文档记录，返回是否确实删除了记录。
pub fn delete_document(connection: &Connection, id: &str) -> Result<bool, DocumentStoreError> {
    validate_document_id(id)?;

    let deleted = connection
        .execute("DELETE FROM documents WHERE document_id = ?1", params![id])
        .map_err(|source| DocumentStoreError::Query {
            operation: "delete",
            source,
        })?;

    Ok(deleted != 0)
}

struct PreparedDocument {
    path_key: Vec<u8>,
    path_display: String,
    line_start: Option<i64>,
    line_end: Option<i64>,
}

fn prepare_document(document: &DocumentRecord) -> Result<PreparedDocument, DocumentStoreError> {
    validate_document_id(&document.id)?;
    if document.title.trim().is_empty() {
        return Err(DocumentStoreError::EmptyTitle);
    }

    let source_path = normalize_path(&document.source_path)
        .map_err(|source| DocumentStoreError::InvalidSourcePath { source })?;
    let (line_start, line_end) = prepare_location(document.line_start, document.line_end)?;

    Ok(PreparedDocument {
        path_key: path_key(&source_path),
        path_display: source_path.to_string_lossy().into_owned(),
        line_start,
        line_end,
    })
}

fn validate_document_id(id: &str) -> Result<(), DocumentStoreError> {
    if id.trim().is_empty() {
        return Err(DocumentStoreError::EmptyId);
    }

    Ok(())
}

#[derive(Debug)]
struct RawDocument {
    document_id: String,
    source_kind: String,
    source_path_key: Vec<u8>,
    title: String,
    body: String,
    line_start: Option<i64>,
    line_end: Option<i64>,
}

fn raw_document_from_row(row: &Row<'_>) -> rusqlite::Result<RawDocument> {
    Ok(RawDocument {
        document_id: row.get(0)?,
        source_kind: row.get(1)?,
        source_path_key: row.get(2)?,
        title: row.get(3)?,
        body: row.get(4)?,
        line_start: row.get(5)?,
        line_end: row.get(6)?,
    })
}

impl RawDocument {
    fn into_record(self) -> Result<DocumentRecord, DocumentStoreError> {
        if self.source_kind != "local_file" {
            return Err(DocumentStoreError::UnsupportedStoredSource);
        }

        let source_path = path_from_key(&self.source_path_key)
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or(DocumentStoreError::InvalidStoredPath)?;
        let (line_start, line_end) = decode_stored_location(self.line_start, self.line_end)?;
        let document = DocumentRecord {
            id: self.document_id,
            source_path,
            title: self.title,
            body: self.body,
            line_start,
            line_end,
        };
        prepare_document(&document)?;
        Ok(document)
    }
}

fn prepare_location(
    line_start: Option<u64>,
    line_end: Option<u64>,
) -> Result<(Option<i64>, Option<i64>), DocumentStoreError> {
    match (line_start, line_end) {
        (None, None) => Ok((None, None)),
        (Some(line_start), Some(line_end)) if line_start != 0 && line_end >= line_start => Ok((
            Some(
                i64::try_from(line_start)
                    .map_err(|_| DocumentStoreError::LocationOutOfRange { value: line_start })?,
            ),
            Some(
                i64::try_from(line_end)
                    .map_err(|_| DocumentStoreError::LocationOutOfRange { value: line_end })?,
            ),
        )),
        _ => Err(DocumentStoreError::InvalidLocation),
    }
}

fn decode_stored_location(
    line_start: Option<i64>,
    line_end: Option<i64>,
) -> Result<(Option<u64>, Option<u64>), DocumentStoreError> {
    match (line_start, line_end) {
        (None, None) => Ok((None, None)),
        (Some(line_start), Some(line_end)) if line_start > 0 && line_end >= line_start => Ok((
            Some(u64::try_from(line_start).map_err(|_| DocumentStoreError::InvalidStoredLocation)?),
            Some(u64::try_from(line_end).map_err(|_| DocumentStoreError::InvalidStoredLocation)?),
        )),
        _ => Err(DocumentStoreError::InvalidStoredLocation),
    }
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

#[cfg(unix)]
fn path_from_key(key: &[u8]) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;

    Some(PathBuf::from(std::ffi::OsString::from_vec(key.to_vec())))
}

#[cfg(windows)]
fn path_from_key(key: &[u8]) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;

    if !key.len().is_multiple_of(2) {
        return None;
    }

    let wide = key
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| u16::from_le_bytes(*chunk))
        .collect::<Vec<_>>();

    Some(PathBuf::from(std::ffi::OsString::from_wide(&wide)))
}

#[cfg(not(any(unix, windows)))]
fn path_from_key(key: &[u8]) -> Option<PathBuf> {
    String::from_utf8(key.to_vec()).map(PathBuf::from).ok()
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

/// 统一文档持久化错误。
#[derive(Debug)]
pub enum DocumentStoreError {
    EmptyId,
    EmptyTitle,
    InvalidBatchSize,
    InvalidSourcePath {
        source: FileMetadataError,
    },
    InvalidLocation,
    LocationOutOfRange {
        value: u64,
    },
    InvalidStoredPath,
    InvalidStoredLocation,
    UnsupportedStoredSource,
    Query {
        operation: &'static str,
        source: rusqlite::Error,
    },
}

impl DocumentStoreError {
    /// 返回不包含路径、正文或原始 SQLite 信息的安全错误分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::EmptyId => "document_id_invalid",
            Self::EmptyTitle => "document_title_invalid",
            Self::InvalidBatchSize => "document_batch_size_invalid",
            Self::InvalidSourcePath { .. } => "document_source_path_invalid",
            Self::InvalidLocation => "document_location_invalid",
            Self::LocationOutOfRange { .. } => "document_location_out_of_range",
            Self::InvalidStoredPath => "document_store_path_corrupt",
            Self::InvalidStoredLocation => "document_store_location_corrupt",
            Self::UnsupportedStoredSource => "document_source_unsupported",
            Self::Query { operation, .. } => match *operation {
                "upsert" => "document_write",
                "get" => "document_read",
                "delete" => "document_delete",
                _ => "document_query",
            },
        }
    }

    /// 返回可以直接展示给用户的非敏感中文说明。
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::EmptyId => "文档标识不能为空。",
            Self::EmptyTitle => "文档标题不能为空。",
            Self::InvalidBatchSize => "文档读取批次大小必须大于零。",
            Self::InvalidSourcePath { .. } => "文档来源路径无效。",
            Self::InvalidLocation | Self::LocationOutOfRange { .. } => "文档位置范围无效。",
            Self::InvalidStoredPath | Self::InvalidStoredLocation => "文档记录已损坏。",
            Self::UnsupportedStoredSource => "文档来源类型不受支持。",
            Self::Query { .. } => "文档本地存储暂时不可用。",
        }
    }
}

impl fmt::Display for DocumentStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "统一文档持久化失败: {}", self.kind())
    }
}

impl Error for DocumentStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSourcePath { source } => Some(source),
            Self::Query { source, .. } => Some(source),
            Self::EmptyId
            | Self::EmptyTitle
            | Self::InvalidBatchSize
            | Self::InvalidLocation
            | Self::LocationOutOfRange { .. }
            | Self::InvalidStoredPath
            | Self::InvalidStoredLocation
            | Self::UnsupportedStoredSource => None,
        }
    }
}

/// 一批增量文件更新的事务错误。
#[derive(Debug)]
pub enum FileMutationError {
    TransactionBegin { source: rusqlite::Error },
    TransactionCommit { source: rusqlite::Error },
    Metadata { source: FileMetadataError },
    Document { source: DocumentStoreError },
    DocumentDelete { source: rusqlite::Error },
    PathMismatch,
}

impl FileMutationError {
    /// 返回不包含路径、正文或原始数据库错误的安全分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::TransactionBegin { .. } => "file_mutation_transaction_begin",
            Self::TransactionCommit { .. } => "file_mutation_transaction_commit",
            Self::Metadata { .. } => "file_mutation_metadata",
            Self::Document { .. } => "file_mutation_document",
            Self::DocumentDelete { .. } => "file_mutation_document_delete",
            Self::PathMismatch => "file_mutation_path_mismatch",
        }
    }

    /// 返回可以直接展示给用户的非敏感中文说明。
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::TransactionBegin { .. }
            | Self::TransactionCommit { .. }
            | Self::Metadata { .. }
            | Self::Document { .. }
            | Self::DocumentDelete { .. } => "增量索引暂时无法保存。",
            Self::PathMismatch => "增量索引收到不一致的文件路径。",
        }
    }
}

impl fmt::Display for FileMutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "增量文件更新失败: {}", self.kind())
    }
}

impl Error for FileMutationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::TransactionBegin { source } | Self::TransactionCommit { source } => Some(source),
            Self::Metadata { source } => Some(source),
            Self::Document { source } => Some(source),
            Self::DocumentDelete { source } => Some(source),
            Self::PathMismatch => None,
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
        apply_file_mutations, begin_file_metadata_rescan, delete_document, get_document,
        get_file_metadata, initialize_database, list_document_batch, list_documents_for_path,
        normalize_path, upsert_document, upsert_file_metadata, upsert_file_metadata_batch,
        DatabaseError, DocumentRecord, DocumentStoreError, FileMetadata, FileMetadataError,
        FileMutation, FileMutationError, FileMutationUpsert, CURRENT_SCHEMA_VERSION,
        DOCUMENTS_MIGRATION_SQL, FILE_METADATA_MIGRATION_SQL, FOUNDATION_MIGRATION_SQL,
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

    fn trigger_exists(connection: &Connection, trigger_name: &str) -> bool {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'trigger' AND name = ?1)",
                [trigger_name],
                |row| row.get(0),
            )
            .expect("检查测试触发器失败")
    }

    fn matching_document_ids(connection: &Connection, query: &str) -> Vec<String> {
        let mut statement = connection
            .prepare(
                "SELECT documents.document_id
                 FROM documents_fts
                 JOIN documents ON documents.rowid = documents_fts.rowid
                 WHERE documents_fts MATCH ?1
                 ORDER BY documents.document_id",
            )
            .expect("准备 FTS 测试查询失败");

        statement
            .query_map([query], |row| row.get(0))
            .expect("执行 FTS 测试查询失败")
            .map(|row| row.expect("读取 FTS 测试结果失败"))
            .collect()
    }

    fn assert_fts_integrity(connection: &Connection) {
        connection
            .execute(
                "INSERT INTO documents_fts (documents_fts) VALUES ('integrity-check')",
                [],
            )
            .expect("FTS integrity-check 失败");
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
        assert!(table_exists(&connection, "documents"));
        assert!(table_exists(&connection, "documents_fts"));
        assert!(table_exists(&connection, "embedding_models"));
        assert!(table_exists(&connection, "document_embeddings"));
        assert!(trigger_exists(&connection, "documents_fts_after_insert"));
        assert!(trigger_exists(&connection, "documents_fts_after_delete"));
        assert!(trigger_exists(&connection, "documents_fts_after_update"));
        assert!(trigger_exists(
            &connection,
            "documents_embeddings_after_delete"
        ));
        assert!(trigger_exists(
            &connection,
            "documents_embeddings_after_content_update"
        ));
        let fts_schema: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'documents_fts'",
                [],
                |row| row.get(0),
            )
            .expect("读取 FTS schema 测试定义失败");
        assert!(fts_schema.contains("unicode61"));
        assert_fts_integrity(&connection);
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
        assert!(table_exists(&connection, "documents"));
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
    fn migrates_v2_database_to_current_schema_without_losing_existing_records() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();

        let connection = Connection::open(&database_path).expect("创建 v2 迁移测试数据库失败");
        connection
            .execute_batch(FOUNDATION_MIGRATION_SQL)
            .expect("创建 v2 基础 schema 失败");
        connection
            .execute_batch(FILE_METADATA_MIGRATION_SQL)
            .expect("创建 v2 文件元数据 schema 失败");
        connection
            .pragma_update(None, "user_version", 2)
            .expect("设置 v2 schema 版本失败");
        connection
            .execute(
                "INSERT INTO nexus_metadata (key, value) VALUES (?1, ?2)",
                params!["before_m3", "preserved"],
            )
            .expect("写入 v2 迁移元数据失败");
        drop(connection);

        let connection = initialize_database(&database_path).expect("执行 v2 到 v3 迁移失败");

        assert_eq!(
            schema_version(&connection),
            i64::from(CURRENT_SCHEMA_VERSION)
        );
        assert!(table_exists(&connection, "documents"));
        let value: String = connection
            .query_row(
                "SELECT value FROM nexus_metadata WHERE key = ?1",
                ["before_m3"],
                |row| row.get(0),
            )
            .expect("读取 v3 迁移后的元数据失败");
        assert_eq!(value, "preserved");
    }

    #[test]
    fn rebuilds_existing_documents_when_migrating_to_fts_schema() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let document = DocumentRecord {
            id: "file:before-fts".to_owned(),
            source_path: temporary_directory.child_path("before-fts.md"),
            title: "Before FTS".to_owned(),
            body: "legacy searchable content".to_owned(),
            line_start: None,
            line_end: None,
        };

        let connection = Connection::open(&database_path).expect("创建 v3 FTS 迁移数据库失败");
        connection
            .execute_batch(FOUNDATION_MIGRATION_SQL)
            .expect("创建 v3 基础 schema 失败");
        connection
            .execute_batch(FILE_METADATA_MIGRATION_SQL)
            .expect("创建 v3 文件元数据 schema 失败");
        connection
            .execute_batch(DOCUMENTS_MIGRATION_SQL)
            .expect("创建 v3 文档 schema 失败");
        connection
            .pragma_update(None, "user_version", 3)
            .expect("设置 v3 schema 版本失败");
        upsert_document(&connection, &document).expect("写入 v3 既有文档失败");
        drop(connection);

        let connection = initialize_database(&database_path).expect("执行 v3 到 v4 迁移失败");

        assert_eq!(
            matching_document_ids(&connection, "legacy"),
            vec![document.id]
        );
        assert_fts_integrity(&connection);
    }

    #[test]
    fn keeps_fts_in_sync_for_document_insert_update_and_delete() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let connection =
            initialize_database(&database_path).expect("初始化 FTS 同步测试数据库失败");
        let document = DocumentRecord {
            id: "file:sync".to_owned(),
            source_path: temporary_directory.child_path("sync.md"),
            title: "Alpha title".to_owned(),
            body: "alpha body".to_owned(),
            line_start: None,
            line_end: None,
        };

        upsert_document(&connection, &document).expect("写入 FTS 同步测试文档失败");
        assert_eq!(
            matching_document_ids(&connection, "alpha"),
            vec![document.id.clone()]
        );

        let updated = DocumentRecord {
            title: "Beta title".to_owned(),
            body: "beta body".to_owned(),
            ..document.clone()
        };
        upsert_document(&connection, &updated).expect("更新 FTS 同步测试文档失败");
        assert!(matching_document_ids(&connection, "alpha").is_empty());
        assert_eq!(
            matching_document_ids(&connection, "beta"),
            vec![document.id.clone()]
        );

        assert!(delete_document(&connection, &document.id).expect("删除 FTS 同步测试文档失败"));
        assert!(matching_document_ids(&connection, "beta").is_empty());
        assert_fts_integrity(&connection);
    }

    #[test]
    fn lists_documents_in_stable_batches_and_by_source_path() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let connection = initialize_database(&database_path).expect("初始化文档列表测试数据库失败");
        let shared_path = temporary_directory.child_path("shared.md");
        let documents = [
            DocumentRecord {
                id: "file:b".to_owned(),
                source_path: temporary_directory.child_path("other.md"),
                title: "B".to_owned(),
                body: "body b".to_owned(),
                line_start: None,
                line_end: None,
            },
            DocumentRecord {
                id: "file:a".to_owned(),
                source_path: shared_path.clone(),
                title: "A".to_owned(),
                body: "body a".to_owned(),
                line_start: None,
                line_end: None,
            },
            DocumentRecord {
                id: "file:c".to_owned(),
                source_path: shared_path.clone(),
                title: "C".to_owned(),
                body: "body c".to_owned(),
                line_start: None,
                line_end: None,
            },
        ];
        for document in &documents {
            upsert_document(&connection, document).expect("写入文档列表测试记录失败");
        }

        let first_batch = list_document_batch(&connection, None, 2).expect("读取第一批文档失败");
        assert_eq!(
            first_batch
                .iter()
                .map(|document| document.id.as_str())
                .collect::<Vec<_>>(),
            vec!["file:a", "file:b"]
        );
        let second_batch =
            list_document_batch(&connection, Some("file:b"), 2).expect("读取第二批文档失败");
        assert_eq!(
            second_batch
                .iter()
                .map(|document| document.id.as_str())
                .collect::<Vec<_>>(),
            vec!["file:c"]
        );

        let source_documents =
            list_documents_for_path(&connection, &shared_path).expect("按来源路径读取文档失败");
        assert_eq!(
            source_documents
                .iter()
                .map(|document| document.id.as_str())
                .collect::<Vec<_>>(),
            vec!["file:a", "file:c"]
        );
        assert!(matches!(
            list_document_batch(&connection, None, 0),
            Err(DocumentStoreError::InvalidBatchSize)
        ));
    }

    #[test]
    fn rolls_back_document_and_fts_changes_together() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let mut connection =
            initialize_database(&database_path).expect("初始化 FTS 回滚测试数据库失败");
        let document = DocumentRecord {
            id: "file:rollback".to_owned(),
            source_path: temporary_directory.child_path("rollback.md"),
            title: "Rollback title".to_owned(),
            body: "rollback body".to_owned(),
            line_start: None,
            line_end: None,
        };

        {
            let transaction = connection.transaction().expect("开始 FTS 回滚测试事务失败");
            upsert_document(&transaction, &document).expect("事务内写入 FTS 测试文档失败");
        }

        assert!(get_document(&connection, &document.id)
            .expect("读取回滚后的文档失败")
            .is_none());
        assert!(matching_document_ids(&connection, "rollback").is_empty());
        assert_fts_integrity(&connection);
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
    fn stores_updates_reads_and_deletes_document_records() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let connection = initialize_database(&database_path).expect("初始化文档数据库失败");
        let document_path = temporary_directory.child_path("notes/meeting.md");

        let document = DocumentRecord {
            id: "file:meeting".to_owned(),
            source_path: document_path.clone(),
            title: "Meeting".to_owned(),
            body: "first body".to_owned(),
            line_start: Some(2),
            line_end: Some(5),
        };
        upsert_document(&connection, &document).expect("首次写入文档失败");

        let loaded = get_document(&connection, &document.id)
            .expect("读取文档失败")
            .expect("找不到刚写入的文档");
        assert_eq!(loaded, document);

        let updated = DocumentRecord {
            title: "Updated meeting".to_owned(),
            body: "updated body".to_owned(),
            line_start: None,
            line_end: None,
            ..document.clone()
        };
        upsert_document(&connection, &updated).expect("更新文档失败");

        let loaded = get_document(&connection, &document.id)
            .expect("读取更新后的文档失败")
            .expect("找不到更新后的文档");
        assert_eq!(loaded, updated);

        assert!(delete_document(&connection, &document.id).expect("删除文档失败"));
        assert!(get_document(&connection, &document.id)
            .expect("读取已删除文档失败")
            .is_none());
        assert!(!delete_document(&connection, &document.id).expect("重复删除文档失败"));
    }

    #[test]
    fn rejects_invalid_document_records_without_database_write() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let connection = initialize_database(&database_path).expect("初始化文档边界测试数据库失败");
        let source_path = temporary_directory.child_path("notes/meeting.md");

        let cases = [
            (
                DocumentRecord {
                    id: "  ".to_owned(),
                    source_path: source_path.clone(),
                    title: "Meeting".to_owned(),
                    body: "secret body".to_owned(),
                    line_start: None,
                    line_end: None,
                },
                "document_id_invalid",
            ),
            (
                DocumentRecord {
                    id: "file:empty-title".to_owned(),
                    source_path: source_path.clone(),
                    title: "\t".to_owned(),
                    body: String::new(),
                    line_start: None,
                    line_end: None,
                },
                "document_title_invalid",
            ),
            (
                DocumentRecord {
                    id: "file:invalid-location".to_owned(),
                    source_path: source_path.clone(),
                    title: "Meeting".to_owned(),
                    body: String::new(),
                    line_start: Some(5),
                    line_end: Some(2),
                },
                "document_location_invalid",
            ),
            (
                DocumentRecord {
                    id: "file:out-of-range".to_owned(),
                    source_path: source_path.clone(),
                    title: "Meeting".to_owned(),
                    body: String::new(),
                    line_start: Some(u64::MAX),
                    line_end: Some(u64::MAX),
                },
                "document_location_out_of_range",
            ),
            (
                DocumentRecord {
                    id: "file:empty-path".to_owned(),
                    source_path: PathBuf::new(),
                    title: "Meeting".to_owned(),
                    body: String::new(),
                    line_start: None,
                    line_end: None,
                },
                "document_source_path_invalid",
            ),
        ];

        for (document, expected_kind) in cases {
            let error =
                upsert_document(&connection, &document).expect_err("无效文档不应写入数据库");
            assert_eq!(error.kind(), expected_kind);
            assert!(!error.user_message().is_empty());
            assert!(!error.to_string().contains("secret body"));
            assert!(!error.to_string().contains("meeting.md"));
        }

        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
            .expect("统计无效文档写入结果失败");
        assert_eq!(count, 0);
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
    fn applies_file_mutations_as_one_metadata_document_and_fts_transaction() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let mut connection = initialize_database(&database_path).expect("初始化增量事务数据库失败");
        let metadata = synthetic_metadata(&temporary_directory.path, 0);
        let document = DocumentRecord {
            id: "file:incremental-transaction".to_owned(),
            source_path: metadata.path.clone(),
            title: "Incremental note".to_owned(),
            body: "transaction body".to_owned(),
            line_start: None,
            line_end: None,
        };

        let summary = apply_file_mutations(
            &mut connection,
            &[FileMutation::Upsert(Box::new(FileMutationUpsert {
                metadata: metadata.clone(),
                document: Some(document),
            }))],
        )
        .expect("提交增量事务失败");

        assert_eq!(summary.metadata_upserted, 1);
        assert_eq!(summary.documents_upserted, 1);
        assert!(get_file_metadata(&connection, &metadata.path)
            .expect("读取增量事务元数据失败")
            .is_some());
        let fts_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM documents_fts WHERE documents_fts MATCH ?1",
                ["transaction"],
                |row| row.get(0),
            )
            .expect("检查增量事务 FTS 结果失败");
        assert_eq!(fts_count, 1);

        let summary = apply_file_mutations(
            &mut connection,
            &[FileMutation::Remove {
                path: metadata.path.clone(),
            }],
        )
        .expect("删除增量事务记录失败");
        assert_eq!(summary.metadata_removed, 1);
        assert_eq!(summary.documents_removed, 1);
        assert!(get_document(&connection, "file:incremental-transaction")
            .expect("读取删除后的增量文档失败")
            .is_none());
    }

    #[test]
    fn rolls_back_the_whole_file_mutation_batch_on_a_later_error() {
        let temporary_directory = TemporaryDirectory::new();
        let database_path = temporary_directory.database_path();
        let mut connection = initialize_database(&database_path).expect("初始化回滚事务数据库失败");
        let existing = synthetic_metadata(&temporary_directory.path, 0);
        let added = synthetic_metadata(&temporary_directory.path, 1);
        upsert_file_metadata(&connection, &existing).expect("写入回滚事务既有元数据失败");

        let error = apply_file_mutations(
            &mut connection,
            &[
                FileMutation::Upsert(Box::new(FileMutationUpsert {
                    metadata: added.clone(),
                    document: None,
                })),
                FileMutation::Upsert(Box::new(FileMutationUpsert {
                    metadata: existing.clone(),
                    document: Some(DocumentRecord {
                        id: "file:invalid-incremental".to_owned(),
                        source_path: existing.path.clone(),
                        title: String::new(),
                        body: "must roll back".to_owned(),
                        line_start: None,
                        line_end: None,
                    }),
                })),
            ],
        )
        .expect_err("后续文档错误不应提交前一条增量操作");

        assert!(matches!(error, FileMutationError::Document { .. }));
        assert!(get_file_metadata(&connection, &added.path)
            .expect("读取回滚后的新增元数据失败")
            .is_none());
        assert!(get_file_metadata(&connection, &existing.path)
            .expect("读取回滚后的既有元数据失败")
            .is_some());
        assert!(get_document(&connection, "file:invalid-incremental")
            .expect("读取回滚后的无效文档失败")
            .is_none());
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
