use std::{
    fs,
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use nexus_core::{
    index_document_embeddings, EmbeddingIndexOptions, EmbeddingProvider, LocalFeatureEmbedding,
    RescanControl,
};
use nexus_db::{
    extract_search_text, initialize_database, search_documents, search_documents_hybrid,
    upsert_document, DocumentRecord, MAX_SEARCH_LIMIT,
};

const TOP_K: usize = 3;
const MEASUREMENT_ROUNDS: usize = 15;

struct CorpusDocument {
    id: &'static str,
    file_name: &'static str,
    title: &'static str,
    body: &'static str,
}

const CORPUS: &[CorpusDocument] = &[
    CorpusDocument {
        id: "doc-project-plan",
        file_name: "project-plan.md",
        title: "Quarterly Project Plan",
        body: "The quarterly plan tracks project milestones, owners, and release dates.",
    },
    CorpusDocument {
        id: "doc-meeting-notes",
        file_name: "meeting-notes.md",
        title: "Meeting Notes",
        body: "The project plan discussion covered owners, risks, and release dates.",
    },
    CorpusDocument {
        id: "doc-release-plan",
        file_name: "release-plan.md",
        title: "Release Plan",
        body: "The release plan coordinates the next project checkpoint and rollout.",
    },
    CorpusDocument {
        id: "doc-travel-plan",
        file_name: "travel-plan.md",
        title: "Travel Plan",
        body: "The travel plan lists an itinerary, hotel reservations, and calendar dates.",
    },
    CorpusDocument {
        id: "doc-backup-runbook",
        file_name: "backup-runbook.md",
        title: "Backup Runbook",
        body: "The local backup strategy protects encrypted archives and verifies restore checks.",
    },
    CorpusDocument {
        id: "doc-privacy-notes",
        file_name: "privacy-notes.md",
        title: "Privacy Notes",
        body: "Local-first storage keeps personal data on device and avoids cloud upload.",
    },
    CorpusDocument {
        id: "doc-database-migration",
        file_name: "database-migration.md",
        title: "SQLite Schema Migration",
        body: "Schema migration rebuilds the FTS index after canonical document changes.",
    },
    CorpusDocument {
        id: "doc-search-research",
        file_name: "search-research.md",
        title: "Search Evaluation Notes",
        body: "Lexical ranking and recall measurements guide future search evaluation.",
    },
    CorpusDocument {
        id: "doc-code-index",
        file_name: "code-index.md",
        title: "Parser and Markdown Index",
        body: "The parser reads Markdown and code files before indexing normalized text.",
    },
    CorpusDocument {
        id: "doc-parser-changelog",
        file_name: "parser-changelog.md",
        title: "Parser Changelog",
        body: "The parser changed its test plan while documenting an unrelated release.",
    },
];

struct QueryCase {
    name: &'static str,
    query: &'static str,
    relevant: &'static [&'static str],
}

const QUERY_CASES: &[QueryCase] = &[
    QueryCase {
        name: "计划导航",
        query: "plan",
        relevant: &[
            "doc-project-plan",
            "doc-meeting-notes",
            "doc-release-plan",
            "doc-travel-plan",
        ],
    },
    QueryCase {
        name: "项目计划语义词",
        query: "project plan",
        relevant: &["doc-project-plan", "doc-meeting-notes", "doc-release-plan"],
    },
    QueryCase {
        name: "季度计划短语",
        query: "\"quarterly plan\"",
        relevant: &["doc-project-plan"],
    },
    QueryCase {
        name: "备份恢复",
        query: "backup restore",
        relevant: &["doc-backup-runbook"],
    },
    QueryCase {
        name: "本地数据",
        query: "local data",
        relevant: &["doc-privacy-notes"],
    },
    QueryCase {
        name: "模式索引",
        query: "schema FTS",
        relevant: &["doc-database-migration"],
    },
    QueryCase {
        name: "排序召回",
        query: "ranking recall",
        relevant: &["doc-search-research"],
    },
    QueryCase {
        name: "会议短语",
        query: "\"meeting notes\"",
        relevant: &["doc-meeting-notes"],
    },
    QueryCase {
        name: "解析 Markdown",
        query: "parser markdown",
        relevant: &["doc-code-index"],
    },
];

struct TemporaryFixture {
    root: PathBuf,
    database_path: PathBuf,
}

impl TemporaryFixture {
    fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("读取 semantic 评估时间失败")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "nexus-semantic-quality-{}-{timestamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("创建 semantic 评估目录失败");
        let database_path = root.join("nexus.sqlite3");
        Self {
            root,
            database_path,
        }
    }
}

impl Drop for TemporaryFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[derive(Debug)]
struct Measurement {
    lexical_ids: Vec<String>,
    hybrid_ids: Vec<String>,
    lexical_recall_at_3: f64,
    hybrid_recall_at_3: f64,
    lexical_top_1_hit: bool,
    hybrid_top_1_hit: bool,
    lexical_median_micros: u128,
    hybrid_median_micros: u128,
}

#[test]
fn evaluates_hybrid_search_against_the_m3_fixed_corpus() {
    let fixture = TemporaryFixture::new();
    let connection =
        initialize_database(&fixture.database_path).expect("初始化 semantic 评估数据库失败");

    for document in CORPUS {
        upsert_document(
            &connection,
            &DocumentRecord {
                id: document.id.to_owned(),
                source_path: fixture.root.join(document.file_name),
                title: document.title.to_owned(),
                body: document.body.to_owned(),
                line_start: None,
                line_end: None,
            },
        )
        .expect("写入 semantic 固定评估语料失败");
    }
    drop(connection);

    let control = RescanControl::new();
    let provider = LocalFeatureEmbedding::new();
    let summary = index_document_embeddings(
        &fixture.database_path,
        &provider,
        EmbeddingIndexOptions { batch_size: 3 },
        &control,
    )
    .expect("建立 semantic 评估向量失败");
    assert_eq!(summary.documents_seen, CORPUS.len());
    assert_eq!(summary.embeddings_written, CORPUS.len());
    assert_eq!(summary.documents_failed, 0);

    let connection =
        initialize_database(&fixture.database_path).expect("重新打开 semantic 评估数据库失败");
    let measure_query = |query_case: &QueryCase| {
        let query_text = extract_search_text(query_case.query)
            .expect("提取 semantic 评估查询文本失败")
            .expect("评估查询应包含正文文本");
        let query_vector = provider
            .embed(&query_text)
            .expect("生成 semantic 评估查询向量失败");
        let lexical = search_documents(&connection, query_case.query, MAX_SEARCH_LIMIT)
            .expect("执行 semantic 评估 lexical 查询失败");
        let hybrid = search_documents_hybrid(
            &connection,
            query_case.query,
            query_vector.as_slice(),
            provider.model_id(),
            provider.model_version(),
            MAX_SEARCH_LIMIT,
        )
        .expect("执行 semantic 评估 hybrid 查询失败");
        let lexical_ids = lexical
            .iter()
            .take(TOP_K)
            .map(|result| result.document_id.clone())
            .collect::<Vec<_>>();
        let hybrid_ids = hybrid
            .iter()
            .take(TOP_K)
            .map(|result| result.result.document_id.clone())
            .collect::<Vec<_>>();

        let mut lexical_durations = Vec::with_capacity(MEASUREMENT_ROUNDS);
        let mut hybrid_durations = Vec::with_capacity(MEASUREMENT_ROUNDS);
        for _ in 0..MEASUREMENT_ROUNDS {
            let started = Instant::now();
            search_documents(&connection, query_case.query, MAX_SEARCH_LIMIT)
                .expect("测量 semantic lexical 查询失败");
            lexical_durations.push(started.elapsed().as_micros());

            let started = Instant::now();
            search_documents_hybrid(
                &connection,
                query_case.query,
                query_vector.as_slice(),
                provider.model_id(),
                provider.model_version(),
                MAX_SEARCH_LIMIT,
            )
            .expect("测量 semantic hybrid 查询失败");
            hybrid_durations.push(started.elapsed().as_micros());
        }
        lexical_durations.sort_unstable();
        hybrid_durations.sort_unstable();

        Measurement {
            lexical_recall_at_3: recall_at_k(&lexical_ids, query_case.relevant),
            hybrid_recall_at_3: recall_at_k(&hybrid_ids, query_case.relevant),
            lexical_top_1_hit: is_relevant(lexical_ids.first(), query_case.relevant),
            hybrid_top_1_hit: is_relevant(hybrid_ids.first(), query_case.relevant),
            lexical_ids,
            hybrid_ids,
            lexical_median_micros: percentile(&lexical_durations, 50),
            hybrid_median_micros: percentile(&hybrid_durations, 50),
        }
    };

    let measurements = QUERY_CASES.iter().map(measure_query).collect::<Vec<_>>();
    assert_eq!(measurements.len(), QUERY_CASES.len());
    assert!(measurements.iter().all(|measurement| {
        measurement.hybrid_ids.len() <= TOP_K && measurement.lexical_ids.len() <= TOP_K
    }));
    assert!(measurements
        .iter()
        .all(|measurement| { measurement.hybrid_recall_at_3 >= measurement.lexical_recall_at_3 }));

    let lexical_recall: f64 = measurements
        .iter()
        .map(|measurement| measurement.lexical_recall_at_3)
        .sum::<f64>()
        / measurements.len() as f64;
    let hybrid_recall: f64 = measurements
        .iter()
        .map(|measurement| measurement.hybrid_recall_at_3)
        .sum::<f64>()
        / measurements.len() as f64;
    let lexical_top_1_hits = measurements
        .iter()
        .filter(|measurement| measurement.lexical_top_1_hit)
        .count();
    let hybrid_top_1_hits = measurements
        .iter()
        .filter(|measurement| measurement.hybrid_top_1_hit)
        .count();

    println!("M5 hybrid search quality evaluation");
    println!("corpus_documents={}", CORPUS.len());
    println!("queries={}", QUERY_CASES.len());
    println!("top_k={TOP_K}");
    println!("macro_recall_at_3_lexical={lexical_recall:.4}");
    println!("macro_recall_at_3_hybrid={hybrid_recall:.4}");
    println!("top_1_hits_lexical={lexical_top_1_hits}");
    println!("top_1_hits_hybrid={hybrid_top_1_hits}");
    for (query_case, measurement) in QUERY_CASES.iter().zip(&measurements) {
        println!(
            "case={} query={} lexical={:?} hybrid={:?} recall_lexical={:.4} recall_hybrid={:.4} median_lexical_us={} median_hybrid_us={}",
            query_case.name,
            query_case.query,
            measurement.lexical_ids,
            measurement.hybrid_ids,
            measurement.lexical_recall_at_3,
            measurement.hybrid_recall_at_3,
            measurement.lexical_median_micros,
            measurement.hybrid_median_micros,
        );
    }
}

fn recall_at_k(ids: &[String], relevant: &[&str]) -> f64 {
    let hits = relevant
        .iter()
        .filter(|id| ids.iter().any(|item| item == *id))
        .count();
    hits as f64 / relevant.len() as f64
}

fn is_relevant(id: Option<&String>, relevant: &[&str]) -> bool {
    id.is_some_and(|id| relevant.iter().any(|expected| expected == id))
}

fn percentile(values: &[u128], percentile: usize) -> u128 {
    if values.is_empty() {
        return 0;
    }

    let index = ((values.len() - 1) * percentile).div_ceil(100);
    values[index]
}
