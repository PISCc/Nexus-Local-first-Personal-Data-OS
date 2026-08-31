//! Nexus 本地优先核心边界。
//!
//! 核心层负责组织本地数据基础设施的初始化，但不依赖 Tauri 或前端。
//! 平台层只负责提供路径、记录安全状态，并把结果传给界面。

#![forbid(unsafe_code)]

use std::{error::Error, fmt, path::Path};

use nexus_db::{initialize_database, DatabaseError};

/// 初始化 Nexus 本地核心。
///
/// 数据库连接在本次启动检查结束后由调用方释放；后续里程碑再决定运行时
/// 数据库服务的持有方式。这里保留核心层的初始化边界，避免 UI 直接操作数据库。
pub fn initialize<P: AsRef<Path>>(database_path: P) -> Result<(), CoreError> {
    let _connection = initialize_database(database_path).map_err(CoreError::Database)?;
    Ok(())
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
        path::PathBuf,
        process,
        sync::atomic::{AtomicUsize, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use nexus_db::DatabaseError;

    use super::{initialize, CoreError};

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
}
