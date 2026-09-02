//! 本地 embedding 元数据和向量的 SQLite 存储边界。
//!
//! 本模块只负责校验、版本登记、编码和事务写入，不生成向量，也不读取文件正文。
//! 生成工作由 `nexus-core` 的 embedding provider 负责；这样以后替换模型时，不需要
//! 让数据库层依赖推理引擎或网络服务。

use std::{error::Error, fmt};

use rusqlite::{params, Connection, OptionalExtension};

/// 输入指纹的固定字节长度。
pub const EMBEDDING_FINGERPRINT_BYTES: usize = 16;

/// 单个向量允许的最大维度，防止损坏或误配置造成异常内存分配。
pub const MAX_EMBEDDING_DIMENSIONS: usize = 16_384;

/// 一个可持久化的 embedding provider 身份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingModel {
    pub model_id: String,
    pub model_version: String,
    pub provider_kind: String,
    pub dimensions: usize,
}

/// 一条文档 embedding 记录。
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentEmbedding {
    pub document_id: String,
    pub model_id: String,
    pub model_version: String,
    pub source_fingerprint: Vec<u8>,
    pub vector: Vec<f32>,
}

/// 一次 embedding 批次的写入统计。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmbeddingWriteSummary {
    pub models_registered: usize,
    pub embeddings_written: usize,
}

/// embedding 存储错误。
#[derive(Debug)]
pub enum EmbeddingStoreError {
    ModelIdEmpty,
    ModelVersionEmpty,
    ProviderKindEmpty,
    IdentifierContainsNull,
    DimensionsInvalid {
        value: usize,
    },
    ModelConflict,
    DocumentIdEmpty,
    DocumentIdContainsNull,
    DocumentNotFound,
    FingerprintInvalid {
        actual: usize,
    },
    VectorLengthMismatch {
        expected: usize,
        actual: usize,
    },
    VectorNonFinite,
    VectorZeroNorm,
    StoredVectorCorrupt,
    Query {
        operation: &'static str,
        source: rusqlite::Error,
    },
}

impl EmbeddingStoreError {
    /// 返回不包含文档 ID、正文或 SQLite 原始文本的错误分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ModelIdEmpty
            | Self::ModelVersionEmpty
            | Self::ProviderKindEmpty
            | Self::IdentifierContainsNull
            | Self::ModelConflict => "embedding_model_invalid",
            Self::DimensionsInvalid { .. } | Self::VectorLengthMismatch { .. } => {
                "embedding_dimensions_invalid"
            }
            Self::DocumentIdEmpty | Self::DocumentIdContainsNull => "embedding_document_invalid",
            Self::DocumentNotFound => "embedding_document_missing",
            Self::FingerprintInvalid { .. } => "embedding_fingerprint_invalid",
            Self::VectorNonFinite | Self::VectorZeroNorm => "embedding_vector_invalid",
            Self::StoredVectorCorrupt => "embedding_vector_corrupt",
            Self::Query { operation, .. } => match *operation {
                "model_insert" | "model_read" => "embedding_model_storage",
                "embedding_insert" | "embedding_read" => "embedding_storage",
                "document_exists" => "embedding_document_read",
                _ => "embedding_query",
            },
        }
    }

    /// 返回可直接展示给用户的非敏感说明。
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::ModelIdEmpty
            | Self::ModelVersionEmpty
            | Self::ProviderKindEmpty
            | Self::IdentifierContainsNull
            | Self::ModelConflict
            | Self::DimensionsInvalid { .. } => "本地向量模型配置无效。",
            Self::DocumentIdEmpty | Self::DocumentIdContainsNull | Self::DocumentNotFound => {
                "向量对应的文档记录不可用。"
            }
            Self::FingerprintInvalid { .. }
            | Self::VectorLengthMismatch { .. }
            | Self::VectorNonFinite
            | Self::VectorZeroNorm
            | Self::StoredVectorCorrupt => "本地向量数据无效。",
            Self::Query { .. } => "本地向量存储暂时不可用。",
        }
    }
}

impl fmt::Display for EmbeddingStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "本地向量存储失败: {}", self.kind())
    }
}

impl Error for EmbeddingStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Query { source, .. } => Some(source),
            Self::ModelIdEmpty
            | Self::ModelVersionEmpty
            | Self::ProviderKindEmpty
            | Self::IdentifierContainsNull
            | Self::DimensionsInvalid { .. }
            | Self::ModelConflict
            | Self::DocumentIdEmpty
            | Self::DocumentIdContainsNull
            | Self::DocumentNotFound
            | Self::FingerprintInvalid { .. }
            | Self::VectorLengthMismatch { .. }
            | Self::VectorNonFinite
            | Self::VectorZeroNorm
            | Self::StoredVectorCorrupt => None,
        }
    }
}

/// 在一个事务中登记模型并 upsert 一批文档向量。
///
/// 模型的 `(model_id, model_version)` 一旦登记，维度和 provider 类型不能静默改变；
/// 新模型必须使用新的版本。向量只保存标题/正文输入的指纹，数据库不保存额外正文副本。
pub fn upsert_document_embeddings(
    connection: &mut Connection,
    model: &EmbeddingModel,
    embeddings: &[DocumentEmbedding],
) -> Result<EmbeddingWriteSummary, EmbeddingStoreError> {
    validate_model(model)?;
    for embedding in embeddings {
        validate_embedding(model, embedding)?;
    }

    let model_was_registered = connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM embedding_models
                 WHERE model_id = ?1 AND model_version = ?2
             )",
            params![&model.model_id, &model.model_version],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|source| EmbeddingStoreError::Query {
            operation: "model_read",
            source,
        })?;

    let transaction = connection
        .transaction()
        .map_err(|source| EmbeddingStoreError::Query {
            operation: "model_insert",
            source,
        })?;

    let existing = transaction
        .query_row(
            "SELECT provider_kind, dimensions
             FROM embedding_models
             WHERE model_id = ?1 AND model_version = ?2",
            params![&model.model_id, &model.model_version],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(|source| EmbeddingStoreError::Query {
            operation: "model_read",
            source,
        })?;

    match existing {
        Some((provider_kind, dimensions)) => {
            let dimensions = usize::try_from(dimensions).map_err(|_| {
                EmbeddingStoreError::DimensionsInvalid {
                    value: model.dimensions,
                }
            })?;
            if provider_kind != model.provider_kind || dimensions != model.dimensions {
                return Err(EmbeddingStoreError::ModelConflict);
            }
        }
        None => {
            let dimensions = i64::try_from(model.dimensions).map_err(|_| {
                EmbeddingStoreError::DimensionsInvalid {
                    value: model.dimensions,
                }
            })?;
            transaction
                .execute(
                    "INSERT INTO embedding_models (
                         model_id, model_version, provider_kind, dimensions
                     ) VALUES (?1, ?2, ?3, ?4)",
                    params![
                        &model.model_id,
                        &model.model_version,
                        &model.provider_kind,
                        dimensions,
                    ],
                )
                .map_err(|source| EmbeddingStoreError::Query {
                    operation: "model_insert",
                    source,
                })?;
        }
    }

    for embedding in embeddings {
        let vector = encode_vector(&embedding.vector);
        let inserted = transaction
            .execute(
                "INSERT INTO document_embeddings (
                     document_id,
                     model_id,
                     model_version,
                     dimensions,
                     source_fingerprint,
                     vector
                 )
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6
                 WHERE EXISTS (
                     SELECT 1 FROM documents WHERE document_id = ?1
                 )
                 ON CONFLICT(document_id, model_id, model_version) DO UPDATE SET
                     dimensions = excluded.dimensions,
                     source_fingerprint = excluded.source_fingerprint,
                     vector = excluded.vector",
                params![
                    &embedding.document_id,
                    &embedding.model_id,
                    &embedding.model_version,
                    i64::try_from(model.dimensions).map_err(|_| {
                        EmbeddingStoreError::DimensionsInvalid {
                            value: model.dimensions,
                        }
                    })?,
                    &embedding.source_fingerprint,
                    vector,
                ],
            )
            .map_err(|source| EmbeddingStoreError::Query {
                operation: "embedding_insert",
                source,
            })?;
        if inserted != 1 {
            return Err(EmbeddingStoreError::DocumentNotFound);
        }
    }

    transaction
        .commit()
        .map_err(|source| EmbeddingStoreError::Query {
            operation: "embedding_insert",
            source,
        })?;

    Ok(EmbeddingWriteSummary {
        models_registered: usize::from(!model_was_registered),
        embeddings_written: embeddings.len(),
    })
}

/// 读取一条指定模型的文档向量；损坏数据会返回安全错误，不会静默参与搜索。
pub fn get_document_embedding(
    connection: &Connection,
    document_id: &str,
    model_id: &str,
    model_version: &str,
) -> Result<Option<DocumentEmbedding>, EmbeddingStoreError> {
    validate_document_id(document_id)?;
    validate_identifier(model_id, true)?;
    validate_identifier(model_version, false)?;

    let row = connection
        .query_row(
            "SELECT dimensions, source_fingerprint, vector
             FROM document_embeddings
             WHERE document_id = ?1 AND model_id = ?2 AND model_version = ?3",
            params![document_id, model_id, model_version],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|source| EmbeddingStoreError::Query {
            operation: "embedding_read",
            source,
        })?;

    let Some((dimensions, source_fingerprint, vector)) = row else {
        return Ok(None);
    };
    let dimensions = usize::try_from(dimensions)
        .ok()
        .filter(|value| (1..=MAX_EMBEDDING_DIMENSIONS).contains(value))
        .ok_or(EmbeddingStoreError::StoredVectorCorrupt)?;
    if source_fingerprint.len() != EMBEDDING_FINGERPRINT_BYTES {
        return Err(EmbeddingStoreError::StoredVectorCorrupt);
    }
    let vector = decode_vector(&vector, dimensions)?;

    Ok(Some(DocumentEmbedding {
        document_id: document_id.to_owned(),
        model_id: model_id.to_owned(),
        model_version: model_version.to_owned(),
        source_fingerprint,
        vector,
    }))
}

fn validate_model(model: &EmbeddingModel) -> Result<(), EmbeddingStoreError> {
    validate_identifier(&model.model_id, true)?;
    validate_identifier(&model.model_version, false)?;
    if model.provider_kind.trim().is_empty() {
        return Err(EmbeddingStoreError::ProviderKindEmpty);
    }
    if model.provider_kind.contains('\0') {
        return Err(EmbeddingStoreError::IdentifierContainsNull);
    }
    if model.dimensions == 0 || model.dimensions > MAX_EMBEDDING_DIMENSIONS {
        return Err(EmbeddingStoreError::DimensionsInvalid {
            value: model.dimensions,
        });
    }
    Ok(())
}

fn validate_identifier(value: &str, model_id: bool) -> Result<(), EmbeddingStoreError> {
    if value.trim().is_empty() {
        return Err(if model_id {
            EmbeddingStoreError::ModelIdEmpty
        } else {
            EmbeddingStoreError::ModelVersionEmpty
        });
    }
    if value.contains('\0') {
        return Err(EmbeddingStoreError::IdentifierContainsNull);
    }
    Ok(())
}

fn validate_document_id(value: &str) -> Result<(), EmbeddingStoreError> {
    if value.trim().is_empty() {
        return Err(EmbeddingStoreError::DocumentIdEmpty);
    }
    if value.contains('\0') {
        return Err(EmbeddingStoreError::DocumentIdContainsNull);
    }
    Ok(())
}

fn validate_embedding(
    model: &EmbeddingModel,
    embedding: &DocumentEmbedding,
) -> Result<(), EmbeddingStoreError> {
    validate_document_id(&embedding.document_id)?;
    if embedding.model_id != model.model_id || embedding.model_version != model.model_version {
        return Err(EmbeddingStoreError::ModelConflict);
    }
    if embedding.source_fingerprint.len() != EMBEDDING_FINGERPRINT_BYTES {
        return Err(EmbeddingStoreError::FingerprintInvalid {
            actual: embedding.source_fingerprint.len(),
        });
    }
    if embedding.vector.len() != model.dimensions {
        return Err(EmbeddingStoreError::VectorLengthMismatch {
            expected: model.dimensions,
            actual: embedding.vector.len(),
        });
    }
    if embedding.vector.iter().any(|value| !value.is_finite()) {
        return Err(EmbeddingStoreError::VectorNonFinite);
    }
    if squared_norm(&embedding.vector) <= f32::EPSILON {
        return Err(EmbeddingStoreError::VectorZeroNorm);
    }
    Ok(())
}

fn encode_vector(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

pub(crate) fn decode_vector(
    bytes: &[u8],
    dimensions: usize,
) -> Result<Vec<f32>, EmbeddingStoreError> {
    let expected_bytes = dimensions
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or(EmbeddingStoreError::StoredVectorCorrupt)?;
    if bytes.len() != expected_bytes {
        return Err(EmbeddingStoreError::StoredVectorCorrupt);
    }

    let (chunks, remainder) = bytes.as_chunks::<{ std::mem::size_of::<f32>() }>();
    if !remainder.is_empty() {
        return Err(EmbeddingStoreError::StoredVectorCorrupt);
    }
    let vector = chunks
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect::<Vec<_>>();
    if vector.iter().any(|value| !value.is_finite()) || squared_norm(&vector) <= f32::EPSILON {
        return Err(EmbeddingStoreError::StoredVectorCorrupt);
    }
    Ok(vector)
}

fn squared_norm(vector: &[f32]) -> f32 {
    vector.iter().map(|value| value * value).sum()
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::PathBuf,
        process,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::{
        get_document_embedding, upsert_document_embeddings, DocumentEmbedding, EmbeddingModel,
        EmbeddingStoreError, EMBEDDING_FINGERPRINT_BYTES,
    };
    use crate::{initialize_database, upsert_document, DocumentRecord};

    static TEMP_DIRECTORY_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TemporaryDirectory {
        path: PathBuf,
    }

    impl TemporaryDirectory {
        fn new() -> Self {
            let counter = TEMP_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "nexus-embedding-db-test-{}-{counter}",
                process::id()
            ));
            fs::create_dir_all(&path).expect("创建 embedding 测试目录失败");
            Self { path }
        }

        fn database_path(&self) -> PathBuf {
            self.path.join("nexus.sqlite3")
        }

        fn document(&self) -> DocumentRecord {
            DocumentRecord {
                id: "file:embedding".to_owned(),
                source_path: self.path.join("embedding.md"),
                title: "Embedding title".to_owned(),
                body: "Embedding body".to_owned(),
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

    fn model() -> EmbeddingModel {
        EmbeddingModel {
            model_id: "test-model".to_owned(),
            model_version: "1".to_owned(),
            provider_kind: "test".to_owned(),
            dimensions: 4,
        }
    }

    fn embedding() -> DocumentEmbedding {
        DocumentEmbedding {
            document_id: "file:embedding".to_owned(),
            model_id: "test-model".to_owned(),
            model_version: "1".to_owned(),
            source_fingerprint: vec![7; EMBEDDING_FINGERPRINT_BYTES],
            vector: vec![1.0, 0.0, 0.0, 0.0],
        }
    }

    #[test]
    fn writes_and_reads_versioned_embedding_in_one_local_store() {
        let directory = TemporaryDirectory::new();
        let mut connection = initialize_database(directory.database_path())
            .expect("初始化 embedding 测试数据库失败");
        let document = directory.document();
        upsert_document(&connection, &document).expect("写入 embedding 测试文档失败");

        let summary = upsert_document_embeddings(&mut connection, &model(), &[embedding()])
            .expect("写入 embedding 测试向量失败");
        assert_eq!(summary.models_registered, 1);
        assert_eq!(summary.embeddings_written, 1);

        let stored = get_document_embedding(&connection, &document.id, "test-model", "1")
            .expect("读取 embedding 测试向量失败")
            .expect("应读取到 embedding");
        assert_eq!(stored, embedding());
    }

    #[test]
    fn rejects_model_conflicts_and_invalid_vectors() {
        let directory = TemporaryDirectory::new();
        let mut connection = initialize_database(directory.database_path())
            .expect("初始化 embedding 冲突测试数据库失败");
        let document = directory.document();
        upsert_document(&connection, &document).expect("写入 embedding 冲突测试文档失败");
        upsert_document_embeddings(&mut connection, &model(), &[embedding()])
            .expect("写入初始 embedding 失败");

        let mut conflicting_model = model();
        conflicting_model.dimensions = 8;
        let error = upsert_document_embeddings(&mut connection, &conflicting_model, &[])
            .expect_err("模型维度冲突不应被接受");
        assert!(matches!(error, EmbeddingStoreError::ModelConflict));

        let mut invalid = embedding();
        invalid.vector[0] = f32::NAN;
        let error = upsert_document_embeddings(&mut connection, &model(), &[invalid])
            .expect_err("非有限向量不应被接受");
        assert!(matches!(error, EmbeddingStoreError::VectorNonFinite));
    }

    #[test]
    fn rejects_an_empty_provider_kind_without_misclassifying_it() {
        let directory = TemporaryDirectory::new();
        let mut connection = initialize_database(directory.database_path())
            .expect("初始化 provider 校验测试数据库失败");
        let mut invalid_model = model();
        invalid_model.provider_kind = "  ".to_owned();

        let error = upsert_document_embeddings(&mut connection, &invalid_model, &[])
            .expect_err("空 provider 类型不应被接受");
        assert!(matches!(error, EmbeddingStoreError::ProviderKindEmpty));
    }

    #[test]
    fn removes_vectors_when_document_content_changes_or_document_is_deleted() {
        let directory = TemporaryDirectory::new();
        let mut connection = initialize_database(directory.database_path())
            .expect("初始化 embedding 清理测试数据库失败");
        let document = directory.document();
        upsert_document(&connection, &document).expect("写入 embedding 清理测试文档失败");
        upsert_document_embeddings(&mut connection, &model(), &[embedding()])
            .expect("写入清理测试向量失败");

        let updated = DocumentRecord {
            body: "changed body".to_owned(),
            ..document.clone()
        };
        upsert_document(&connection, &updated).expect("更新 embedding 清理测试文档失败");
        assert!(
            get_document_embedding(&connection, &document.id, "test-model", "1")
                .expect("读取更新后的 embedding 失败")
                .is_none()
        );

        upsert_document_embeddings(&mut connection, &model(), &[embedding()])
            .expect("重新写入清理测试向量失败");
        crate::delete_document(&connection, &document.id).expect("删除 embedding 测试文档失败");
        assert!(
            get_document_embedding(&connection, &document.id, "test-model", "1")
                .expect("读取删除后的 embedding 失败")
                .is_none()
        );
    }

    #[test]
    fn does_not_write_a_vector_for_a_missing_document() {
        let directory = TemporaryDirectory::new();
        let mut connection =
            initialize_database(directory.database_path()).expect("初始化缺失文档测试数据库失败");
        let error = upsert_document_embeddings(&mut connection, &model(), &[embedding()])
            .expect_err("缺失文档不应写入向量");
        assert!(matches!(error, EmbeddingStoreError::DocumentNotFound));
    }
}
