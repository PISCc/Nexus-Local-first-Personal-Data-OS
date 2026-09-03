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
  { label: string; title: string; message: string; footer: string }
> = {
  loading: {
    label: "准备中",
    title: "正在准备你的资料。",
    message: "请稍候，准备完成后就可以开始搜索。",
    footer: "正在准备",
  },
  ready: {
    label: "已准备好",
    title: "你的资料可以搜索了。",
    message: "输入关键词，找到后可以直接打开原文件。",
    footer: "随时可搜索",
  },
  degraded: {
    label: "暂时不可用",
    title: "暂时无法读取你的资料。",
    message: "请重新打开应用后再试。",
    footer: "需要重试",
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
    return "搜索花费的时间比预期长，请稍后重试。";
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
  const extension = result.extension?.replace(/^\./u, "").toLowerCase();
  const extensionLabels: Record<string, string> = {
    txt: "文本",
    md: "Markdown",
    py: "Python",
    rs: "Rust",
    js: "JavaScript",
    ts: "TypeScript",
    java: "Java",
    cpp: "C++",
    json: "JSON",
    html: "HTML",
    htm: "HTML",
    docx: "Word 文档",
    pdf: "PDF",
  };

  if (extension !== undefined && extension in extensionLabels) {
    return extensionLabels[extension];
  }

  if (result.fileType !== null && result.fileType.startsWith("text/")) {
    return "文本";
  }

  return "文档";
}

function formatResultDate(timestamp: number | null): string {
  if (timestamp === null) {
    return "修改时间未知";
  }

  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) {
    return "修改时间未知";
  }

  const formatted = new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).format(date);

  return `修改于 ${formatted}`;
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

function SearchView({ startupPhase }: SearchViewProps) {
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
      setMessage(
        startupPhase === "loading"
          ? "资料还在准备中，请稍候再试。"
          : "暂时无法读取你的资料，请重新打开应用后再试。",
      );
      return;
    }

    const requestId = requestIdRef.current + 1;
    requestIdRef.current = requestId;
    setSubmittedQuery(normalizedQuery);
    setResults([]);
    setOpenError(null);
    setPhase("loading");
    setMessage("正在为你查找。");

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
      setMessage(safeSearchMessage(error, "暂时无法搜索，请稍后再试。"));
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
        setOpenError(
          safeSearchMessage(error, "无法打开这个文件，请确认文件仍然存在。"),
        );
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
        ? "搜索中"
        : phase === "empty"
          ? "0 条结果"
          : "还没有搜索";

  return (
    <>
      <section className="search-hero" aria-labelledby="search-title">
        <div className="search-hero-copy">
          <p className="eyebrow">找回你的资料</p>
          <h1 id="search-title">
            把想起的词，
            <span>找回来。</span>
          </h1>
          <p className="search-hero-lede">
            在文件、笔记和代码里，快速找到你要的内容。
          </p>
          <div className="search-flow" aria-label="搜索路径">
            <span>输入</span>
            <span aria-hidden="true">→</span>
            <span>找到</span>
            <span aria-hidden="true">→</span>
            <span>原文件</span>
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
            <strong>这台设备上的资料</strong>
            <p>文件和搜索记录不会上传，每条结果都能回到原文件。</p>
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
            <p>{startupPresentation[startupPhase].message}</p>
            <span className="search-scope-status-footer">
              {startupPresentation[startupPhase].footer}
            </span>
          </div>
        </div>
      </section>

      <section className="search-workspace" aria-labelledby="search-form-title">
        <div className="search-section-heading">
          <p className="eyebrow">开始搜索</p>
          <h2 id="search-form-title">输入你记得的词。</h2>
          <p>按 Enter 搜索；结果会保留原文件位置，方便你直接打开。</p>
        </div>

        <div className="search-panel">
          <form
            className="search-form"
            onSubmit={(event) => void handleSearch(event)}
          >
            <label className="search-label" htmlFor="document-search-query">
              搜索你的资料
              <span>按 Enter 搜索</span>
            </label>
            <div className="search-input-entry">
              <span className="search-input-prefix" aria-hidden="true">
                搜索
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
                placeholder="例如：项目计划、会议记录或预算"
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
              可以输入关键词或短语，也可以按文件名、类型和日期缩小范围。
            </p>
            <div className="search-actions">
              <span className="search-query-example">
                试试：季度计划、会议记录或项目预算
              </span>
              <button
                className="button button-secondary"
                type="button"
                onClick={handleCancel}
                disabled={!isLoading}
              >
                停止搜索
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
            <p className="eyebrow">找到的内容</p>
            <h2 id="search-results-title">
              {submittedQuery.length > 0
                ? `“${submittedQuery}”`
                : "搜索结果会显示在这里"}
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
                            : "打开原文件 →"}
                        </span>
                      </span>
                      <span className="search-result-meta">
                        <span>{resultName}</span>
                        <span>{formatResultType(result)}</span>
                        <span>{formatResultDate(result.modifiedAt)}</span>
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
                  ? "请检查关键词或筛选条件后重试。"
                  : phase === "empty"
                    ? "换一个关键词或减少筛选条件再试试。"
                    : phase === "cancelled"
                      ? "可以修改关键词后再次搜索。"
                      : phase === "loading"
                        ? "正在查找，请稍候。"
                        : "找到后可以直接打开原文件。"}
              </p>
            </div>
          </div>
        )}
      </section>

      <section className="search-notes" aria-label="搜索说明">
        <div>
          <span className="eyebrow">使用说明</span>
          <strong>内容留在你的设备上。</strong>
        </div>
        <p>
          Nexus
          只在本机整理和搜索文件，不上传文件内容、文件名或搜索记录；每条结果都保留原文件位置。
        </p>
        <span className="search-notes-rule" aria-hidden="true" />
      </section>
    </>
  );
}

export default SearchView;
