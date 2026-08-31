//! Nexus 本地数据库边界。
//!
//! M0.3 只建立 crate 边界，不在这里实现 SQLite 连接、迁移或业务模型。
//! M0.4 提供最小的 SQLite 初始化和迁移入口。

#![forbid(unsafe_code)]

use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

use rusqlite::Connection;

const FOUNDATION_MIGRATION_SQL: &str = include_str!("../migrations/0001_foundation.sql");

/// 当前数据库 schema 版本。
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

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

    match version {
        0 => {
            apply_foundation_migration(&mut connection, &path)?;
            Ok(connection)
        }
        CURRENT_SCHEMA_VERSION => Ok(connection),
        found => Err(DatabaseError::UnsupportedSchemaVersion {
            path,
            found,
            supported: CURRENT_SCHEMA_VERSION,
        }),
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

fn apply_foundation_migration(
    connection: &mut Connection,
    path: &Path,
) -> Result<(), DatabaseError> {
    let transaction = connection
        .transaction()
        .map_err(|source| DatabaseError::Migration {
            path: path.to_path_buf(),
            version: CURRENT_SCHEMA_VERSION,
            source,
        })?;

    transaction
        .execute_batch(FOUNDATION_MIGRATION_SQL)
        .map_err(|source| DatabaseError::Migration {
            path: path.to_path_buf(),
            version: CURRENT_SCHEMA_VERSION,
            source,
        })?;

    transaction
        .pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
        .map_err(|source| DatabaseError::Migration {
            path: path.to_path_buf(),
            version: CURRENT_SCHEMA_VERSION,
            source,
        })?;

    transaction
        .commit()
        .map_err(|source| DatabaseError::Migration {
            path: path.to_path_buf(),
            version: CURRENT_SCHEMA_VERSION,
            source,
        })
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
        path::PathBuf,
        process,
        sync::atomic::{AtomicUsize, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use rusqlite::{params, Connection};

    use super::{initialize_database, DatabaseError, CURRENT_SCHEMA_VERSION};

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
        assert!(!table_exists(&connection, "file_index"));
        assert!(database_path.is_file());
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
}
