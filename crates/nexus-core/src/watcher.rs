//! M4.1 文件事件来源适配。
//!
//! 本模块把操作系统文件通知转换为 Nexus 自己的事件类型。它只负责监听、路径范围
//! 过滤和事件归一化，不负责读取文件正文、判断内容是否稳定或写入索引数据库。

use std::{
    error::Error as StdError,
    fmt,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    time::Duration,
};

use nexus_db::{normalize_path, FileMetadataError};
use notify::{
    event::{ModifyKind, RenameMode},
    Event, EventKind, RecursiveMode, Watcher,
};

/// 已归一化的本地文件事件。
///
/// 事件路径可能指向文件或目录；后续消费者需要重新读取文件系统状态后，才能决定
/// 是否解析正文。`RescanRequired` 表示底层事件源报告可能丢失了事件，应回到一次
/// 完整扫描，而不是只依赖当前事件列表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileEvent {
    /// 新路径出现在监听范围内。
    Created { path: PathBuf },
    /// 路径对应的内容或元数据发生变化。
    Modified { path: PathBuf },
    /// 路径从监听范围内消失。
    Removed { path: PathBuf },
    /// 路径在监听范围内完成重命名或移动。
    Renamed { from: PathBuf, to: PathBuf },
    /// 底层事件源无法保证事件完整，需要重新扫描监听根目录。
    RescanRequired { root: PathBuf },
}

/// 启动或消费文件监听时的错误。
#[derive(Debug)]
pub enum FileWatchError {
    /// 监听根目录路径无效。
    InvalidRoot { source: FileMetadataError },
    /// 底层 watcher 无法启动或无法添加监听目录。
    Start { source: notify::Error },
    /// 底层 watcher 报告运行期间错误。
    Event { source: notify::Error },
    /// 事件携带的路径无法规范化。
    EventPath { source: FileMetadataError },
    /// 监听事件通道已经关闭。
    ChannelClosed,
}

impl FileWatchError {
    /// 返回不包含路径或底层错误文本的安全错误分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidRoot { .. } => "file_watch_root_invalid",
            Self::Start { .. } => "file_watch_start",
            Self::Event { .. } => "file_watch_event",
            Self::EventPath { .. } => "file_watch_event_path",
            Self::ChannelClosed => "file_watch_channel_closed",
        }
    }

    /// 返回可以直接展示给用户的非敏感中文说明。
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::InvalidRoot { .. } => "文件监听目录无效。",
            Self::Start { .. } => "无法启动文件监听。",
            Self::Event { .. } => "文件监听暂时不可用。",
            Self::EventPath { .. } => "文件监听收到无效路径。",
            Self::ChannelClosed => "文件监听已关闭。",
        }
    }
}

impl fmt::Display for FileWatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "文件监听失败: {}", self.kind())
    }
}

impl StdError for FileWatchError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::InvalidRoot { source } | Self::EventPath { source } => Some(source),
            Self::Start { source } | Self::Event { source } => Some(source),
            Self::ChannelClosed => None,
        }
    }
}

/// 一个递归监听根目录的文件事件来源。
///
/// `FileWatcher` 持有底层 watcher，因此只要该值存活，监听就会继续。丢弃它会停止
/// 监听；本单元不把监听线程绑定到扫描、解析或数据库生命周期。
pub struct FileWatcher {
    root: PathBuf,
    receiver: Receiver<Result<FileEvent, FileWatchError>>,
    _watcher: notify::RecommendedWatcher,
}

/// 开始递归监听指定目录，并返回与索引逻辑解耦的事件来源。
pub fn watch_directory<P: AsRef<Path>>(root: P) -> Result<FileWatcher, FileWatchError> {
    let root =
        normalize_path(root.as_ref()).map_err(|source| FileWatchError::InvalidRoot { source })?;
    let (sender, receiver) = mpsc::channel();
    let mut normalizer = EventNormalizer::new(root.clone());

    let mut watcher =
        notify::recommended_watcher(move |result: notify::Result<Event>| match result {
            Ok(event) => match normalizer.ingest(event) {
                Ok(events) => {
                    for event in events {
                        if sender.send(Ok(event)).is_err() {
                            break;
                        }
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(error));
                }
            },
            Err(source) => {
                let _ = sender.send(Err(FileWatchError::Event { source }));
            }
        })
        .map_err(|source| FileWatchError::Start { source })?;

    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|source| FileWatchError::Start { source })?;

    Ok(FileWatcher {
        root,
        receiver,
        _watcher: watcher,
    })
}

impl FileWatcher {
    /// 返回监听根目录的规范化路径。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 在指定时限内等待一个归一化事件。
    ///
    /// `Ok(None)` 表示时限内没有事件；底层事件错误会以 `Err` 返回，调用方可以
    /// 记录安全分类后继续等待后续事件。
    pub fn recv_timeout(&self, timeout: Duration) -> Result<Option<FileEvent>, FileWatchError> {
        match self.receiver.recv_timeout(timeout) {
            Ok(Ok(event)) => Ok(Some(event)),
            Ok(Err(error)) => Err(error),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(FileWatchError::ChannelClosed),
        }
    }

    /// 非阻塞地读取一个归一化事件。
    pub fn try_recv(&self) -> Result<Option<FileEvent>, FileWatchError> {
        match self.receiver.try_recv() {
            Ok(Ok(event)) => Ok(Some(event)),
            Ok(Err(error)) => Err(error),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(FileWatchError::ChannelClosed),
        }
    }
}

struct EventNormalizer {
    root: PathBuf,
    pending_rename_from: Option<PathBuf>,
}

impl EventNormalizer {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            pending_rename_from: None,
        }
    }

    fn ingest(&mut self, event: Event) -> Result<Vec<FileEvent>, FileWatchError> {
        if event.need_rescan() {
            self.pending_rename_from = None;
            return Ok(vec![FileEvent::RescanRequired {
                root: self.root.clone(),
            }]);
        }

        match event.kind {
            EventKind::Create(_) => {
                self.simple_events(event.paths, |path| FileEvent::Created { path })
            }
            EventKind::Remove(_) => {
                self.simple_events(event.paths, |path| FileEvent::Removed { path })
            }
            EventKind::Modify(ModifyKind::Name(mode)) => self.rename_events(mode, event.paths),
            EventKind::Modify(_) | EventKind::Any => {
                self.simple_events(event.paths, |path| FileEvent::Modified { path })
            }
            EventKind::Access(_) | EventKind::Other => Ok(self.flush_pending_rename()),
        }
    }

    fn simple_events<F>(
        &mut self,
        paths: Vec<PathBuf>,
        build: F,
    ) -> Result<Vec<FileEvent>, FileWatchError>
    where
        F: Fn(PathBuf) -> FileEvent,
    {
        let mut events = self.flush_pending_rename();
        events.extend(
            self.normalize_event_paths(paths)?
                .into_iter()
                .flatten()
                .map(build),
        );
        Ok(events)
    }

    fn rename_events(
        &mut self,
        mode: RenameMode,
        paths: Vec<PathBuf>,
    ) -> Result<Vec<FileEvent>, FileWatchError> {
        match mode {
            RenameMode::From => {
                let mut events = self.flush_pending_rename();
                let normalized = self.normalize_event_paths(paths)?;
                let mut paths = normalized.into_iter();

                if let Some(Some(path)) = paths.next() {
                    self.pending_rename_from = Some(path);
                }

                events.extend(paths.flatten().map(|path| FileEvent::Removed { path }));
                Ok(events)
            }
            RenameMode::To => {
                let normalized = self.normalize_event_paths(paths)?;
                let mut events = Vec::new();
                let first = normalized.first().cloned().unwrap_or(None);

                if let Some(from) = self.pending_rename_from.take() {
                    events.extend(rename_boundary(Some(from), first));
                } else if let Some(to) = first {
                    events.push(FileEvent::Created { path: to });
                }

                events.extend(
                    normalized
                        .into_iter()
                        .skip(1)
                        .flatten()
                        .map(|path| FileEvent::Created { path }),
                );
                Ok(events)
            }
            RenameMode::Both | RenameMode::Any => {
                let mut events = self.flush_pending_rename();
                let normalized = self.normalize_event_paths(paths)?;

                if normalized.len() >= 2 {
                    events.extend(rename_boundary(
                        normalized[0].clone(),
                        normalized[1].clone(),
                    ));
                    events.extend(
                        normalized
                            .into_iter()
                            .skip(2)
                            .flatten()
                            .map(|path| FileEvent::Modified { path }),
                    );
                } else {
                    events.extend(
                        normalized
                            .into_iter()
                            .flatten()
                            .map(|path| FileEvent::Modified { path }),
                    );
                }

                Ok(events)
            }
            RenameMode::Other => Ok(self.flush_pending_rename()),
        }
    }

    fn normalize_event_paths(
        &self,
        paths: Vec<PathBuf>,
    ) -> Result<Vec<Option<PathBuf>>, FileWatchError> {
        paths
            .into_iter()
            .map(|path| self.normalize_event_path(&path))
            .collect()
    }

    fn normalize_event_path(&self, path: &Path) -> Result<Option<PathBuf>, FileWatchError> {
        let path = normalize_path(path).map_err(|source| FileWatchError::EventPath { source })?;
        if path == self.root || path.starts_with(&self.root) {
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    fn flush_pending_rename(&mut self) -> Vec<FileEvent> {
        self.pending_rename_from
            .take()
            .map(|path| vec![FileEvent::Removed { path }])
            .unwrap_or_default()
    }
}

fn rename_boundary(from: Option<PathBuf>, to: Option<PathBuf>) -> Vec<FileEvent> {
    match (from, to) {
        (Some(from), Some(to)) => vec![FileEvent::Renamed { from, to }],
        (Some(path), None) => vec![FileEvent::Removed { path }],
        (None, Some(path)) => vec![FileEvent::Created { path }],
        (None, None) => Vec::new(),
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

    use notify::{
        event::{CreateKind, Flag, ModifyKind, RemoveKind, RenameMode},
        Event, EventKind,
    };

    use super::{watch_directory, EventNormalizer, FileEvent, FileWatchError};

    static TEMP_DIRECTORY_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TemporaryDirectory {
        path: PathBuf,
    }

    impl TemporaryDirectory {
        fn new() -> Self {
            let sequence = TEMP_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                env::temp_dir().join(format!("nexus-watcher-test-{}-{}", process::id(), sequence));
            fs::create_dir_all(&path).expect("创建文件监听测试目录失败");
            Self { path }
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

    fn normalizer(root: &Path) -> EventNormalizer {
        EventNormalizer::new(root.to_path_buf())
    }

    #[test]
    fn normalizes_create_modify_and_remove_events() {
        let temporary_directory = TemporaryDirectory::new();
        let root = &temporary_directory.path;
        let path = temporary_directory.child_path("notes.txt");
        let mut normalizer = normalizer(root);

        assert_eq!(
            normalizer
                .ingest(Event::new(EventKind::Create(CreateKind::File)).add_path(path.clone()))
                .expect("归一化创建事件失败"),
            vec![FileEvent::Created { path: path.clone() }]
        );
        assert_eq!(
            normalizer
                .ingest(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(path.clone()))
                .expect("归一化修改事件失败"),
            vec![FileEvent::Modified { path: path.clone() }]
        );
        assert_eq!(
            normalizer
                .ingest(Event::new(EventKind::Remove(RemoveKind::File)).add_path(path.clone()))
                .expect("归一化删除事件失败"),
            vec![FileEvent::Removed { path }]
        );
    }

    #[test]
    fn pairs_both_and_separate_rename_events() {
        let temporary_directory = TemporaryDirectory::new();
        let root = &temporary_directory.path;
        let from = temporary_directory.child_path("old.txt");
        let to = temporary_directory.child_path("new.txt");

        let mut both_normalizer = normalizer(root);
        assert_eq!(
            both_normalizer
                .ingest(
                    Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                        .add_path(from.clone())
                        .add_path(to.clone())
                )
                .expect("归一化双端重命名事件失败"),
            vec![FileEvent::Renamed {
                from: from.clone(),
                to: to.clone()
            }]
        );

        let mut split_normalizer = normalizer(root);
        assert!(split_normalizer
            .ingest(
                Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From)))
                    .add_path(from.clone())
            )
            .expect("归一化重命名起点事件失败")
            .is_empty());
        assert_eq!(
            split_normalizer
                .ingest(
                    Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::To)))
                        .add_path(to.clone())
                )
                .expect("归一化重命名终点事件失败"),
            vec![FileEvent::Renamed { from, to }]
        );
    }

    #[test]
    fn converts_moves_across_watch_boundary_to_remove_or_create() {
        let temporary_directory = TemporaryDirectory::new();
        let root = &temporary_directory.path;
        let outside = env::temp_dir().join("nexus-watcher-outside.txt");
        let inside = temporary_directory.child_path("inside.txt");
        let mut normalizer = normalizer(root);

        assert_eq!(
            normalizer
                .ingest(
                    Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                        .add_path(inside.clone())
                        .add_path(outside.clone())
                )
                .expect("归一化移出监听范围事件失败"),
            vec![FileEvent::Removed {
                path: inside.clone()
            }]
        );
        assert_eq!(
            normalizer
                .ingest(
                    Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                        .add_path(outside)
                        .add_path(inside.clone())
                )
                .expect("归一化移入监听范围事件失败"),
            vec![FileEvent::Created { path: inside }]
        );
    }

    #[test]
    fn emits_rescan_required_for_a_loss_signal_and_drops_pending_rename() {
        let temporary_directory = TemporaryDirectory::new();
        let root = &temporary_directory.path;
        let path = temporary_directory.child_path("pending.txt");
        let mut normalizer = normalizer(root);

        assert!(normalizer
            .ingest(
                Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From))).add_path(path)
            )
            .expect("记录待配对重命名失败")
            .is_empty());

        let event = Event::new(EventKind::Any).set_flag(Flag::Rescan);
        assert_eq!(
            normalizer.ingest(event).expect("归一化重新扫描信号失败"),
            vec![FileEvent::RescanRequired {
                root: root.to_path_buf()
            }]
        );
    }

    #[test]
    fn ignores_events_outside_root_and_non_mutating_events() {
        let temporary_directory = TemporaryDirectory::new();
        let root = &temporary_directory.path;
        let outside = env::temp_dir().join("nexus-watcher-outside.txt");
        let mut normalizer = normalizer(root);

        assert!(normalizer
            .ingest(Event::new(EventKind::Create(CreateKind::File)).add_path(outside))
            .expect("过滤监听范围外事件失败")
            .is_empty());
        assert!(normalizer
            .ingest(Event::new(EventKind::Access(
                notify::event::AccessKind::Any
            )))
            .expect("处理访问事件失败")
            .is_empty());
    }

    #[test]
    fn rejects_an_empty_watch_root_without_exposing_path() {
        let error = match watch_directory(PathBuf::new()) {
            Ok(_) => panic!("空监听路径不应成功"),
            Err(error) => error,
        };

        assert!(matches!(error, FileWatchError::InvalidRoot { .. }));
        assert_eq!(error.kind(), "file_watch_root_invalid");
        assert_eq!(error.user_message(), "文件监听目录无效。");
    }

    #[test]
    fn receives_a_real_created_file_event() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("created.txt");
        let watcher = watch_directory(&temporary_directory.path).expect("启动文件监听失败");
        let expected_path = watcher.root().join("created.txt");

        fs::write(&path, b"watcher event").expect("创建监听测试文件失败");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut observed = false;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let Some(event) = watcher
                .recv_timeout(remaining)
                .expect("读取文件监听事件失败")
            else {
                break;
            };

            if matches!(
                event,
                FileEvent::Created { path } if path == expected_path
            ) {
                observed = true;
                break;
            }
        }

        assert!(observed, "文件监听未在时限内报告创建事件");
    }
}
