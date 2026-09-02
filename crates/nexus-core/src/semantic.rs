//! 文档 embedding 索引编排。
//!
//! 本模块把 `DocumentRecord` 交给 embedding provider，再按有界批次写入 `nexus-db`。
//! 它不读取原始文件、不访问网络，也不把 embedding 逻辑放进 UI。完整重建使用
//! 稳定文档 ID 游标，路径刷新只处理受影响来源，便于 M4 的初始索引和增量索引复用。

use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

use nexus_db::{
    initialize_database, list_document_batch, list_documents_for_path, upsert_document_embeddings,
    DocumentEmbedding, DocumentRecord, EmbeddingModel, EmbeddingStoreError,
};

use crate::{document_input_fingerprint, EmbeddingError, EmbeddingProvider, RescanControl};

/// 默认的 embedding 写入批次大小。
pub const DEFAULT_EMBEDDING_BATCH_SIZE: usize = 128;

/// embedding 索引任务参数。
#[derive(Debug, Clone, Copy)]
pub struct EmbeddingIndexOptions {
    pub batch_size: usize,
}

impl Default for EmbeddingIndexOptions {
    fn default() -> Self {
        Self {
            batch_size: DEFAULT_EMBEDDING_BATCH_SIZE,
        }
    }
}

/// 一次完整或局部 embedding 刷新的统计。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmbeddingIndexSummary {
    pub documents_seen: usize,
    pub embeddings_written: usize,
    pub documents_failed: usize,
    pub batches: usize,
}

/// embedding 索引任务错误。
#[derive(Debug)]
pub enum EmbeddingIndexError {
    InvalidBatchSize,
    Database {
        source: nexus_db::DatabaseError,
    },
    DocumentStore {
        source: nexus_db::DocumentStoreError,
    },
    EmbeddingStore {
        source: EmbeddingStoreError,
    },
    Embedding {
        source: EmbeddingError,
    },
    Cancelled,
}

impl EmbeddingIndexError {
    /// 返回不包含路径、正文或底层数据库文本的安全分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidBatchSize => "embedding_batch_size_invalid",
            Self::Database { source } => source.kind(),
            Self::DocumentStore { source } => source.kind(),
            Self::EmbeddingStore { source } => source.kind(),
            Self::Embedding { source } => source.kind(),
            Self::Cancelled => "embedding_index_cancelled",
        }
    }

    /// 返回可直接展示给用户的安全说明。
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::InvalidBatchSize => "向量索引批次大小无效。",
            Self::Database { .. } | Self::DocumentStore { .. } | Self::EmbeddingStore { .. } => {
                "本地向量索引暂时不可用。"
            }
            Self::Embedding { source } => source.user_message(),
            Self::Cancelled => "本地向量索引已取消。",
        }
    }
}

impl fmt::Display for EmbeddingIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "embedding 索引失败: {}", self.kind())
    }
}

impl Error for EmbeddingIndexError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database { source } => Some(source),
            Self::DocumentStore { source } => Some(source),
            Self::EmbeddingStore { source } => Some(source),
            Self::Embedding { source } => Some(source),
            Self::InvalidBatchSize | Self::Cancelled => None,
        }
    }
}

/// 为当前数据库中的全部 canonical 文档建立指定 provider 的 embedding。
///
/// 文档读取和向量写入都按批次进行；单个文档生成失败会计入统计并继续。数据库
/// 错误、模型冲突和取消会终止任务，但已经提交的前序批次保持有效。
pub fn index_document_embeddings<D, P>(
    database_path: D,
    provider: &P,
    options: EmbeddingIndexOptions,
    control: &RescanControl,
) -> Result<EmbeddingIndexSummary, EmbeddingIndexError>
where
    D: AsRef<Path>,
    P: EmbeddingProvider,
{
    validate_options(options)?;
    let mut connection = initialize_database(database_path)
        .map_err(|source| EmbeddingIndexError::Database { source })?;
    if control.is_cancelled() {
        return Err(EmbeddingIndexError::Cancelled);
    }
    let model = model_from_provider(provider);
    upsert_document_embeddings(&mut connection, &model, &[])
        .map_err(|source| EmbeddingIndexError::EmbeddingStore { source })?;

    let mut summary = EmbeddingIndexSummary::default();
    let mut after_document_id = None;

    loop {
        if control.is_cancelled() {
            return Err(EmbeddingIndexError::Cancelled);
        }

        let documents = list_document_batch(
            &connection,
            after_document_id.as_deref(),
            options.batch_size,
        )
        .map_err(|source| EmbeddingIndexError::DocumentStore { source })?;
        if documents.is_empty() {
            break;
        }

        after_document_id = documents.last().map(|document| document.id.clone());
        let (batch, failed) = prepare_embeddings(&documents, provider, control)?;
        summary.documents_seen += documents.len();
        summary.documents_failed += failed;
        summary.batches += 1;

        if !batch.is_empty() {
            let write_summary = upsert_document_embeddings(&mut connection, &model, &batch)
                .map_err(|source| EmbeddingIndexError::EmbeddingStore { source })?;
            summary.embeddings_written += write_summary.embeddings_written;
        }
    }

    Ok(summary)
}

/// 只刷新一批来源路径对应的 canonical 文档 embedding。
///
/// 路径去重后再按文档 ID 去重，兼容未来一个来源拆成多个文档的模型。删除或暂时
/// 不可解析的文档不会重新写向量；M4 的文档更新触发器已经负责清理旧向量。
pub fn refresh_document_embeddings_for_paths<D, P>(
    database_path: D,
    paths: &[PathBuf],
    provider: &P,
    options: EmbeddingIndexOptions,
    control: &RescanControl,
) -> Result<EmbeddingIndexSummary, EmbeddingIndexError>
where
    D: AsRef<Path>,
    P: EmbeddingProvider,
{
    validate_options(options)?;
    let mut connection = initialize_database(database_path)
        .map_err(|source| EmbeddingIndexError::Database { source })?;
    let model = model_from_provider(provider);
    let mut documents_by_id = BTreeMap::<String, DocumentRecord>::new();

    for path in paths {
        if control.is_cancelled() {
            return Err(EmbeddingIndexError::Cancelled);
        }
        for document in list_documents_for_path(&connection, path)
            .map_err(|source| EmbeddingIndexError::DocumentStore { source })?
        {
            documents_by_id.insert(document.id.clone(), document);
        }
    }

    let documents = documents_by_id.into_values().collect::<Vec<_>>();
    let mut documents_failed = 0;
    let mut embeddings_written = 0;
    let mut batches = 0;

    for document_batch in documents.chunks(options.batch_size) {
        let (batch, failed) = prepare_embeddings(document_batch, provider, control)?;
        documents_failed += failed;

        if !batch.is_empty() {
            let write_summary = upsert_document_embeddings(&mut connection, &model, &batch)
                .map_err(|source| EmbeddingIndexError::EmbeddingStore { source })?;
            embeddings_written += write_summary.embeddings_written;
            batches += 1;
        }
    }

    Ok(EmbeddingIndexSummary {
        documents_seen: documents.len(),
        embeddings_written,
        documents_failed,
        batches,
    })
}

fn validate_options(options: EmbeddingIndexOptions) -> Result<(), EmbeddingIndexError> {
    if options.batch_size == 0 {
        return Err(EmbeddingIndexError::InvalidBatchSize);
    }
    Ok(())
}

fn model_from_provider<P: EmbeddingProvider>(provider: &P) -> EmbeddingModel {
    EmbeddingModel {
        model_id: provider.model_id().to_owned(),
        model_version: provider.model_version().to_owned(),
        provider_kind: provider.provider_kind().to_owned(),
        dimensions: provider.dimensions(),
    }
}

fn prepare_embeddings<P: EmbeddingProvider>(
    documents: &[DocumentRecord],
    provider: &P,
    control: &RescanControl,
) -> Result<(Vec<DocumentEmbedding>, usize), EmbeddingIndexError> {
    let mut embeddings = Vec::with_capacity(documents.len());
    let mut documents_failed = 0;

    for document in documents {
        if control.is_cancelled() {
            return Err(EmbeddingIndexError::Cancelled);
        }

        match provider.embed_document(&document.title, &document.body) {
            Ok(vector) => embeddings.push(DocumentEmbedding {
                document_id: document.id.clone(),
                model_id: provider.model_id().to_owned(),
                model_version: provider.model_version().to_owned(),
                source_fingerprint: document_input_fingerprint(&document.title, &document.body)
                    .to_vec(),
                vector: vector.as_slice().to_vec(),
            }),
            Err(_) => documents_failed += 1,
        }
    }

    Ok((embeddings, documents_failed))
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::PathBuf,
        process,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use nexus_db::{get_document_embedding, initialize_database, upsert_document, DocumentRecord};

    use super::{
        index_document_embeddings, refresh_document_embeddings_for_paths, EmbeddingIndexOptions,
    };
    use crate::{LocalFeatureEmbedding, RescanControl};

    static TEMP_DIRECTORY_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TemporaryDirectory {
        path: PathBuf,
    }

    impl TemporaryDirectory {
        fn new() -> Self {
            let counter = TEMP_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "nexus-semantic-index-test-{}-{counter}",
                process::id()
            ));
            fs::create_dir_all(&path).expect("创建 semantic 测试目录失败");
            Self { path }
        }

        fn database_path(&self) -> PathBuf {
            self.path.join("nexus.sqlite3")
        }

        fn document(&self, id: &str, name: &str, body: &str) -> DocumentRecord {
            DocumentRecord {
                id: id.to_owned(),
                source_path: self.path.join(name),
                title: name.to_owned(),
                body: body.to_owned(),
                line_start: None,
                line_end: None,
            }
        }
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn indexes_documents_in_bounded_batches() {
        let directory = TemporaryDirectory::new();
        let database_path = directory.database_path();
        let connection = initialize_database(&database_path).expect("初始化 semantic 数据库失败");
        upsert_document(
            &connection,
            &directory.document("file:b", "b.md", "second body"),
        )
        .expect("写入第二条 semantic 文档失败");
        upsert_document(
            &connection,
            &directory.document("file:a", "a.md", "first body"),
        )
        .expect("写入第一条 semantic 文档失败");

        let summary = index_document_embeddings(
            &database_path,
            &LocalFeatureEmbedding::new(),
            EmbeddingIndexOptions { batch_size: 1 },
            &RescanControl::new(),
        )
        .expect("批量建立 semantic embedding 失败");
        assert_eq!(summary.documents_seen, 2);
        assert_eq!(summary.embeddings_written, 2);
        assert_eq!(summary.batches, 2);
        assert_eq!(summary.documents_failed, 0);

        let connection = initialize_database(&database_path).expect("重新打开 semantic 数据库失败");
        assert!(
            get_document_embedding(&connection, "file:a", "nexus-local-feature-hash", "1")
                .expect("读取第一条 semantic embedding 失败")
                .is_some()
        );
    }

    #[test]
    fn refreshes_only_documents_under_changed_paths() {
        let directory = TemporaryDirectory::new();
        let database_path = directory.database_path();
        let connection = initialize_database(&database_path).expect("初始化局部刷新数据库失败");
        let changed = directory.document("file:changed", "changed.md", "old body");
        let untouched = directory.document("file:untouched", "untouched.md", "same body");
        upsert_document(&connection, &changed).expect("写入待刷新文档失败");
        upsert_document(&connection, &untouched).expect("写入未变更文档失败");
        index_document_embeddings(
            &database_path,
            &LocalFeatureEmbedding::new(),
            EmbeddingIndexOptions::default(),
            &RescanControl::new(),
        )
        .expect("建立局部刷新初始 embedding 失败");

        let old = get_document_embedding(
            &initialize_database(&database_path).expect("读取局部刷新前数据库失败"),
            &changed.id,
            "nexus-local-feature-hash",
            "1",
        )
        .expect("读取局部刷新前向量失败")
        .expect("局部刷新前应存在向量");

        let connection = initialize_database(&database_path).expect("打开局部刷新更新数据库失败");
        let updated = DocumentRecord {
            body: "new body with different content".to_owned(),
            ..changed.clone()
        };
        upsert_document(&connection, &updated).expect("更新待刷新文档失败");
        drop(connection);

        let summary = refresh_document_embeddings_for_paths(
            &database_path,
            std::slice::from_ref(&changed.source_path),
            &LocalFeatureEmbedding::new(),
            EmbeddingIndexOptions::default(),
            &RescanControl::new(),
        )
        .expect("刷新局部 semantic embedding 失败");
        assert_eq!(summary.documents_seen, 1);
        assert_eq!(summary.embeddings_written, 1);

        let connection = initialize_database(&database_path).expect("重新打开局部刷新数据库失败");
        let refreshed =
            get_document_embedding(&connection, &changed.id, "nexus-local-feature-hash", "1")
                .expect("读取局部刷新后向量失败")
                .expect("局部刷新后应存在向量");
        assert_ne!(old.source_fingerprint, refreshed.source_fingerprint);
        assert!(get_document_embedding(
            &connection,
            &untouched.id,
            "nexus-local-feature-hash",
            "1"
        )
        .expect("读取未变更文档向量失败")
        .is_some());
    }

    #[test]
    fn refreshes_all_documents_for_a_path_in_bounded_batches() {
        let directory = TemporaryDirectory::new();
        let database_path = directory.database_path();
        let shared_path = directory.path.join("split.md");
        let connection = initialize_database(&database_path).expect("初始化多文档刷新数据库失败");

        for (id, body) in [
            ("file:split-a", "first body"),
            ("file:split-b", "second body"),
            ("file:split-c", "third body"),
        ] {
            upsert_document(
                &connection,
                &DocumentRecord {
                    id: id.to_owned(),
                    source_path: shared_path.clone(),
                    title: id.to_owned(),
                    body: body.to_owned(),
                    line_start: None,
                    line_end: None,
                },
            )
            .expect("写入同来源多文档失败");
        }
        drop(connection);

        let summary = refresh_document_embeddings_for_paths(
            &database_path,
            std::slice::from_ref(&shared_path),
            &LocalFeatureEmbedding::new(),
            EmbeddingIndexOptions { batch_size: 2 },
            &RescanControl::new(),
        )
        .expect("分批刷新同来源多文档失败");

        assert_eq!(summary.documents_seen, 3);
        assert_eq!(summary.embeddings_written, 3);
        assert_eq!(summary.documents_failed, 0);
        assert_eq!(summary.batches, 2);

        let connection = initialize_database(&database_path).expect("重新打开多文档刷新数据库失败");
        for id in ["file:split-a", "file:split-b", "file:split-c"] {
            assert!(
                get_document_embedding(&connection, id, "nexus-local-feature-hash", "1",)
                    .expect("读取同来源多文档 embedding 失败")
                    .is_some()
            );
        }
    }

    #[test]
    fn cancellation_stops_before_writing_document_batches() {
        let directory = TemporaryDirectory::new();
        let database_path = directory.database_path();
        let connection = initialize_database(&database_path).expect("初始化取消测试数据库失败");
        upsert_document(
            &connection,
            &directory.document("file:cancel", "cancel.md", "cancel body"),
        )
        .expect("写入取消测试文档失败");
        let control = RescanControl::new();
        control.cancel();

        assert!(matches!(
            index_document_embeddings(
                &database_path,
                &LocalFeatureEmbedding::new(),
                EmbeddingIndexOptions::default(),
                &control,
            ),
            Err(super::EmbeddingIndexError::Cancelled)
        ));
        let connection = initialize_database(&database_path).expect("重新打开取消测试数据库失败");
        let model_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM embedding_models", [], |row| {
                row.get(0)
            })
            .expect("统计取消测试模型失败");
        assert_eq!(model_count, 0);
        assert!(get_document_embedding(
            &connection,
            "file:cancel",
            "nexus-local-feature-hash",
            "1"
        )
        .expect("读取取消测试向量失败")
        .is_none());
    }
}
