import { FormEvent, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

const DEFAULT_SEARCH_LIMIT = 100;
const SEARCH_TIMEOUT_MS = 10_000;

type SearchPhase =
  "idle" | "loading" | "success" | "empty" | "error" | "cancelled";

type SearchResult = {
  documentId: string;
  sourcePath: string;
  title: string;
  fileName: string | null;
  extension: string | null;
  fileType: string | null;
  modifiedAt: number | null;
  createdAt: number | null;
  accessedAt: number | null;
  lineStart: number | null;
  lineEnd: number | null;
  relevance: number | null;
  snippet: string | null;
  semanticSimilarity: number | null;
  fusionScore: number | null;
  lexicalRank: number | null;
  semanticRank: number | null;
};

type SearchResponse = {
  results: SearchResult[];
};

type StartupPhase = "loading" | "ready" | "degraded";

type SearchViewProps = {
  startupPhase: StartupPhase;
  startupMessage: string;
};

const startupPresentation: Record<
  StartupPhase,
  { label: string; title: string; footer: string }
> = {
  loading: {
    label: "正在检查",
    title: "正在连接本地核心。",
    footer: "检查中",
  },
  ready: {
    label: "本地 / 就绪",
    title: "基础界面已启动。",
    footer: "连接正常",
  },
  degraded: {
    label: "降级 / 需处理",
    title: "本地核心暂不可用。",
    footer: "尚未连接",
  },
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isNullableSafeInteger(value: unknown): value is number | null {
  return value === null || (Number.isSafeInteger(value) && Number(value) >= 0);
}

function isNullableTimestamp(value: unknown): value is number | null {
  return value === null || Number.isSafeInteger(value);
}

function isNullableFiniteNumber(value: unknown): value is number | null {
  return (
    value === null || (typeof value === "number" && Number.isFinite(value))
  );
}

function isNullableSafeCount(value: unknown): value is number | null {
  return value === null || (Number.isSafeInteger(value) && Number(value) >= 0);
}

function isSearchResult(value: unknown): value is SearchResult {
  if (!isRecord(value)) {
    return false;
  }

  return (
    typeof value.documentId === "string" &&
    value.documentId.length > 0 &&
    typeof value.sourcePath === "string" &&
    value.sourcePath.length > 0 &&
    typeof value.title === "string" &&
    value.title.length > 0 &&
    isNullableString(value.fileName) &&
    isNullableString(value.extension) &&
    isNullableString(value.fileType) &&
    isNullableTimestamp(value.modifiedAt) &&
    isNullableTimestamp(value.createdAt) &&
    isNullableTimestamp(value.accessedAt) &&
    isNullableSafeInteger(value.lineStart) &&
    isNullableSafeInteger(value.lineEnd) &&
    isNullableFiniteNumber(value.relevance) &&
    isNullableString(value.snippet) &&
    isNullableFiniteNumber(value.semanticSimilarity) &&
    isNullableFiniteNumber(value.fusionScore) &&
    isNullableSafeCount(value.lexicalRank) &&
    isNullableSafeCount(value.semanticRank)
  );
}

function isSearchResponse(value: unknown): value is SearchResponse {
  return (
    isRecord(value) &&
    Array.isArray(value.results) &&
    value.results.every((result) => isSearchResult(result))
  );
}

class SearchTimeoutError extends Error {
  constructor() {
    super("search_timeout");
    this.name = "SearchTimeoutError";
  }
}

function invokeSearch(query: string, limit: number): Promise<unknown> {
  let timeoutId: number | undefined;

  return Promise.race([
    invoke<unknown>("search_documents", {
      request: { query, limit },
    }),
    new Promise<never>((_, reject) => {
      timeoutId = window.setTimeout(() => {
        reject(new SearchTimeoutError());
      }, SEARCH_TIMEOUT_MS);
    }),
  ]).finally(() => {
    if (timeoutId !== undefined) {
      window.clearTimeout(timeoutId);
    }
  });
}

function safeSearchMessage(value: unknown, fallback: string): string {
  if (value instanceof SearchTimeoutError) {
    return "本地搜索响应超时，请稍后重试；如果索引正在更新，请等待完成。";
  }

  if (value instanceof Error) {
    return fallback;
  }

  if (
    isRecord(value) &&
    typeof value.message === "string" &&
    value.message.length > 0 &&
    value.message.length <= 120 &&
    !/[\r\n]/u.test(value.message)
  ) {
    return value.message;
  }

  return fallback;
}

function formatCount(value: number): string {
  return value.toLocaleString("zh-CN");
}

function formatResultType(result: SearchResult): string {
  if (result.fileType !== null && result.fileType.length > 0) {
    return result.fileType;
  }

  if (result.extension !== null && result.extension.length > 0) {
    return `${result.extension.toUpperCase()} 文件`;
  }

  return "本地文档";
}

function formatResultDate(timestamp: number | null): string {
  if (timestamp === null) {
    return "时间未知";
  }

  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) {
    return "时间未知";
  }

  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).format(date);
}

function formatRelevance(relevance: number | null): string | null {
  return relevance === null ? null : relevance.toFixed(2);
}

function formatScore(score: number | null): string | null {
  return score === null ? null : score.toFixed(4);
}

function renderSnippet(snippet: string) {
  return snippet.split(/(⟦[^⟧]*⟧)/u).map((segment, index) => {
    const isHit = segment.startsWith("⟦") && segment.endsWith("⟧");
    const content = isHit ? segment.slice(1, -1) : segment;

    return isHit ? (
      <mark className="search-hit" key={`${segment}-${index}`}>
        {content}
      </mark>
    ) : (
      <span key={`${segment}-${index}`}>{content}</span>
    );
  });
}

function SearchView({ startupPhase, startupMessage }: SearchViewProps) {
  const [query, setQuery] = useState("");
  const [submittedQuery, setSubmittedQuery] = useState("");
  const [phase, setPhase] = useState<SearchPhase>("idle");
  const [message, setMessage] = useState("输入关键词或筛选条件后开始搜索。");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [openError, setOpenError] = useState<string | null>(null);
  const [openingDocumentId, setOpeningDocumentId] = useState<string | null>(
    null,
  );
  const requestIdRef = useRef(0);
  const activeRef = useRef(true);

  useEffect(() => {
    activeRef.current = true;

    return () => {
      activeRef.current = false;
      requestIdRef.current += 1;
    };
  }, []);

  const handleSearch = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (phase === "loading") {
      return;
    }

    const normalizedQuery = query.trim();
    if (normalizedQuery.length === 0) {
      requestIdRef.current += 1;
      setSubmittedQuery("");
      setResults([]);
      setOpenError(null);
      setPhase("idle");
      setMessage("输入关键词或筛选条件后开始搜索。");
      return;
    }

    if (startupPhase !== "ready") {
      requestIdRef.current += 1;
      setSubmittedQuery(normalizedQuery);
      setResults([]);
      setOpenError(null);
      setPhase("error");
      setMessage("本地核心尚未就绪，请等待连接完成后重试。");
      return;
    }

    const requestId = requestIdRef.current + 1;
    requestIdRef.current = requestId;
    setSubmittedQuery(normalizedQuery);
    setResults([]);
    setOpenError(null);
    setPhase("loading");
    setMessage("正在本地索引中查找。");

    try {
      const response = await invokeSearch(
        normalizedQuery,
        DEFAULT_SEARCH_LIMIT,
      );

      if (
        !activeRef.current ||
        requestIdRef.current !== requestId ||
        !isSearchResponse(response)
      ) {
        if (activeRef.current && requestIdRef.current === requestId) {
          throw new Error("invalid_search_response");
        }
        return;
      }

      setResults(response.results);
      if (response.results.length === 0) {
        setPhase("empty");
        setMessage("没有找到匹配内容。");
      } else {
        setPhase("success");
        setMessage(`找到 ${formatCount(response.results.length)} 条匹配结果。`);
      }
    } catch (error) {
      if (!activeRef.current || requestIdRef.current !== requestId) {
        return;
      }

      setPhase("error");
      setMessage(safeSearchMessage(error, "本地搜索暂时不可用。"));
    }
  };

  const handleCancel = () => {
    if (phase !== "loading") {
      return;
    }

    requestIdRef.current += 1;
    setPhase("cancelled");
    setMessage("本次搜索已取消。");
  };

  const handleOpen = async (documentId: string) => {
    if (openingDocumentId !== null) {
      return;
    }

    setOpeningDocumentId(documentId);
    setOpenError(null);

    try {
      await invoke("open_document", {
        request: { documentId },
      });
    } catch (error) {
      if (activeRef.current) {
        setOpenError(safeSearchMessage(error, "无法打开原始文件。"));
      }
    } finally {
      if (activeRef.current) {
        setOpeningDocumentId(null);
      }
    }
  };

  const isLoading = phase === "loading";
  const resultLabel =
    phase === "success"
      ? `${formatCount(results.length)} 条结果`
      : phase === "loading"
        ? "查询中"
        : phase === "empty"
          ? "0 条结果"
          : "等待查询";

  return (
    <>
      <section className="search-hero" aria-labelledby="search-title">
        <div className="search-hero-copy">
          <p className="eyebrow">Nexus / M5 混合搜索</p>
          <h1 id="search-title">
            把想起的词，
            <span>找回来。</span>
          </h1>
          <p className="search-hero-lede">
            在本地索引的文件、笔记与代码里，寻找可追溯的原始内容。
          </p>
          <div className="search-flow" aria-label="搜索路径">
            <span>query</span>
            <span aria-hidden="true">→</span>
            <span>result</span>
            <span aria-hidden="true">→</span>
            <span>source</span>
          </div>
        </div>
        <div className="search-scope" aria-label="搜索范围">
          <img
            className="search-scope-icon"
            src="/nexus-product-icon.png"
            alt=""
            width="108"
            height="108"
            aria-hidden="true"
          />
          <span className="search-scope-index">03</span>
          <div>
            <span className="eyebrow">搜索范围</span>
            <strong>本地全文 + 向量</strong>
            <p>查询不会上传文件、文件名、向量或搜索历史。</p>
          </div>
          <div
            className={`search-scope-status search-scope-status-${startupPhase}`}
            aria-labelledby="search-status-title"
          >
            <span className="search-scope-status-label" aria-live="polite">
              <span className="status-dot" aria-hidden="true" />
              {startupPresentation[startupPhase].label}
            </span>
            <h2 id="search-status-title">
              {startupPresentation[startupPhase].title}
            </h2>
            <p>{startupMessage}</p>
            <span className="search-scope-status-footer">
              {startupPresentation[startupPhase].footer}
            </span>
          </div>
        </div>
      </section>

      <section className="search-workspace" aria-labelledby="search-form-title">
        <div className="search-section-heading">
          <p className="eyebrow">本地索引 / 查询</p>
          <h2 id="search-form-title">从一个词开始。</h2>
          <p>关键词和筛选条件按 Enter 后执行；结果始终保留原文件引用。</p>
        </div>

        <div className="search-panel">
          <form
            className="search-form"
            onSubmit={(event) => void handleSearch(event)}
          >
            <label className="search-label" htmlFor="document-search-query">
              搜索本地文档
              <span>Enter 执行</span>
            </label>
            <div className="search-input-entry">
              <span className="search-input-prefix" aria-hidden="true">
                QUERY
              </span>
              <input
                id="document-search-query"
                className="search-input"
                type="search"
                value={query}
                onChange={(event) => {
                  setQuery(event.target.value);
                  setOpenError(null);
                }}
                placeholder='例如：项目计划 或 filename:"会议记录"'
                autoComplete="off"
                spellCheck={false}
                aria-describedby="search-query-help"
                disabled={isLoading}
              />
              <button
                className="button button-primary search-submit"
                type="submit"
                disabled={isLoading || startupPhase !== "ready"}
              >
                {isLoading ? "搜索中…" : "搜索"}
              </button>
            </div>
            <p id="search-query-help" className="search-help">
              支持关键词、双引号短语、filename、path、ext、type 和日期筛选。
            </p>
            <div className="search-actions">
              <span className="search-query-example">
                例如：<code>{'"季度计划" ext:md modified>=2026-01-01'}</code>
              </span>
              <button
                className="button button-secondary"
                type="button"
                onClick={handleCancel}
                disabled={!isLoading}
              >
                取消查询
              </button>
            </div>
          </form>
        </div>
      </section>

      <section
        className={`search-results search-results-${phase}`}
        aria-labelledby="search-results-title"
        aria-busy={isLoading}
      >
        <div className="search-results-heading">
          <div>
            <p className="eyebrow">检索结果</p>
            <h2 id="search-results-title">
              {submittedQuery.length > 0
                ? `“${submittedQuery}”`
                : "等待一次查询"}
            </h2>
          </div>
          <span className="search-result-count" aria-live="polite">
            {resultLabel}
          </span>
        </div>

        {openError !== null ? (
          <p className="search-inline-error" role="alert">
            {openError}
          </p>
        ) : null}

        {phase === "success" ? (
          <ol className="search-result-list" aria-label="搜索结果列表">
            {results.map((result, index) => {
              const relevance = formatRelevance(result.relevance);
              const resultName = result.fileName ?? result.title;

              return (
                <li className="search-result-item" key={result.documentId}>
                  <button
                    className="search-result-card"
                    type="button"
                    onClick={() => void handleOpen(result.documentId)}
                    disabled={openingDocumentId !== null}
                    aria-label={`打开 ${resultName}`}
                  >
                    <span className="search-result-index" aria-hidden="true">
                      {String(index + 1).padStart(2, "0")}
                    </span>
                    <span className="search-result-main">
                      <span className="search-result-title-row">
                        <strong>{result.title}</strong>
                        <span className="search-result-open-label">
                          {openingDocumentId === result.documentId
                            ? "打开中…"
                            : "定位原文件 →"}
                        </span>
                      </span>
                      <span className="search-result-meta">
                        <span>{resultName}</span>
                        <span>{formatResultType(result)}</span>
                        <span>{formatResultDate(result.modifiedAt)}</span>
                        {relevance !== null ? (
                          <span>词法 {relevance}</span>
                        ) : null}
                        {result.fusionScore !== null ? (
                          <span>混合 {formatScore(result.fusionScore)}</span>
                        ) : null}
                      </span>
                      <span className="search-result-path">
                        {result.sourcePath}
                      </span>
                      {result.snippet !== null ? (
                        <span className="search-result-snippet">
                          {renderSnippet(result.snippet)}
                        </span>
                      ) : null}
                    </span>
                  </button>
                </li>
              );
            })}
          </ol>
        ) : (
          <div
            className="search-state"
            role={phase === "error" ? "alert" : undefined}
          >
            <span className="search-state-index" aria-hidden="true">
              {phase === "error" ? "!" : phase === "empty" ? "00" : "—"}
            </span>
            <div>
              <strong>{message}</strong>
              <p>
                {phase === "error"
                  ? "请检查查询格式或确认本地核心仍然可用。"
                  : phase === "empty"
                    ? "可以尝试更短的关键词，或移除一个筛选条件。"
                    : phase === "cancelled"
                      ? "你可以修改查询条件后再次执行。"
                      : phase === "loading"
                        ? "正在读取本地索引，请稍候。"
                        : "查询结果会显示在这里，并保留原始文件路径。"}
              </p>
            </div>
          </div>
        )}
      </section>

      <section className="search-notes" aria-label="搜索说明">
        <div>
          <span className="eyebrow">搜索原则</span>
          <strong>先给你确定的结果。</strong>
        </div>
        <p>
          当前搜索保留本地 lexical
          index，并在可用时加入本地向量基线；不依赖人工智能或网络。
        </p>
        <span className="search-notes-rule" aria-hidden="true" />
      </section>
    </>
  );
}

export default SearchView;
