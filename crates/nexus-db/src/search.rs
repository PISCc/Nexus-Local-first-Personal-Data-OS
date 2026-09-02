//! 本地全文查询和基础元数据过滤。
//!
//! M3.2/M3.3 提供受限、确定性的关键词/短语查询、基本过滤器、lexical ranking
//! 和有限匹配片段；不把原始 FTS5 查询语法暴露给上层，也不在这里实现搜索 UI。

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BinaryHeap},
    error::Error,
    fmt,
    path::PathBuf,
};

use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};

use super::{decode_stored_location, embedding::decode_vector, path_from_key};

/// 默认建议的单次搜索结果上限。
pub const DEFAULT_SEARCH_LIMIT: usize = 100;

/// 单次搜索允许请求的最大结果数。
pub const MAX_SEARCH_LIMIT: usize = 1_000;

const SEARCH_SELECT_SQL: &str = "
SELECT
    documents.document_id,
    documents.source_kind,
    documents.source_path_key,
    documents.title,
    file_metadata.file_name,
    file_metadata.extension,
    file_metadata.file_type,
    file_metadata.modified_at,
    file_metadata.created_at,
    file_metadata.accessed_at,
    documents.line_start,
    documents.line_end,
    NULL AS relevance,
    NULL AS title_snippet,
    NULL AS body_snippet
FROM documents
LEFT JOIN file_metadata ON file_metadata.path_key = documents.source_path_key
";

const SEARCH_TEXT_SELECT_SQL: &str = "
SELECT
    documents.document_id,
    documents.source_kind,
    documents.source_path_key,
    documents.title,
    file_metadata.file_name,
    file_metadata.extension,
    file_metadata.file_type,
    file_metadata.modified_at,
    file_metadata.created_at,
    file_metadata.accessed_at,
    documents.line_start,
    documents.line_end,
    -bm25(documents_fts, 1.0, 5.0, 1.0) AS relevance,
    snippet(documents_fts, 1, '⟦', '⟧', '…', 32) AS title_snippet,
    snippet(documents_fts, 2, '⟦', '⟧', '…', 32) AS body_snippet
FROM documents_fts
JOIN documents ON documents.rowid = documents_fts.rowid
LEFT JOIN file_metadata ON file_metadata.path_key = documents.source_path_key
";

const SEMANTIC_SELECT_SQL: &str = "
SELECT
    documents.document_id,
    documents.source_kind,
    documents.source_path_key,
    documents.title,
    file_metadata.file_name,
    file_metadata.extension,
    file_metadata.file_type,
    file_metadata.modified_at,
    file_metadata.created_at,
    file_metadata.accessed_at,
    documents.line_start,
    documents.line_end,
    document_embeddings.dimensions,
    document_embeddings.vector
FROM document_embeddings
JOIN documents ON documents.document_id = document_embeddings.document_id
LEFT JOIN file_metadata ON file_metadata.path_key = documents.source_path_key
";

/// 一条本地搜索命中。
///
/// 返回可追溯的文档、当前可用的文件元数据、确定性相关性和匹配片段。
///
/// `snippet` 是纯文本，命中范围使用 `⟦` 和 `⟧` 标记；结果不携带完整正文。
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub document_id: String,
    pub source_path: PathBuf,
    pub title: String,
    pub file_name: Option<String>,
    pub extension: Option<String>,
    pub file_type: Option<String>,
    pub modified_at: Option<i64>,
    pub created_at: Option<i64>,
    pub accessed_at: Option<i64>,
    pub line_start: Option<u64>,
    pub line_end: Option<u64>,
    /// FTS5 relevance score；数值越大越相关。仅正文查询会产生该值。
    pub relevance: Option<f64>,
    /// 命中片段，命中范围由 `⟦` 和 `⟧` 标记。
    pub snippet: Option<String>,
}

/// 一条混合检索命中。
///
/// `result.relevance` 保留 lexical BM25 分数；`semantic_similarity` 和
/// `fusion_score` 单独表达向量分支与 RRF 融合结果，避免把不同量纲的分数
/// 直接相加。结果仍然只携带可追溯元数据和有限 snippet，不携带完整正文。
#[derive(Debug, Clone, PartialEq)]
pub struct HybridSearchResult {
    pub result: SearchResult,
    pub lexical_rank: Option<usize>,
    pub semantic_rank: Option<usize>,
    pub semantic_similarity: Option<f32>,
    pub fusion_score: f64,
}

/// 执行一次本地关键词/短语搜索和基本元数据过滤。
///
/// `query` 支持以下受限语法，所有条件按 AND 组合：
///
/// - `keyword`：一个关键词；多个关键词必须同时命中。
/// - `"短语"`：双引号包裹的短语。
/// - `filename:value`、`path:value`：不区分 ASCII 大小写的包含匹配。
/// - `ext:value` / `extension:value`：扩展名精确匹配，自动去除开头的点号。
/// - `type:value`：文件类型精确匹配。
/// - `modified|created|accessed|date:YYYY-MM-DD`：匹配该日期的整天。
/// - 日期也支持 `modified>=YYYY-MM-DD` 和 `modified<=YYYY-MM-DD` 等比较。
///
/// 日期按 UTC 日历日解释；文件时间字段仍使用数据库现有的 Unix epoch 毫秒。
/// `limit` 必须在 `1..=MAX_SEARCH_LIMIT` 内。正文查询按 relevance 降序、文档 ID
/// 升序排序；仅过滤器查询按文档 ID 升序排序。
pub fn search_documents(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchResult>, SearchError> {
    if !(1..=MAX_SEARCH_LIMIT).contains(&limit) {
        return Err(SearchError::InvalidLimit);
    }

    let parsed = parse_search_query(query)?;
    let limit = i64::try_from(limit).map_err(|_| SearchError::InvalidLimit)?;
    let has_text_query = parsed.fts_query.is_some();
    let sql = build_search_sql(has_text_query);
    let parameters = bind_parameters(&parsed, limit, has_text_query);
    let mut statement = connection
        .prepare(&sql)
        .map_err(|source| SearchError::Query {
            operation: "prepare",
            source,
        })?;

    let rows = statement
        .query_map(params_from_iter(parameters), |row| {
            Ok(RawSearchResult {
                document_id: row.get(0)?,
                source_kind: row.get(1)?,
                source_path_key: row.get(2)?,
                title: row.get(3)?,
                file_name: row.get(4)?,
                extension: row.get(5)?,
                file_type: row.get(6)?,
                modified_at: row.get(7)?,
                created_at: row.get(8)?,
                accessed_at: row.get(9)?,
                line_start: row.get(10)?,
                line_end: row.get(11)?,
                relevance: row.get(12)?,
                title_snippet: row.get(13)?,
                body_snippet: row.get(14)?,
            })
        })
        .map_err(|source| SearchError::Query {
            operation: "query",
            source,
        })?;

    rows.map(|row| {
        let row = row.map_err(|source| SearchError::Query {
            operation: "read",
            source,
        })?;
        row.into_result()
    })
    .collect()
}

/// 提取搜索条件中的正文文本，供 embedding 查询使用。
///
/// 文件名、路径、类型和日期筛选不会进入查询向量；返回 `None` 表示这是一个只含
/// 元数据筛选的查询，调用方应只执行 lexical 分支。
pub fn extract_search_text(query: &str) -> Result<Option<String>, SearchError> {
    Ok(parse_search_query(query)?.text_query)
}

/// 执行 lexical + vector 的本地混合检索。
///
/// lexical 分支始终先执行并保留其过滤、BM25 和 snippet 语义；向量分支只使用
/// 已登记的指定模型版本。缺少该版本或没有向量时安全退回 lexical 结果，因此
/// embedding 索引不可用不会让核心搜索失效。两侧使用 Reciprocal Rank Fusion，
/// 不把 BM25 和余弦相似度这两个不同量纲直接相加。
pub fn search_documents_hybrid(
    connection: &Connection,
    query: &str,
    query_vector: &[f32],
    model_id: &str,
    model_version: &str,
    limit: usize,
) -> Result<Vec<HybridSearchResult>, SearchError> {
    if !(1..=MAX_SEARCH_LIMIT).contains(&limit) {
        return Err(SearchError::InvalidLimit);
    }

    let parsed = parse_search_query(query)?;
    let candidate_limit = hybrid_candidate_limit(limit);
    let lexical = search_documents(connection, query, candidate_limit)?;

    if parsed.fts_query.is_none() {
        return Ok(fuse_results(lexical, Vec::new(), limit));
    }

    let Some(dimensions) = embedding_model_dimensions(connection, model_id, model_version)? else {
        return Ok(fuse_results(lexical, Vec::new(), limit));
    };
    validate_query_vector(query_vector, dimensions)?;

    let semantic = search_semantic_candidates(
        connection,
        &parsed,
        query_vector,
        model_id,
        model_version,
        dimensions,
        candidate_limit,
    )?;

    Ok(fuse_results(lexical, semantic, limit))
}

fn hybrid_candidate_limit(limit: usize) -> usize {
    limit.saturating_mul(5).clamp(50, MAX_SEARCH_LIMIT)
}

fn embedding_model_dimensions(
    connection: &Connection,
    model_id: &str,
    model_version: &str,
) -> Result<Option<usize>, SearchError> {
    if model_id.trim().is_empty() || model_version.trim().is_empty() {
        return Err(SearchError::InvalidEmbedding);
    }

    let dimensions = connection
        .query_row(
            "SELECT dimensions
             FROM embedding_models
             WHERE model_id = ?1 AND model_version = ?2",
            params![model_id, model_version],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|source| SearchError::Query {
            operation: "embedding_model_read",
            source,
        })?;

    let Some(dimensions) = dimensions else {
        return Ok(None);
    };
    let dimensions = usize::try_from(dimensions)
        .ok()
        .filter(|value| *value > 0 && *value <= super::MAX_EMBEDDING_DIMENSIONS)
        .ok_or(SearchError::InvalidStoredEmbedding)?;
    Ok(Some(dimensions))
}

fn validate_query_vector(vector: &[f32], dimensions: usize) -> Result<(), SearchError> {
    if vector.len() != dimensions
        || vector.iter().any(|value| !value.is_finite())
        || squared_norm(vector) <= f32::EPSILON
    {
        return Err(SearchError::InvalidEmbedding);
    }
    Ok(())
}

fn search_semantic_candidates(
    connection: &Connection,
    parsed: &ParsedSearchQuery,
    query_vector: &[f32],
    model_id: &str,
    model_version: &str,
    dimensions: usize,
    limit: usize,
) -> Result<Vec<SemanticCandidate>, SearchError> {
    let mut sql = String::from(SEMANTIC_SELECT_SQL);
    sql.push_str("WHERE document_embeddings.model_id = ?1\n");
    sql.push_str("  AND document_embeddings.model_version = ?2\n");
    let mut parameter_index = 3;
    append_metadata_filters(&mut sql, &mut parameter_index);

    let mut parameters = vec![
        Value::Text(model_id.to_owned()),
        Value::Text(model_version.to_owned()),
    ];
    parameters.extend(bind_filter_parameters(parsed));

    let mut statement = connection
        .prepare(&sql)
        .map_err(|source| SearchError::Query {
            operation: "embedding_prepare",
            source,
        })?;
    let rows = statement
        .query_map(params_from_iter(parameters), |row| {
            Ok((
                RawSearchResult {
                    document_id: row.get(0)?,
                    source_kind: row.get(1)?,
                    source_path_key: row.get(2)?,
                    title: row.get(3)?,
                    file_name: row.get(4)?,
                    extension: row.get(5)?,
                    file_type: row.get(6)?,
                    modified_at: row.get(7)?,
                    created_at: row.get(8)?,
                    accessed_at: row.get(9)?,
                    line_start: row.get(10)?,
                    line_end: row.get(11)?,
                    relevance: None,
                    title_snippet: None,
                    body_snippet: None,
                },
                row.get::<_, i64>(12)?,
                row.get::<_, Vec<u8>>(13)?,
            ))
        })
        .map_err(|source| SearchError::Query {
            operation: "embedding_query",
            source,
        })?;

    let query_norm = squared_norm(query_vector).sqrt();
    let mut candidates = BinaryHeap::with_capacity(limit);
    for row in rows {
        let (raw, stored_dimensions, vector_bytes) = row.map_err(|source| SearchError::Query {
            operation: "embedding_read",
            source,
        })?;
        let Ok(stored_dimensions) = usize::try_from(stored_dimensions) else {
            continue;
        };
        if stored_dimensions != dimensions {
            continue;
        }
        let Ok(vector) = decode_vector(&vector_bytes, stored_dimensions) else {
            continue;
        };
        let similarity = cosine_similarity(query_vector, query_norm, &vector);
        let result = raw.into_result()?;
        let candidate = RankedSemanticCandidate { result, similarity };
        if candidates.len() < limit {
            candidates.push(candidate);
        } else if candidates
            .peek()
            .is_some_and(|worst| candidate_is_better(&candidate, worst))
        {
            candidates.pop();
            candidates.push(candidate);
        }
    }

    let mut candidates = candidates.into_vec();
    candidates.sort_by(|left, right| {
        right
            .similarity
            .total_cmp(&left.similarity)
            .then_with(|| left.result.document_id.cmp(&right.result.document_id))
    });
    Ok(candidates
        .into_iter()
        .map(|candidate| SemanticCandidate {
            result: candidate.result,
            similarity: candidate.similarity,
        })
        .collect())
}

fn candidate_is_better(
    candidate: &RankedSemanticCandidate,
    worst: &RankedSemanticCandidate,
) -> bool {
    candidate.similarity > worst.similarity
        || (candidate.similarity == worst.similarity
            && candidate.result.document_id < worst.result.document_id)
}

fn cosine_similarity(left: &[f32], left_norm: f32, right: &[f32]) -> f32 {
    let right_norm = squared_norm(right).sqrt();
    if right_norm <= f32::EPSILON {
        return -1.0;
    }

    (left
        .iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum::<f32>()
        / (left_norm * right_norm))
        .clamp(-1.0, 1.0)
}

fn squared_norm(vector: &[f32]) -> f32 {
    vector.iter().map(|value| value * value).sum()
}

#[derive(Debug)]
struct RankedSemanticCandidate {
    result: SearchResult,
    similarity: f32,
}

impl PartialEq for RankedSemanticCandidate {
    fn eq(&self, other: &Self) -> bool {
        self.similarity == other.similarity && self.result.document_id == other.result.document_id
    }
}

impl Eq for RankedSemanticCandidate {}

impl Ord for RankedSemanticCandidate {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .similarity
            .total_cmp(&self.similarity)
            .then_with(|| self.result.document_id.cmp(&other.result.document_id))
    }
}

impl PartialOrd for RankedSemanticCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug)]
struct SemanticCandidate {
    result: SearchResult,
    similarity: f32,
}

fn fuse_results(
    lexical: Vec<SearchResult>,
    semantic: Vec<SemanticCandidate>,
    limit: usize,
) -> Vec<HybridSearchResult> {
    #[derive(Debug)]
    struct FusionEntry {
        result: SearchResult,
        lexical_rank: Option<usize>,
        semantic_rank: Option<usize>,
        semantic_similarity: Option<f32>,
    }

    let mut entries = BTreeMap::<String, FusionEntry>::new();
    for (index, result) in lexical.into_iter().enumerate() {
        let document_id = result.document_id.clone();
        entries.insert(
            document_id,
            FusionEntry {
                result,
                lexical_rank: Some(index + 1),
                semantic_rank: None,
                semantic_similarity: None,
            },
        );
    }

    for (index, candidate) in semantic.into_iter().enumerate() {
        let document_id = candidate.result.document_id.clone();
        if let Some(entry) = entries.get_mut(&document_id) {
            entry.semantic_rank = Some(index + 1);
            entry.semantic_similarity = Some(candidate.similarity);
        } else {
            entries.insert(
                document_id,
                FusionEntry {
                    result: candidate.result,
                    lexical_rank: None,
                    semantic_rank: Some(index + 1),
                    semantic_similarity: Some(candidate.similarity),
                },
            );
        }
    }

    let mut results = entries
        .into_values()
        .map(|entry| HybridSearchResult {
            fusion_score: reciprocal_rank_score(entry.lexical_rank)
                + reciprocal_rank_score(entry.semantic_rank),
            result: entry.result,
            lexical_rank: entry.lexical_rank,
            semantic_rank: entry.semantic_rank,
            semantic_similarity: entry.semantic_similarity,
        })
        .collect::<Vec<_>>();
    results.sort_by(|left, right| {
        right
            .fusion_score
            .total_cmp(&left.fusion_score)
            .then_with(|| left.result.document_id.cmp(&right.result.document_id))
    });
    results.truncate(limit);
    results
}

fn reciprocal_rank_score(rank: Option<usize>) -> f64 {
    rank.map(|rank| 1.0 / (60.0 + rank as f64)).unwrap_or(0.0)
}

fn build_search_sql(has_text_query: bool) -> String {
    let mut sql = String::from(if has_text_query {
        SEARCH_TEXT_SELECT_SQL
    } else {
        SEARCH_SELECT_SQL
    });
    let mut parameter_index = 1;

    if has_text_query {
        sql.push_str("WHERE documents_fts MATCH ?1\n");
        parameter_index += 1;
    } else {
        sql.push_str("WHERE 1\n");
    }

    append_metadata_filters(&mut sql, &mut parameter_index);

    if has_text_query {
        sql.push_str(&format!(
            "ORDER BY relevance DESC, documents.document_id COLLATE BINARY ASC\nLIMIT ?{parameter_index}"
        ));
    } else {
        sql.push_str(&format!(
            "ORDER BY documents.document_id COLLATE BINARY ASC\nLIMIT ?{parameter_index}"
        ));
    }
    sql
}

fn append_contains_filter(sql: &mut String, parameter_index: &mut usize, expression: &str) {
    let index = *parameter_index;
    sql.push_str(&format!(
        "  AND (?{index} IS NULL OR instr({expression}, lower(?{index})) > 0)\n"
    ));
    *parameter_index += 1;
}

fn append_metadata_filters(sql: &mut String, parameter_index: &mut usize) {
    append_contains_filter(
        sql,
        parameter_index,
        "lower(COALESCE(file_metadata.file_name, documents.source_path_display))",
    );
    append_contains_filter(sql, parameter_index, "lower(documents.source_path_display)");
    append_exact_filter(
        sql,
        parameter_index,
        "lower(COALESCE(file_metadata.extension, ''))",
    );
    append_exact_filter(
        sql,
        parameter_index,
        "lower(COALESCE(file_metadata.file_type, ''))",
    );
    append_comparison_filter(sql, parameter_index, "file_metadata.modified_at", ">=");
    append_comparison_filter(sql, parameter_index, "file_metadata.modified_at", "<");
    append_comparison_filter(sql, parameter_index, "file_metadata.created_at", ">=");
    append_comparison_filter(sql, parameter_index, "file_metadata.created_at", "<");
    append_comparison_filter(sql, parameter_index, "file_metadata.accessed_at", ">=");
    append_comparison_filter(sql, parameter_index, "file_metadata.accessed_at", "<");
}

fn append_exact_filter(sql: &mut String, parameter_index: &mut usize, expression: &str) {
    let index = *parameter_index;
    sql.push_str(&format!(
        "  AND (?{index} IS NULL OR {expression} = lower(?{index}))\n"
    ));
    *parameter_index += 1;
}

fn append_comparison_filter(
    sql: &mut String,
    parameter_index: &mut usize,
    expression: &str,
    operator: &str,
) {
    let index = *parameter_index;
    sql.push_str(&format!(
        "  AND (?{index} IS NULL OR {expression} {operator} ?{index})\n"
    ));
    *parameter_index += 1;
}

fn bind_parameters(parsed: &ParsedSearchQuery, limit: i64, has_text_query: bool) -> Vec<Value> {
    let mut parameters = Vec::with_capacity(if has_text_query { 12 } else { 11 });
    if has_text_query {
        parameters.push(
            parsed
                .fts_query
                .as_ref()
                .map(|value| Value::Text(value.clone()))
                .unwrap_or(Value::Null),
        );
    }
    parameters.extend(bind_filter_parameters(parsed));
    parameters.push(Value::Integer(limit));
    parameters
}

fn bind_filter_parameters(parsed: &ParsedSearchQuery) -> Vec<Value> {
    vec![
        optional_text(parsed.filename.as_deref()),
        optional_text(parsed.path.as_deref()),
        optional_text(parsed.extension.as_deref()),
        optional_text(parsed.file_type.as_deref()),
        optional_integer(parsed.modified.lower),
        optional_integer(parsed.modified.upper),
        optional_integer(parsed.created.lower),
        optional_integer(parsed.created.upper),
        optional_integer(parsed.accessed.lower),
        optional_integer(parsed.accessed.upper),
    ]
}

fn optional_text(value: Option<&str>) -> Value {
    value
        .map(|value| Value::Text(value.to_owned()))
        .unwrap_or(Value::Null)
}

fn optional_integer(value: Option<i64>) -> Value {
    value.map(Value::Integer).unwrap_or(Value::Null)
}

#[derive(Debug)]
struct RawSearchResult {
    document_id: String,
    source_kind: String,
    source_path_key: Vec<u8>,
    title: String,
    file_name: Option<String>,
    extension: Option<String>,
    file_type: Option<String>,
    modified_at: Option<i64>,
    created_at: Option<i64>,
    accessed_at: Option<i64>,
    line_start: Option<i64>,
    line_end: Option<i64>,
    relevance: Option<f64>,
    title_snippet: Option<String>,
    body_snippet: Option<String>,
}

impl RawSearchResult {
    fn into_result(self) -> Result<SearchResult, SearchError> {
        if self.source_kind != "local_file" {
            return Err(SearchError::UnsupportedStoredSource);
        }

        let source_path = path_from_key(&self.source_path_key)
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or(SearchError::InvalidStoredPath)?;
        let (line_start, line_end) = decode_stored_location(self.line_start, self.line_end)
            .map_err(|_| SearchError::InvalidStoredLocation)?;
        let snippets = [self.title_snippet, self.body_snippet];
        let snippet = snippets
            .iter()
            .flatten()
            .find(|value| value.contains('⟦') && value.contains('⟧'))
            .cloned()
            .or_else(|| {
                snippets
                    .into_iter()
                    .flatten()
                    .find(|value| !value.is_empty())
            });

        Ok(SearchResult {
            document_id: self.document_id,
            source_path,
            title: self.title,
            file_name: self.file_name,
            extension: self.extension,
            file_type: self.file_type,
            modified_at: self.modified_at,
            created_at: self.created_at,
            accessed_at: self.accessed_at,
            line_start,
            line_end,
            relevance: self.relevance,
            snippet,
        })
    }
}

/// 本地搜索错误。
#[derive(Debug)]
pub enum SearchError {
    EmptyQuery,
    InvalidQuerySyntax,
    UnsupportedFilter,
    EmptyFilterValue,
    InvalidDate,
    ConflictingFilter,
    InvalidLimit,
    InvalidEmbedding,
    InvalidStoredEmbedding,
    InvalidStoredPath,
    InvalidStoredLocation,
    UnsupportedStoredSource,
    Query {
        operation: &'static str,
        source: rusqlite::Error,
    },
}

impl SearchError {
    /// 返回不包含查询内容、路径、正文或原始 SQLite 信息的安全错误分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::EmptyQuery => "search_query_empty",
            Self::InvalidQuerySyntax => "search_query_invalid",
            Self::UnsupportedFilter => "search_filter_unsupported",
            Self::EmptyFilterValue => "search_filter_empty",
            Self::InvalidDate => "search_date_invalid",
            Self::ConflictingFilter => "search_filter_conflict",
            Self::InvalidLimit => "search_limit_invalid",
            Self::InvalidEmbedding => "search_embedding_invalid",
            Self::InvalidStoredEmbedding => "search_embedding_corrupt",
            Self::InvalidStoredPath => "search_result_path_corrupt",
            Self::InvalidStoredLocation => "search_result_location_corrupt",
            Self::UnsupportedStoredSource => "search_result_source_unsupported",
            Self::Query { .. } => "search_query",
        }
    }

    /// 返回可以直接展示给用户的非敏感中文说明。
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::EmptyQuery => "搜索条件不能为空。",
            Self::InvalidQuerySyntax => "搜索条件格式无效，请检查关键词、引号和筛选语法。",
            Self::UnsupportedFilter => "搜索筛选字段不受支持。",
            Self::EmptyFilterValue => "搜索筛选值不能为空。",
            Self::InvalidDate => "搜索日期无效，请使用 YYYY-MM-DD 格式。",
            Self::ConflictingFilter => "相同搜索筛选条件重复或互相冲突。",
            Self::InvalidLimit => "搜索结果数量必须在 1 到 1000 之间。",
            Self::InvalidEmbedding => "搜索向量无效。",
            Self::InvalidStoredEmbedding => "本地向量索引不可用。",
            Self::InvalidStoredPath
            | Self::InvalidStoredLocation
            | Self::UnsupportedStoredSource => "搜索结果中的文档记录不可用。",
            Self::Query { .. } => "本地搜索暂时不可用。",
        }
    }
}

impl fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "本地搜索失败: {}", self.kind())
    }
}

impl Error for SearchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Query { source, .. } => Some(source),
            Self::EmptyQuery
            | Self::InvalidQuerySyntax
            | Self::UnsupportedFilter
            | Self::EmptyFilterValue
            | Self::InvalidDate
            | Self::ConflictingFilter
            | Self::InvalidLimit
            | Self::InvalidEmbedding
            | Self::InvalidStoredEmbedding
            | Self::InvalidStoredPath
            | Self::InvalidStoredLocation
            | Self::UnsupportedStoredSource => None,
        }
    }
}

#[derive(Debug, Default)]
struct ParsedSearchQuery {
    fts_query: Option<String>,
    text_query: Option<String>,
    filename: Option<String>,
    path: Option<String>,
    extension: Option<String>,
    file_type: Option<String>,
    modified: DateRange,
    created: DateRange,
    accessed: DateRange,
}

impl ParsedSearchQuery {
    fn has_filter(&self) -> bool {
        self.filename.is_some()
            || self.path.is_some()
            || self.extension.is_some()
            || self.file_type.is_some()
            || self.modified.has_value()
            || self.created.has_value()
            || self.accessed.has_value()
    }
}

#[derive(Debug, Default)]
struct DateRange {
    lower: Option<i64>,
    upper: Option<i64>,
}

impl DateRange {
    fn has_value(&self) -> bool {
        self.lower.is_some() || self.upper.is_some()
    }

    fn add(&mut self, operator: DateOperator, lower: i64, upper: i64) -> Result<(), SearchError> {
        match operator {
            DateOperator::Equal => {
                if self.has_value() {
                    return Err(SearchError::ConflictingFilter);
                }
                self.lower = Some(lower);
                self.upper = Some(upper);
            }
            DateOperator::GreaterOrEqual => {
                if self.lower.is_some() {
                    return Err(SearchError::ConflictingFilter);
                }
                self.lower = Some(lower);
            }
            DateOperator::LessOrEqual => {
                if self.upper.is_some() {
                    return Err(SearchError::ConflictingFilter);
                }
                self.upper = Some(upper);
            }
        }

        if let (Some(lower), Some(upper)) = (self.lower, self.upper) {
            if lower >= upper {
                return Err(SearchError::ConflictingFilter);
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum DateField {
    Modified,
    Created,
    Accessed,
}

#[derive(Debug, Clone, Copy)]
enum DateOperator {
    Equal,
    GreaterOrEqual,
    LessOrEqual,
}

#[derive(Debug)]
enum Filter {
    Filename(String),
    Path(String),
    Extension(String),
    FileType(String),
    Date {
        field: DateField,
        operator: DateOperator,
        value: String,
    },
}

#[derive(Debug)]
struct QueryToken {
    value: String,
    starts_with_quote: bool,
    quoted: bool,
}

fn parse_search_query(query: &str) -> Result<ParsedSearchQuery, SearchError> {
    if query.contains('\0') {
        return Err(SearchError::InvalidQuerySyntax);
    }

    if query.trim().is_empty() {
        return Err(SearchError::EmptyQuery);
    }

    let tokens = tokenize(query)?;
    let mut parsed = ParsedSearchQuery::default();
    let mut text_terms = Vec::new();

    for token in tokens {
        if !token.starts_with_quote {
            if let Some(filter) = parse_filter(&token.value)? {
                apply_filter(&mut parsed, filter)?;
                continue;
            }
        }

        let value = token.value.trim();
        if value.is_empty() {
            return Err(SearchError::InvalidQuerySyntax);
        }
        if !token.quoted && is_reserved_operator(value) {
            return Err(SearchError::InvalidQuerySyntax);
        }
        if value.contains('\0') {
            return Err(SearchError::InvalidQuerySyntax);
        }
        text_terms.push(value.to_owned());
    }

    if text_terms.is_empty() && !parsed.has_filter() {
        return Err(SearchError::EmptyQuery);
    }

    if !text_terms.is_empty() {
        parsed.text_query = Some(text_terms.join(" "));
        parsed.fts_query = Some(
            text_terms
                .iter()
                .map(|term| quote_fts_phrase(term))
                .collect::<Vec<_>>()
                .join(" AND "),
        );
    }

    Ok(parsed)
}

fn tokenize(query: &str) -> Result<Vec<QueryToken>, SearchError> {
    let mut characters = query.chars().peekable();
    let mut tokens = Vec::new();

    loop {
        while characters
            .peek()
            .is_some_and(|character| character.is_whitespace())
        {
            characters.next();
        }

        if characters.peek().is_none() {
            break;
        }

        let starts_with_quote = characters.peek() == Some(&'"');
        let mut value = String::new();
        let mut quoted = false;
        let mut inside_quote = false;
        let mut closed_quote = false;

        while let Some(&character) = characters.peek() {
            if !inside_quote && character.is_whitespace() {
                break;
            }

            if character == '"' {
                characters.next();
                quoted = true;

                if inside_quote {
                    if characters.peek() == Some(&'"') {
                        characters.next();
                        value.push('"');
                    } else {
                        inside_quote = false;
                        closed_quote = true;
                    }
                } else {
                    if closed_quote {
                        return Err(SearchError::InvalidQuerySyntax);
                    }
                    inside_quote = true;
                }
                continue;
            }

            if closed_quote {
                return Err(SearchError::InvalidQuerySyntax);
            }

            characters.next();
            value.push(character);
        }

        if inside_quote || value.is_empty() {
            return Err(SearchError::InvalidQuerySyntax);
        }

        tokens.push(QueryToken {
            value,
            starts_with_quote,
            quoted,
        });
    }

    Ok(tokens)
}

fn parse_filter(token: &str) -> Result<Option<Filter>, SearchError> {
    if let Some(separator) = token.find(':') {
        let field = parse_field(&token[..separator]).ok_or(SearchError::UnsupportedFilter)?;
        let value = token[separator + 1..].trim();

        if value.is_empty() {
            return Err(SearchError::EmptyFilterValue);
        }

        return match field {
            SearchField::Filename => Ok(Some(Filter::Filename(value.to_owned()))),
            SearchField::Path => Ok(Some(Filter::Path(value.to_owned()))),
            SearchField::Extension => Ok(Some(Filter::Extension(value.to_owned()))),
            SearchField::FileType => Ok(Some(Filter::FileType(value.to_owned()))),
            SearchField::Modified | SearchField::Created | SearchField::Accessed => {
                let (operator, date) = parse_date_expression(value)?;
                Ok(Some(Filter::Date {
                    field: field.date_field().ok_or(SearchError::InvalidQuerySyntax)?,
                    operator,
                    value: date.to_owned(),
                }))
            }
        };
    }

    for (operator_text, operator) in [
        (">=", DateOperator::GreaterOrEqual),
        ("<=", DateOperator::LessOrEqual),
        ("=", DateOperator::Equal),
    ] {
        if let Some(separator) = token.find(operator_text) {
            let field = parse_field(&token[..separator]).ok_or(SearchError::UnsupportedFilter)?;
            let Some(field) = field.date_field() else {
                return Err(SearchError::InvalidQuerySyntax);
            };
            let value = token[separator + operator_text.len()..].trim();
            if value.is_empty() {
                return Err(SearchError::EmptyFilterValue);
            }
            return Ok(Some(Filter::Date {
                field,
                operator,
                value: value.to_owned(),
            }));
        }
    }

    if token.contains('<') || token.contains('>') {
        return Err(SearchError::InvalidQuerySyntax);
    }

    Ok(None)
}

#[derive(Debug, Clone, Copy)]
enum SearchField {
    Filename,
    Path,
    Extension,
    FileType,
    Modified,
    Created,
    Accessed,
}

impl SearchField {
    fn date_field(self) -> Option<DateField> {
        match self {
            Self::Modified => Some(DateField::Modified),
            Self::Created => Some(DateField::Created),
            Self::Accessed => Some(DateField::Accessed),
            Self::Filename | Self::Path | Self::Extension | Self::FileType => None,
        }
    }
}

fn parse_field(value: &str) -> Option<SearchField> {
    if value.eq_ignore_ascii_case("filename") {
        Some(SearchField::Filename)
    } else if value.eq_ignore_ascii_case("path") {
        Some(SearchField::Path)
    } else if value.eq_ignore_ascii_case("ext") || value.eq_ignore_ascii_case("extension") {
        Some(SearchField::Extension)
    } else if value.eq_ignore_ascii_case("type") {
        Some(SearchField::FileType)
    } else if value.eq_ignore_ascii_case("modified") || value.eq_ignore_ascii_case("date") {
        Some(SearchField::Modified)
    } else if value.eq_ignore_ascii_case("created") {
        Some(SearchField::Created)
    } else if value.eq_ignore_ascii_case("accessed") {
        Some(SearchField::Accessed)
    } else {
        None
    }
}

fn parse_date_expression(value: &str) -> Result<(DateOperator, &str), SearchError> {
    if let Some(value) = value.strip_prefix(">=") {
        Ok((DateOperator::GreaterOrEqual, value.trim()))
    } else if let Some(value) = value.strip_prefix("<=") {
        Ok((DateOperator::LessOrEqual, value.trim()))
    } else if let Some(value) = value.strip_prefix('=') {
        Ok((DateOperator::Equal, value.trim()))
    } else if value.starts_with('<') || value.starts_with('>') {
        Err(SearchError::InvalidQuerySyntax)
    } else {
        Ok((DateOperator::Equal, value))
    }
}

fn apply_filter(parsed: &mut ParsedSearchQuery, filter: Filter) -> Result<(), SearchError> {
    match filter {
        Filter::Filename(value) => {
            set_unique(&mut parsed.filename, value)?;
        }
        Filter::Path(value) => {
            set_unique(&mut parsed.path, value)?;
        }
        Filter::Extension(value) => {
            let value = value.trim_start_matches('.').to_ascii_lowercase();
            if value.is_empty() || value.chars().any(char::is_whitespace) {
                return Err(SearchError::InvalidQuerySyntax);
            }
            set_unique(&mut parsed.extension, value)?;
        }
        Filter::FileType(value) => {
            set_unique(&mut parsed.file_type, value.trim().to_owned())?;
        }
        Filter::Date {
            field,
            operator,
            value,
        } => {
            let (lower, upper) = parse_date(value.trim())?;
            match field {
                DateField::Modified => parsed.modified.add(operator, lower, upper)?,
                DateField::Created => parsed.created.add(operator, lower, upper)?,
                DateField::Accessed => parsed.accessed.add(operator, lower, upper)?,
            }
        }
    }

    Ok(())
}

fn set_unique<T>(slot: &mut Option<T>, value: T) -> Result<(), SearchError> {
    if slot.is_some() {
        return Err(SearchError::ConflictingFilter);
    }
    *slot = Some(value);
    Ok(())
}

fn parse_date(value: &str) -> Result<(i64, i64), SearchError> {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-') {
        return Err(SearchError::InvalidDate);
    }

    let year = decimal_number(bytes, 0, 4).ok_or(SearchError::InvalidDate)?;
    let month = decimal_number(bytes, 5, 2).ok_or(SearchError::InvalidDate)?;
    let day = decimal_number(bytes, 8, 2).ok_or(SearchError::InvalidDate)?;

    if year == 0 || !(1..=12).contains(&month) {
        return Err(SearchError::InvalidDate);
    }

    let year = i32::try_from(year).map_err(|_| SearchError::InvalidDate)?;
    if !(1..=days_in_month(year, month)).contains(&day) {
        return Err(SearchError::InvalidDate);
    }

    let days = days_from_civil(year, month, day);
    let start = days
        .checked_mul(86_400_000)
        .ok_or(SearchError::InvalidDate)?;
    let end = start
        .checked_add(86_400_000)
        .ok_or(SearchError::InvalidDate)?;
    Ok((start, end))
}

fn decimal_number(bytes: &[u8], start: usize, length: usize) -> Option<u32> {
    bytes
        .get(start..start + length)?
        .iter()
        .try_fold(0_u32, |number, byte| {
            byte.is_ascii_digit()
                .then_some(number * 10 + u32::from(byte - b'0'))
        })
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    era * 146_097 + day_of_era - 719_468
}

fn quote_fts_phrase(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn is_reserved_operator(value: &str) -> bool {
    value.eq_ignore_ascii_case("and")
        || value.eq_ignore_ascii_case("or")
        || value.eq_ignore_ascii_case("not")
        || value.eq_ignore_ascii_case("near")
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use crate::{
        upsert_document, upsert_document_embeddings, upsert_file_metadata, DocumentEmbedding,
        DocumentRecord, EmbeddingModel, FileMetadata, EMBEDDING_FINGERPRINT_BYTES,
    };

    static NEXT_TEMPORARY_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    struct TemporaryDirectory {
        path: PathBuf,
    }

    impl TemporaryDirectory {
        fn new() -> Self {
            let id = NEXT_TEMPORARY_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("nexus-db-search-{}-{}", std::process::id(), id));
            fs::create_dir_all(&path).expect("创建搜索测试目录失败");
            Self { path }
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

    fn test_document(id: &str, source_path: PathBuf, title: &str, body: &str) -> DocumentRecord {
        DocumentRecord {
            id: id.to_owned(),
            source_path,
            title: title.to_owned(),
            body: body.to_owned(),
            line_start: None,
            line_end: None,
        }
    }

    fn test_metadata(
        path: &Path,
        file_name: &str,
        extension: &str,
        file_type: &str,
        modified_at: i64,
    ) -> FileMetadata {
        FileMetadata {
            path: path.to_path_buf(),
            file_name: file_name.to_owned(),
            extension: Some(extension.to_owned()),
            size_bytes: 1,
            modified_at: Some(modified_at),
            created_at: Some(modified_at),
            accessed_at: Some(modified_at),
            file_type: Some(file_type.to_owned()),
        }
    }

    fn result_ids(results: &[SearchResult]) -> Vec<&str> {
        results
            .iter()
            .map(|result| result.document_id.as_str())
            .collect()
    }

    fn test_embedding_model() -> EmbeddingModel {
        EmbeddingModel {
            model_id: "test-model".to_owned(),
            model_version: "1".to_owned(),
            provider_kind: "test".to_owned(),
            dimensions: 4,
        }
    }

    fn test_embedding(document_id: &str, vector: [f32; 4]) -> DocumentEmbedding {
        DocumentEmbedding {
            document_id: document_id.to_owned(),
            model_id: "test-model".to_owned(),
            model_version: "1".to_owned(),
            source_fingerprint: vec![1; EMBEDDING_FINGERPRINT_BYTES],
            vector: vector.to_vec(),
        }
    }

    #[test]
    fn parses_keywords_phrases_and_filters_without_exposing_raw_fts_syntax() {
        let parsed = parse_search_query(
            r#"alpha "beta gamma" filename:"project notes" ext:.MD modified>=2024-01-01"#,
        )
        .expect("解析搜索条件失败");

        assert_eq!(
            parsed.fts_query.as_deref(),
            Some("\"alpha\" AND \"beta gamma\"")
        );
        assert_eq!(parsed.filename.as_deref(), Some("project notes"));
        assert_eq!(parsed.extension.as_deref(), Some("md"));
        assert_eq!(
            parsed.modified.lower,
            Some(parse_date("2024-01-01").unwrap().0)
        );
        assert!(parsed.modified.upper.is_none());
    }

    #[test]
    fn parses_date_boundaries_and_leap_days() {
        let (_, february_end) = parse_date("2024-02-29").expect("闰日解析失败");
        let (march_start, _) = parse_date("2024-03-01").expect("三月日期解析失败");
        assert_eq!(february_end, march_start);

        assert!(matches!(
            parse_date("2023-02-29"),
            Err(SearchError::InvalidDate)
        ));
        assert!(matches!(
            parse_date("2024-13-01"),
            Err(SearchError::InvalidDate)
        ));
    }

    #[test]
    fn rejects_invalid_query_syntax_and_conflicting_filters() {
        assert!(matches!(
            parse_search_query("\"unterminated"),
            Err(SearchError::InvalidQuerySyntax)
        ));
        assert!(matches!(
            parse_search_query("unknown:value"),
            Err(SearchError::UnsupportedFilter)
        ));
        assert!(matches!(
            parse_search_query("ext:"),
            Err(SearchError::EmptyFilterValue)
        ));
        assert!(matches!(
            parse_search_query("modified>=2024-01-01 modified>=2024-02-01"),
            Err(SearchError::ConflictingFilter)
        ));
        assert!(matches!(
            parse_search_query("modified>=2024-02-30"),
            Err(SearchError::InvalidDate)
        ));
        assert!(matches!(
            parse_search_query("filename:secret\0"),
            Err(SearchError::InvalidQuerySyntax)
        ));
        assert!(matches!(
            parse_search_query("AND"),
            Err(SearchError::InvalidQuerySyntax)
        ));
    }

    #[test]
    fn searches_text_phrases_metadata_filters_and_date_ranges() {
        let temporary_directory = TemporaryDirectory::new();
        let connection = crate::initialize_database(temporary_directory.database_path())
            .expect("初始化搜索测试数据库失败");
        let alpha_path = temporary_directory.child_path("notes/project-alpha.md");
        let beta_path = temporary_directory.child_path("archive/project-beta.txt");
        let (january_start, _) = parse_date("2024-01-01").expect("解析一月日期失败");
        let (february_start, _) = parse_date("2024-02-01").expect("解析二月日期失败");

        let alpha = test_document(
            "file:alpha",
            alpha_path.clone(),
            "Project Alpha",
            "green river planning notes",
        );
        let beta = test_document(
            "file:beta",
            beta_path.clone(),
            "Project Beta",
            "green mountain archive",
        );
        upsert_document(&connection, &alpha).expect("写入 alpha 文档失败");
        upsert_document(&connection, &beta).expect("写入 beta 文档失败");
        upsert_file_metadata(
            &connection,
            &test_metadata(
                &alpha_path,
                "project-alpha.md",
                "md",
                "text/markdown",
                january_start,
            ),
        )
        .expect("写入 alpha 元数据失败");
        upsert_file_metadata(
            &connection,
            &test_metadata(
                &beta_path,
                "project-beta.txt",
                "txt",
                "text/plain",
                february_start,
            ),
        )
        .expect("写入 beta 元数据失败");

        let results = search_documents(&connection, "green", 10).expect("关键词搜索失败");
        let mut ids = result_ids(&results);
        ids.sort_unstable();
        assert_eq!(ids, vec!["file:alpha", "file:beta"]);
        assert!(results.iter().all(|result| result.relevance.is_some()));
        assert!(results.iter().all(|result| result.snippet.is_some()));

        let results = search_documents(&connection, "\"green river\"", 10).expect("短语搜索失败");
        assert_eq!(result_ids(&results), vec!["file:alpha"]);

        let results = search_documents(
            &connection,
            "filename:project-alpha ext:MD type:text/markdown modified<=2024-01-31",
            10,
        )
        .expect("文件元数据过滤搜索失败");
        assert_eq!(result_ids(&results), vec!["file:alpha"]);

        let results = search_documents(&connection, "path:archive modified>=2024-02-01", 10)
            .expect("路径和日期过滤搜索失败");
        assert_eq!(result_ids(&results), vec!["file:beta"]);
        assert_eq!(results[0].file_name.as_deref(), Some("project-beta.txt"));
        assert_eq!(results[0].extension.as_deref(), Some("txt"));
        assert_eq!(results[0].file_type.as_deref(), Some("text/plain"));
    }

    #[test]
    fn ranks_title_matches_before_body_matches_and_uses_id_tiebreak() {
        let temporary_directory = TemporaryDirectory::new();
        let connection = crate::initialize_database(temporary_directory.database_path())
            .expect("初始化排序测试数据库失败");

        for (id, name, title, body) in [
            (
                "file:title",
                "title.md",
                "needle",
                "unrelated context words",
            ),
            ("file:body", "body.md", "other", "needle"),
            ("file:a-tie", "a-tie.md", "shared", "same body"),
            ("file:b-tie", "b-tie.md", "shared", "same body"),
        ] {
            let document = test_document(id, temporary_directory.child_path(name), title, body);
            upsert_document(&connection, &document).expect("写入排序测试文档失败");
        }

        let results = search_documents(&connection, "needle", 10).expect("相关性排序搜索失败");
        assert_eq!(result_ids(&results), vec!["file:title", "file:body"]);
        assert!(
            results[0].relevance.expect("标题命中缺少相关性")
                > results[1].relevance.expect("正文命中缺少相关性")
        );
        assert!(results[0]
            .snippet
            .as_deref()
            .is_some_and(|snippet| snippet.contains("⟦needle⟧")));
        assert!(results[1]
            .snippet
            .as_deref()
            .is_some_and(|snippet| snippet.contains("⟦needle⟧")));

        let results = search_documents(&connection, "shared", 10).expect("tie-break 搜索失败");
        assert_eq!(result_ids(&results), vec!["file:a-tie", "file:b-tie"]);
        assert!(results.iter().all(|result| result.relevance.is_some()));
    }

    #[test]
    fn supports_filter_only_queries_and_bounds_result_count() {
        let temporary_directory = TemporaryDirectory::new();
        let connection = crate::initialize_database(temporary_directory.database_path())
            .expect("初始化过滤器测试数据库失败");
        for (id, name) in [("file:a", "a.md"), ("file:b", "b.md")] {
            let path = temporary_directory.child_path(name);
            let document = test_document(id, path.clone(), name, "body");
            upsert_document(&connection, &document).expect("写入过滤器测试文档失败");
            upsert_file_metadata(
                &connection,
                &test_metadata(&path, name, "md", "text/markdown", 0),
            )
            .expect("写入过滤器测试元数据失败");
        }

        let results = search_documents(&connection, "ext:md", 1).expect("扩展名过滤搜索失败");
        assert_eq!(results.len(), 1);
        assert!(results[0].relevance.is_none());
        assert!(results[0].snippet.is_none());
        assert!(matches!(
            search_documents(&connection, "", DEFAULT_SEARCH_LIMIT),
            Err(SearchError::EmptyQuery)
        ));
        assert!(matches!(
            search_documents(&connection, "ext:md", 0),
            Err(SearchError::InvalidLimit)
        ));
        assert!(matches!(
            search_documents(&connection, "ext:md", MAX_SEARCH_LIMIT + 1),
            Err(SearchError::InvalidLimit)
        ));
    }

    #[test]
    fn fuses_lexical_and_semantic_candidates_without_replacing_lexical_results() {
        let temporary_directory = TemporaryDirectory::new();
        let mut connection = crate::initialize_database(temporary_directory.database_path())
            .expect("初始化混合搜索测试数据库失败");
        let alpha = test_document(
            "file:alpha",
            temporary_directory.child_path("alpha.md"),
            "Alpha",
            "alpha lexical document",
        );
        let semantic = test_document(
            "file:semantic",
            temporary_directory.child_path("semantic.md"),
            "Semantic candidate",
            "unrelated wording",
        );
        upsert_document(&connection, &alpha).expect("写入 lexical 文档失败");
        upsert_document(&connection, &semantic).expect("写入 semantic 文档失败");
        let model = test_embedding_model();
        upsert_document_embeddings(
            &mut connection,
            &model,
            &[
                test_embedding(&alpha.id, [1.0, 0.0, 0.0, 0.0]),
                test_embedding(&semantic.id, [0.0, 1.0, 0.0, 0.0]),
            ],
        )
        .expect("写入混合搜索向量失败");

        let results = search_documents_hybrid(
            &connection,
            "alpha",
            &[0.0, 1.0, 0.0, 0.0],
            &model.model_id,
            &model.model_version,
            10,
        )
        .expect("混合搜索失败");

        let semantic_result = results
            .iter()
            .find(|result| result.result.document_id == semantic.id)
            .expect("语义候选应进入混合结果");
        assert_eq!(semantic_result.lexical_rank, None);
        assert_eq!(semantic_result.semantic_rank, Some(1));
        assert!(semantic_result
            .semantic_similarity
            .is_some_and(|value| value > 0.99));
        assert!(results
            .iter()
            .any(|result| result.result.document_id == alpha.id));
    }

    #[test]
    fn applies_metadata_filters_to_the_semantic_branch() {
        let temporary_directory = TemporaryDirectory::new();
        let mut connection = crate::initialize_database(temporary_directory.database_path())
            .expect("初始化混合过滤测试数据库失败");
        let markdown_path = temporary_directory.child_path("allowed.md");
        let text_path = temporary_directory.child_path("filtered.txt");
        let markdown = test_document("file:allowed", markdown_path.clone(), "Allowed", "topic");
        let text = test_document("file:filtered", text_path.clone(), "Filtered", "topic");
        upsert_document(&connection, &markdown).expect("写入允许文档失败");
        upsert_document(&connection, &text).expect("写入过滤文档失败");
        upsert_file_metadata(
            &connection,
            &test_metadata(&markdown_path, "allowed.md", "md", "text/markdown", 0),
        )
        .expect("写入允许文档元数据失败");
        upsert_file_metadata(
            &connection,
            &test_metadata(&text_path, "filtered.txt", "txt", "text/plain", 0),
        )
        .expect("写入过滤文档元数据失败");
        let model = test_embedding_model();
        upsert_document_embeddings(
            &mut connection,
            &model,
            &[
                test_embedding(&markdown.id, [1.0, 0.0, 0.0, 0.0]),
                test_embedding(&text.id, [0.0, 1.0, 0.0, 0.0]),
            ],
        )
        .expect("写入混合过滤向量失败");

        let results = search_documents_hybrid(
            &connection,
            "topic ext:md",
            &[0.0, 1.0, 0.0, 0.0],
            &model.model_id,
            &model.model_version,
            10,
        )
        .expect("混合过滤搜索失败");

        assert_eq!(
            result_ids(
                &results
                    .iter()
                    .map(|item| item.result.clone())
                    .collect::<Vec<_>>()
            ),
            vec![markdown.id.as_str()]
        );
        assert!(results
            .iter()
            .all(|result| result.result.document_id != text.id));
    }

    #[test]
    fn falls_back_to_lexical_search_when_the_requested_model_is_missing() {
        let temporary_directory = TemporaryDirectory::new();
        let connection = crate::initialize_database(temporary_directory.database_path())
            .expect("初始化 lexical fallback 测试数据库失败");
        let document = test_document(
            "file:fallback",
            temporary_directory.child_path("fallback.md"),
            "Fallback",
            "fallback term",
        );
        upsert_document(&connection, &document).expect("写入 fallback 文档失败");

        let results =
            search_documents_hybrid(&connection, "fallback", &[], "missing-model", "1", 10)
                .expect("缺失模型时 lexical fallback 失败");
        assert_eq!(
            result_ids(
                &results
                    .iter()
                    .map(|item| item.result.clone())
                    .collect::<Vec<_>>()
            ),
            vec![document.id.as_str()]
        );
        assert!(results.iter().all(|result| result.semantic_rank.is_none()));
    }

    #[test]
    fn skips_corrupt_stored_vectors_and_keeps_lexical_results() {
        let temporary_directory = TemporaryDirectory::new();
        let mut connection = crate::initialize_database(temporary_directory.database_path())
            .expect("初始化损坏向量回退测试数据库失败");
        let document = test_document(
            "file:corrupt-vector",
            temporary_directory.child_path("corrupt-vector.md"),
            "Corrupt vector",
            "corrupt vector term",
        );
        upsert_document(&connection, &document).expect("写入损坏向量测试文档失败");
        let model = test_embedding_model();
        upsert_document_embeddings(
            &mut connection,
            &model,
            &[test_embedding(&document.id, [1.0, 0.0, 0.0, 0.0])],
        )
        .expect("写入损坏向量测试 embedding 失败");
        connection
            .execute(
                "UPDATE document_embeddings SET vector = ?1 WHERE document_id = ?2",
                rusqlite::params![vec![0_u8; 16], &document.id],
            )
            .expect("写入损坏向量测试数据失败");

        let results = search_documents_hybrid(
            &connection,
            "corrupt",
            &[1.0, 0.0, 0.0, 0.0],
            &model.model_id,
            &model.model_version,
            10,
        )
        .expect("损坏向量 lexical fallback 失败");

        assert_eq!(
            result_ids(
                &results
                    .iter()
                    .map(|item| item.result.clone())
                    .collect::<Vec<_>>()
            ),
            vec![document.id.as_str()]
        );
        assert!(results.iter().all(|result| result.semantic_rank.is_none()));
    }

    #[test]
    fn rejects_an_invalid_query_vector_only_when_the_model_is_available() {
        let temporary_directory = TemporaryDirectory::new();
        let mut connection = crate::initialize_database(temporary_directory.database_path())
            .expect("初始化向量参数测试数据库失败");
        let document = test_document(
            "file:vector",
            temporary_directory.child_path("vector.md"),
            "Vector",
            "vector term",
        );
        upsert_document(&connection, &document).expect("写入向量参数测试文档失败");
        let model = test_embedding_model();
        upsert_document_embeddings(
            &mut connection,
            &model,
            &[test_embedding(&document.id, [1.0, 0.0, 0.0, 0.0])],
        )
        .expect("写入向量参数测试 embedding 失败");

        assert!(matches!(
            search_documents_hybrid(
                &connection,
                "vector",
                &[1.0, 0.0, 0.0],
                &model.model_id,
                &model.model_version,
                10,
            ),
            Err(SearchError::InvalidEmbedding)
        ));
    }
}
