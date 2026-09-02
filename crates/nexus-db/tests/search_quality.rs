use std::{
    fs,
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use nexus_db::{
    initialize_database, search_documents, upsert_document, DocumentRecord, SearchResult,
    MAX_SEARCH_LIMIT,
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
            .expect("读取测试时间失败")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "nexus-search-quality-{}-{timestamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("创建搜索质量测试目录失败");
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
    current_ids: Vec<String>,
    baseline_ids: Vec<String>,
    current_recall_at_3: f64,
    baseline_recall_at_3: f64,
    current_top_1_hit: bool,
    baseline_top_1_hit: bool,
    median_micros: u128,
    p95_micros: u128,
}

#[test]
fn evaluates_fixed_corpus_and_prints_search_quality_report() {
    let fixture = TemporaryFixture::new();
    let connection = initialize_database(&fixture.database_path).expect("初始化评估数据库失败");

    for document in CORPUS {
        let record = DocumentRecord {
            id: document.id.to_owned(),
            source_path: fixture.root.join(document.file_name),
            title: document.title.to_owned(),
            body: document.body.to_owned(),
            line_start: None,
            line_end: None,
        };
        upsert_document(&connection, &record).expect("写入固定评估语料失败");
    }

    let measure_query = |query_case: &QueryCase| {
        let all_results = search_documents(&connection, query_case.query, MAX_SEARCH_LIMIT)
            .expect("执行固定查询失败");
        let current_ids = top_ids(&all_results, TOP_K);
        let baseline_ids = baseline_ids(&all_results, TOP_K);
        let current_recall_at_3 = recall_at_k(&current_ids, query_case.relevant);
        let baseline_recall_at_3 = recall_at_k(&baseline_ids, query_case.relevant);
        let current_top_1_hit = is_relevant(current_ids.first(), query_case.relevant);
        let baseline_top_1_hit = is_relevant(baseline_ids.first(), query_case.relevant);
        let mut durations = Vec::with_capacity(MEASUREMENT_ROUNDS);

        for _ in 0..MEASUREMENT_ROUNDS {
            let started = Instant::now();
            search_documents(&connection, query_case.query, MAX_SEARCH_LIMIT)
                .expect("测量固定查询失败");
            durations.push(started.elapsed().as_micros());
        }

        durations.sort_unstable();
        Measurement {
            current_ids,
            baseline_ids,
            current_recall_at_3,
            baseline_recall_at_3,
            current_top_1_hit,
            baseline_top_1_hit,
            median_micros: percentile(&durations, 50),
            p95_micros: percentile(&durations, 95),
        }
    };

    let measurements: Vec<_> = QUERY_CASES.iter().map(measure_query).collect();

    assert_eq!(measurements.len(), QUERY_CASES.len());
    assert!(measurements.iter().all(|measurement| {
        measurement.current_recall_at_3 >= measurement.baseline_recall_at_3
    }));
    assert!(measurements.iter().all(|measurement| {
        measurement.current_ids.len() <= TOP_K && measurement.baseline_ids.len() <= TOP_K
    }));

    let total_current_recall: f64 = measurements
        .iter()
        .map(|measurement| measurement.current_recall_at_3)
        .sum();
    let total_baseline_recall: f64 = measurements
        .iter()
        .map(|measurement| measurement.baseline_recall_at_3)
        .sum();
    let current_top_1_hits = measurements
        .iter()
        .filter(|measurement| measurement.current_top_1_hit)
        .count();
    let baseline_top_1_hits = measurements
        .iter()
        .filter(|measurement| measurement.baseline_top_1_hit)
        .count();
    let fts_segment_bytes: i64 = connection
        .query_row(
            "SELECT COALESCE(SUM(length(block)), 0) FROM documents_fts_data",
            [],
            |row| row.get(0),
        )
        .expect("读取 FTS5 segment 大小失败");
    assert!(fts_segment_bytes > 0);
    drop(connection);
    let database_bytes = fs::metadata(&fixture.database_path)
        .expect("读取评估数据库大小失败")
        .len();

    println!("M3.5 search quality evaluation");
    println!("corpus_documents={}", CORPUS.len());
    println!("queries={}", QUERY_CASES.len());
    println!("database_bytes={database_bytes}");
    println!("fts_segment_bytes={fts_segment_bytes}");
    println!("top_k={TOP_K}");
    println!(
        "macro_recall_at_3_current={:.4}",
        total_current_recall / measurements.len() as f64
    );
    println!(
        "macro_recall_at_3_baseline={:.4}",
        total_baseline_recall / measurements.len() as f64
    );
    println!("top_1_hits_current={current_top_1_hits}");
    println!("top_1_hits_baseline={baseline_top_1_hits}");
    for (query_case, measurement) in QUERY_CASES.iter().zip(&measurements) {
        println!(
            "case={} query={} current={:?} baseline={:?} recall_current={:.4} recall_baseline={:.4} median_us={} p95_us={}",
            query_case.name,
            query_case.query,
            measurement.current_ids,
            measurement.baseline_ids,
            measurement.current_recall_at_3,
            measurement.baseline_recall_at_3,
            measurement.median_micros,
            measurement.p95_micros
        );
    }
}

fn top_ids(results: &[SearchResult], limit: usize) -> Vec<String> {
    results
        .iter()
        .take(limit)
        .map(|result| result.document_id.clone())
        .collect()
}

fn baseline_ids(results: &[SearchResult], limit: usize) -> Vec<String> {
    let mut ids: Vec<_> = results
        .iter()
        .map(|result| result.document_id.clone())
        .collect();
    ids.sort_unstable();
    ids.truncate(limit);
    ids
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
