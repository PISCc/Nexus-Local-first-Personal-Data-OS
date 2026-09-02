import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import SearchView from "./SearchView";

const principles = [
  {
    index: "01",
    title: "默认本地运行",
    description: "除非你主动选择，源数据始终保留在这台设备上。",
  },
  {
    index: "02",
    title: "先搜索，再用人工智能",
    description: "确定性的检索能力是基础，而不是事后补上的功能。",
  },
  {
    index: "03",
    title: "来源始终可追溯",
    description: "未来的每个回答，都应能回到它所依据的原始文档。",
  },
];

type StartupPhase = "loading" | "ready" | "degraded";

type StartupStatus = {
  phase: StartupPhase;
  message: string;
};

type BackendStartupStatus = {
  phase: Exclude<StartupPhase, "loading">;
  message: string;
};

type RescanPhase =
  | "idle"
  | "starting"
  | "running"
  | "cancelling"
  | "completed"
  | "cancelled"
  | "failed";

type RescanProgress = {
  processed: number;
  filesSucceeded: number;
  filesFailed: number;
  pathsSkipped: number;
  documentsSucceeded: number;
  documentsFailed: number;
  documentsSkipped: number;
};

type RescanSummary = {
  filesSucceeded: number;
  filesFailed: number;
  pathsSkipped: number;
  documentsSucceeded: number;
  documentsFailed: number;
  documentsSkipped: number;
  recordsRemoved: number;
  batchesCommitted: number;
};

type RescanStatusResponse = {
  state: "idle" | "running";
  scanId: number | null;
  progress: RescanProgress | null;
};

type StartRescanResponse = {
  scanId: number;
};

type CancelRescanResponse = {
  scanId: number;
  accepted: boolean;
};

type RescanProgressEvent = RescanProgress & {
  scanId: number;
};

type RescanFinishedEvent = {
  scanId: number;
  status: "completed" | "cancelled" | "failed";
  message: string;
  summary: RescanSummary | null;
  errorKind: string | null;
};

type WatchPhase = "idle" | "starting" | "watching" | "stopped" | "failed";

type WatchStatusResponse = {
  state: "idle" | "running";
  watchId: number | null;
};

type WatchStatusEvent = {
  watchId: number;
  state: "starting" | "watching" | "stopped" | "failed";
  message: string;
  errorKind: string | null;
};

type IncrementalFinishedEvent = {
  watchId: number;
  changesReceived: number;
  filesUpdated: number;
  filesRemoved: number;
  filesFailed: number;
  documentsUpdated: number;
  documentsRemoved: number;
  retries: number;
  fullRescan: boolean;
};

type ActiveView = "search" | "index";

const initialStartupStatus: StartupStatus = {
  phase: "loading",
  message: "正在连接本地核心，请稍候。",
};

const emptyProgress: RescanProgress = {
  processed: 0,
  filesSucceeded: 0,
  filesFailed: 0,
  pathsSkipped: 0,
  documentsSucceeded: 0,
  documentsFailed: 0,
  documentsSkipped: 0,
};

const statusPresentation: Record<
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

const rescanPhasePresentation: Record<
  RescanPhase,
  { label: string; title: string }
> = {
  idle: { label: "等待开始", title: "输入目录后开始一次本地初始索引。" },
  starting: { label: "准备中", title: "正在准备本地索引任务。" },
  running: { label: "扫描中", title: "正在扫描文件并建立正文索引。" },
  cancelling: { label: "取消中", title: "正在停止本地索引。" },
  completed: { label: "已完成", title: "本次本地索引已完成。" },
  cancelled: { label: "已取消", title: "本次本地索引已取消。" },
  failed: { label: "未完成", title: "本次本地索引未完成。" },
};

const watchPhasePresentation: Record<
  WatchPhase,
  { label: string; message: string }
> = {
  idle: { label: "未开启", message: "完成一次索引后会自动同步文件变化。" },
  starting: { label: "准备中", message: "正在准备文件自动同步。" },
  watching: { label: "已开启", message: "文件变化会在本地自动同步。" },
  stopped: { label: "已停止", message: "文件自动同步已停止。" },
  failed: { label: "需重试", message: "文件自动同步暂时不可用。" },
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isBackendStartupStatus(value: unknown): value is BackendStartupStatus {
  if (!isRecord(value)) {
    return false;
  }

  return (
    (value.phase === "ready" || value.phase === "degraded") &&
    typeof value.message === "string"
  );
}

function isSafeCount(value: unknown): value is number {
  return Number.isSafeInteger(value) && Number(value) >= 0;
}

function isRescanProgress(value: unknown): value is RescanProgress {
  return (
    isRecord(value) &&
    isSafeCount(value.processed) &&
    isSafeCount(value.filesSucceeded) &&
    isSafeCount(value.filesFailed) &&
    isSafeCount(value.pathsSkipped) &&
    isSafeCount(value.documentsSucceeded) &&
    isSafeCount(value.documentsFailed) &&
    isSafeCount(value.documentsSkipped)
  );
}

function isScanId(value: unknown): value is number {
  return Number.isSafeInteger(value) && Number(value) > 0;
}

function isRescanStatusResponse(value: unknown): value is RescanStatusResponse {
  if (
    !isRecord(value) ||
    (value.state !== "idle" && value.state !== "running")
  ) {
    return false;
  }

  const hasValidScanId = value.scanId === null || isScanId(value.scanId);
  const hasValidProgress =
    value.progress === null || isRescanProgress(value.progress);

  return hasValidScanId && hasValidProgress;
}

function isStartRescanResponse(value: unknown): value is StartRescanResponse {
  return isRecord(value) && isScanId(value.scanId);
}

function isCancelRescanResponse(value: unknown): value is CancelRescanResponse {
  return (
    isRecord(value) &&
    isScanId(value.scanId) &&
    typeof value.accepted === "boolean"
  );
}

function isRescanProgressEvent(value: unknown): value is RescanProgressEvent {
  return isRecord(value) && isScanId(value.scanId) && isRescanProgress(value);
}

function isRescanSummary(value: unknown): value is RescanSummary {
  if (!isRecord(value)) {
    return false;
  }

  const candidate = value as Record<string, unknown>;
  return (
    isSafeCount(candidate.filesSucceeded) &&
    isSafeCount(candidate.filesFailed) &&
    isSafeCount(candidate.pathsSkipped) &&
    isSafeCount(candidate.documentsSucceeded) &&
    isSafeCount(candidate.documentsFailed) &&
    isSafeCount(candidate.documentsSkipped) &&
    isSafeCount(candidate.recordsRemoved) &&
    isSafeCount(candidate.batchesCommitted)
  );
}

function isRescanFinishedEvent(value: unknown): value is RescanFinishedEvent {
  return (
    isRecord(value) &&
    isScanId(value.scanId) &&
    (value.status === "completed" ||
      value.status === "cancelled" ||
      value.status === "failed") &&
    typeof value.message === "string" &&
    value.message.length > 0 &&
    value.message.length <= 120 &&
    !/[\r\n]/u.test(value.message) &&
    (value.summary === null || isRescanSummary(value.summary)) &&
    (value.errorKind === null || typeof value.errorKind === "string")
  );
}

function isWatchStatusResponse(value: unknown): value is WatchStatusResponse {
  if (
    !isRecord(value) ||
    (value.state !== "idle" && value.state !== "running")
  ) {
    return false;
  }

  return value.watchId === null || isScanId(value.watchId);
}

function isWatchStatusEvent(value: unknown): value is WatchStatusEvent {
  return (
    isRecord(value) &&
    isScanId(value.watchId) &&
    (value.state === "starting" ||
      value.state === "watching" ||
      value.state === "stopped" ||
      value.state === "failed") &&
    typeof value.message === "string" &&
    value.message.length > 0 &&
    value.message.length <= 120 &&
    !/[\r\n]/u.test(value.message) &&
    (value.errorKind === null || typeof value.errorKind === "string")
  );
}

function isIncrementalFinishedEvent(
  value: unknown,
): value is IncrementalFinishedEvent {
  if (!isRecord(value) || !isScanId(value.watchId)) {
    return false;
  }

  return (
    isSafeCount(value.changesReceived) &&
    isSafeCount(value.filesUpdated) &&
    isSafeCount(value.filesRemoved) &&
    isSafeCount(value.filesFailed) &&
    isSafeCount(value.documentsUpdated) &&
    isSafeCount(value.documentsRemoved) &&
    isSafeCount(value.retries) &&
    typeof value.fullRescan === "boolean"
  );
}

function safeCommandMessage(value: unknown, fallback: string): string {
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

async function loadStartupStatus(): Promise<StartupStatus> {
  try {
    const response = await invoke<unknown>("get_startup_status");

    if (isBackendStartupStatus(response)) {
      return response;
    }
  } catch {
    // 浏览器预览没有 Tauri 核心，统一呈现为可理解的降级状态。
  }

  return {
    phase: "degraded",
    message: "桌面核心暂不可用，当前处于降级模式。",
  };
}

function App() {
  const [activeView, setActiveView] = useState<ActiveView>("search");
  const [startupStatus, setStartupStatus] =
    useState<StartupStatus>(initialStartupStatus);
  const [rootPath, setRootPath] = useState("");
  const [rescanPhase, setRescanPhase] = useState<RescanPhase>("idle");
  const [rescanMessage, setRescanMessage] = useState(
    rescanPhasePresentation.idle.title,
  );
  const [scanId, setScanId] = useState<number | null>(null);
  const [progress, setProgress] = useState<RescanProgress>(emptyProgress);
  const [summary, setSummary] = useState<RescanSummary | null>(null);
  const [watchPhase, setWatchPhase] = useState<WatchPhase>("idle");
  const [watchMessage, setWatchMessage] = useState(
    watchPhasePresentation.idle.message,
  );
  const [incrementalSummary, setIncrementalSummary] =
    useState<IncrementalFinishedEvent | null>(null);
  const activeScanIdRef = useRef<number | null>(null);
  const rescanPhaseRef = useRef<RescanPhase>("idle");
  const activeWatchIdRef = useRef<number | null>(null);
  const latestWatchIdRef = useRef<number | null>(null);
  const watchPhaseRef = useRef<WatchPhase>("idle");

  useEffect(() => {
    let active = true;

    void loadStartupStatus().then((status) => {
      if (active) {
        setStartupStatus(status);
      }
    });

    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    rescanPhaseRef.current = rescanPhase;
  }, [rescanPhase]);

  useEffect(() => {
    watchPhaseRef.current = watchPhase;
  }, [watchPhase]);

  useEffect(() => {
    let active = true;
    const unlisteners: Array<() => void> = [];

    const handleProgress = (payload: unknown) => {
      if (!active || !isRescanProgressEvent(payload)) {
        return;
      }

      if (activeScanIdRef.current === null) {
        if (rescanPhaseRef.current !== "starting") {
          return;
        }
        activeScanIdRef.current = payload.scanId;
        setScanId(payload.scanId);
      }

      if (activeScanIdRef.current !== payload.scanId) {
        return;
      }

      setProgress(payload);
      if (rescanPhaseRef.current === "starting") {
        rescanPhaseRef.current = "running";
        setRescanPhase("running");
        setRescanMessage(rescanPhasePresentation.running.title);
      }
    };

    const handleFinished = (payload: unknown) => {
      if (!active || !isRescanFinishedEvent(payload)) {
        return;
      }

      if (activeScanIdRef.current === null) {
        if (rescanPhaseRef.current !== "starting") {
          return;
        }
        activeScanIdRef.current = payload.scanId;
        setScanId(payload.scanId);
      }

      if (activeScanIdRef.current !== payload.scanId) {
        return;
      }

      activeScanIdRef.current = null;
      rescanPhaseRef.current = payload.status;
      setRescanPhase(payload.status);
      setRescanMessage(payload.message);
      setSummary(payload.summary);
    };

    const handleWatchStatus = (payload: unknown) => {
      if (!active || !isWatchStatusEvent(payload)) {
        return;
      }

      if (
        latestWatchIdRef.current !== null &&
        payload.watchId < latestWatchIdRef.current
      ) {
        return;
      }

      if (
        activeWatchIdRef.current !== null &&
        payload.state !== "starting" &&
        payload.watchId !== activeWatchIdRef.current
      ) {
        return;
      }

      latestWatchIdRef.current = payload.watchId;
      if (payload.state === "starting" || payload.state === "watching") {
        activeWatchIdRef.current = payload.watchId;
      } else {
        activeWatchIdRef.current = null;
      }
      setWatchPhase(payload.state);
      setWatchMessage(payload.message);
    };

    const handleIncrementalFinished = (payload: unknown) => {
      if (!active || !isIncrementalFinishedEvent(payload)) {
        return;
      }

      if (
        latestWatchIdRef.current !== null &&
        payload.watchId < latestWatchIdRef.current
      ) {
        return;
      }

      if (
        activeWatchIdRef.current !== null &&
        payload.watchId !== activeWatchIdRef.current
      ) {
        return;
      }

      latestWatchIdRef.current = payload.watchId;
      setIncrementalSummary(payload);
    };

    const loadRescanStatus = async () => {
      try {
        const response = await invoke<unknown>("get_rescan_status");
        if (
          !active ||
          !isRescanStatusResponse(response) ||
          response.state !== "running" ||
          response.scanId === null
        ) {
          return;
        }

        activeScanIdRef.current = response.scanId;
        setScanId(response.scanId);
        setProgress(response.progress ?? emptyProgress);
        rescanPhaseRef.current = "running";
        setRescanPhase("running");
        setRescanMessage(rescanPhasePresentation.running.title);
      } catch {
        // 浏览器预览或旧版本核心可能没有重扫命令，保持空闲界面。
      }
    };

    const loadWatchStatus = async () => {
      try {
        const response = await invoke<unknown>("get_watch_status");
        if (!active || !isWatchStatusResponse(response)) {
          return;
        }

        if (response.state === "running" && response.watchId !== null) {
          activeWatchIdRef.current = response.watchId;
          latestWatchIdRef.current = response.watchId;
          setWatchPhase("watching");
          setWatchMessage(watchPhasePresentation.watching.message);
        } else if (
          watchPhaseRef.current !== "starting" &&
          watchPhaseRef.current !== "watching"
        ) {
          setWatchPhase("idle");
          setWatchMessage(watchPhasePresentation.idle.message);
        }
      } catch {
        // 浏览器预览或旧版本核心可能没有监听命令，保持空闲界面。
      }
    };

    const subscribe = async () => {
      try {
        const [
          unlistenProgress,
          unlistenFinished,
          unlistenWatchStatus,
          unlistenIncrementalFinished,
        ] = await Promise.all([
          listen<unknown>("rescan-progress", (event) => {
            handleProgress(event.payload);
          }),
          listen<unknown>("rescan-finished", (event) => {
            handleFinished(event.payload);
          }),
          listen<unknown>("watch-status", (event) => {
            handleWatchStatus(event.payload);
          }),
          listen<unknown>("incremental-finished", (event) => {
            handleIncrementalFinished(event.payload);
          }),
        ]);

        if (!active) {
          unlistenProgress();
          unlistenFinished();
          unlistenWatchStatus();
          unlistenIncrementalFinished();
          return;
        }

        unlisteners.push(
          unlistenProgress,
          unlistenFinished,
          unlistenWatchStatus,
          unlistenIncrementalFinished,
        );
      } catch {
        // 浏览器预览没有 Tauri 事件总线，不影响页面内容展示。
      }
    };

    void loadRescanStatus();
    void loadWatchStatus();
    void subscribe();

    return () => {
      active = false;
      unlisteners.forEach((unlisten) => {
        unlisten();
      });
    };
  }, []);

  const presentation = statusPresentation[startupStatus.phase];
  const rescanPresentation = rescanPhasePresentation[rescanPhase];
  const isRescanBusy =
    rescanPhase === "starting" ||
    rescanPhase === "running" ||
    rescanPhase === "cancelling";

  const handleStartRescan = async () => {
    const normalizedRootPath = rootPath.trim();
    if (normalizedRootPath.length === 0 || isRescanBusy) {
      if (normalizedRootPath.length === 0) {
        setRescanMessage("请先填写扫描目录。");
      }
      return;
    }

    setSummary(null);
    setIncrementalSummary(null);
    setProgress(emptyProgress);
    setScanId(null);
    activeScanIdRef.current = null;
    rescanPhaseRef.current = "starting";
    setRescanPhase("starting");
    setRescanMessage(rescanPhasePresentation.starting.title);
    activeWatchIdRef.current = null;
    setWatchPhase("starting");
    setWatchMessage(watchPhasePresentation.starting.message);

    try {
      const response = await invoke<unknown>("start_rescan", {
        request: {
          rootPath: normalizedRootPath,
          ignoredPaths: [],
          followSymlinks: false,
          indexContent: true,
        },
      });

      if (!isStartRescanResponse(response)) {
        throw new Error("invalid_rescan_start_response");
      }

      if (rescanPhaseRef.current === "starting") {
        activeScanIdRef.current = response.scanId;
        setScanId(response.scanId);
        rescanPhaseRef.current = "running";
        setRescanPhase("running");
        setRescanMessage(rescanPhasePresentation.running.title);
      }
    } catch (error) {
      activeScanIdRef.current = null;
      setScanId(null);
      rescanPhaseRef.current = "failed";
      setRescanPhase("failed");
      setRescanMessage(safeCommandMessage(error, "无法启动手动重扫。"));
      setWatchPhase("failed");
      setWatchMessage(watchPhasePresentation.failed.message);
    }
  };

  const handleCancelRescan = async () => {
    const currentScanId = activeScanIdRef.current;
    if (currentScanId === null || !isRescanBusy) {
      return;
    }

    rescanPhaseRef.current = "cancelling";
    setRescanPhase("cancelling");
    setRescanMessage(rescanPhasePresentation.cancelling.title);

    try {
      const response = await invoke<unknown>("cancel_rescan", {
        request: { scanId: currentScanId },
      });

      if (!isCancelRescanResponse(response) || !response.accepted) {
        throw new Error("rescan_cancel_rejected");
      }
    } catch (error) {
      if (activeScanIdRef.current === currentScanId) {
        rescanPhaseRef.current = "running";
        setRescanPhase("running");
        setRescanMessage(safeCommandMessage(error, "无法取消手动重扫。"));
      }
    }
  };

  return (
    <div className="app-shell">
      <aside className="side-rail" aria-label="Nexus 导航">
        <div>
          <div className="brand-lockup">
            <span className="brand-index">N / 01</span>
            <span className="brand-name">Nexus</span>
          </div>
          <p className="rail-caption">个人数据操作系统</p>
        </div>

        <nav className="rail-nav" aria-label="里程碑">
          <div className="rail-item quiet">
            <span className="rail-item-index">00</span>
            <span className="rail-item-label">工程基础</span>
            <span className="rail-item-state">完成</span>
          </div>
          <button
            className={`rail-item ${activeView === "index" ? "active" : "quiet"}`}
            type="button"
            onClick={() => {
              setActiveView("index");
            }}
            aria-current={activeView === "index" ? "page" : undefined}
          >
            <span className="rail-item-index">01</span>
            <span className="rail-item-label">文件索引</span>
            <span className="rail-item-state">
              {activeView === "index" ? "当前" : "返回"}
            </span>
          </button>
          <button
            className={`rail-item ${activeView === "search" ? "active" : "quiet"}`}
            type="button"
            onClick={() => {
              setActiveView("search");
            }}
            aria-current={activeView === "search" ? "page" : undefined}
          >
            <span className="rail-item-index">03</span>
            <span className="rail-item-label">全文搜索</span>
            <span className="rail-item-state">
              {activeView === "search" ? "当前" : "打开"}
            </span>
          </button>
        </nav>

        <div className="rail-footer">
          <span className="rail-footer-label">存储模式</span>
          <div className="local-badge">
            <span className="status-dot" aria-hidden="true" />
            仅本地
          </div>
          <p>让重要数据始终留在身边。</p>
        </div>
      </aside>

      <main className="main-surface">
        <header className="topbar">
          <div className="breadcrumb">
            <span>工作区</span>
            <span className="breadcrumb-separator" aria-hidden="true">
              /
            </span>
            <strong>{activeView === "search" ? "全文搜索" : "文件索引"}</strong>
          </div>
          <div className="topbar-meta">
            <span className="version-pill">v0.1.0</span>
            <span className="topbar-note">离线优先</span>
          </div>
        </header>

        <div className="content-wrap">
          {activeView === "search" ? (
            <SearchView
              startupPhase={startupStatus.phase}
              startupMessage={startupStatus.message}
            />
          ) : (
            <>
              <section className="hero" aria-labelledby="hero-title">
                <div className="hero-copy">
                  <p className="eyebrow">Nexus / M3 初始索引</p>
                  <h1 id="hero-title">
                    个人数据，
                    <span>留在身边。</span>
                  </h1>
                  <p className="hero-lede">
                    为文件、笔记、代码与想法搭建一处安静可靠的本地基础。
                  </p>
                  <div className="hero-meta">
                    <span>当前界面</span>
                    <strong>文件索引</strong>
                  </div>
                </div>

                <section
                  className={`status-card status-card-${startupStatus.phase}`}
                  aria-labelledby="status-title"
                >
                  <div className="status-card-header">
                    <span className="eyebrow">运行状态</span>
                    <span className="status-state" aria-live="polite">
                      <span className="status-dot" aria-hidden="true" />
                      {presentation.label}
                    </span>
                  </div>
                  <div className="status-card-body">
                    <span className="status-card-index" aria-hidden="true">
                      01
                    </span>
                    <div>
                      <h2 id="status-title">{presentation.title}</h2>
                      <p>{startupStatus.message}</p>
                    </div>
                  </div>
                  <div className="status-card-footer">
                    <span>核心连接</span>
                    <span className="footer-rule" aria-hidden="true" />
                    <strong>{presentation.footer}</strong>
                  </div>
                </section>
              </section>

              <section
                className="rescan-workspace"
                aria-labelledby="rescan-title"
              >
                <div className="section-heading rescan-heading">
                  <p className="eyebrow">本地索引 / 手动任务</p>
                  <h2 id="rescan-title">从一处目录开始。</h2>
                  <p>
                    扫描文件元数据并建立正文索引，所有数据与任务状态都保留在本地。目录路径由你明确输入。
                  </p>
                </div>

                <div className="rescan-card">
                  <div className="rescan-card-header">
                    <div>
                      <span className="rescan-card-index">01 / 初始索引</span>
                      <h3>建立可检索的本地内容</h3>
                    </div>
                    <div
                      className={`scan-phase scan-phase-${rescanPhase}`}
                      aria-live="polite"
                    >
                      <span className="status-dot" aria-hidden="true" />
                      {rescanPresentation.label}
                    </div>
                  </div>

                  <div
                    className={`watch-sync-status watch-sync-status-${watchPhase}`}
                    aria-live="polite"
                    aria-label="文件自动同步状态"
                  >
                    <span className="status-dot" aria-hidden="true" />
                    <span>
                      自动同步 · {watchPhasePresentation[watchPhase].label}
                    </span>
                    <small>{watchMessage}</small>
                  </div>

                  <form
                    className="scan-form"
                    onSubmit={(event) => {
                      event.preventDefault();
                      void handleStartRescan();
                    }}
                  >
                    <label className="path-label" htmlFor="scan-root-path">
                      扫描目录
                      <span>输入完整路径</span>
                    </label>
                    <div className="path-entry">
                      <span className="path-prefix" aria-hidden="true">
                        /
                      </span>
                      <input
                        id="scan-root-path"
                        className="path-input"
                        type="text"
                        value={rootPath}
                        onChange={(event) => {
                          setRootPath(event.target.value);
                          if (rescanPhase === "failed") {
                            rescanPhaseRef.current = "idle";
                            setRescanPhase("idle");
                            setRescanMessage(
                              rescanPhasePresentation.idle.title,
                            );
                          }
                        }}
                        placeholder="例如：C:\\Users\\你的名字\\Documents"
                        autoComplete="off"
                        spellCheck={false}
                        aria-describedby="scan-root-help"
                        disabled={isRescanBusy}
                      />
                    </div>
                    <p id="scan-root-help" className="path-help">
                      支持 Windows 完整路径；扫描过程中不会上传文件或文件名。
                    </p>
                    <div className="scan-actions">
                      <button
                        className="button button-primary"
                        type="submit"
                        disabled={isRescanBusy || rootPath.trim().length === 0}
                      >
                        {rescanPhase === "starting" ? "准备中…" : "开始索引"}
                      </button>
                      <button
                        className="button button-secondary"
                        type="button"
                        onClick={() => {
                          void handleCancelRescan();
                        }}
                        disabled={!isRescanBusy || rescanPhase === "cancelling"}
                      >
                        {rescanPhase === "cancelling" ? "取消中…" : "取消任务"}
                      </button>
                    </div>
                  </form>

                  <div className="scan-progress-block" aria-live="polite">
                    <div className="scan-progress-heading">
                      <span>{rescanMessage}</span>
                      <strong>
                        {formatCount(progress.processed)} 项已处理
                      </strong>
                    </div>
                    <div
                      className={`progress-track progress-track-${rescanPhase}`}
                      role="progressbar"
                      aria-label="本地重扫进度"
                      aria-valuemin={0}
                      aria-valuetext={`${formatCount(progress.processed)} 项已处理`}
                    >
                      <span className="progress-fill" aria-hidden="true" />
                    </div>
                    <div className="scan-metrics" aria-label="重扫统计">
                      <div>
                        <span>成功</span>
                        <strong>{formatCount(progress.filesSucceeded)}</strong>
                      </div>
                      <div>
                        <span>跳过</span>
                        <strong>{formatCount(progress.pathsSkipped)}</strong>
                      </div>
                      <div>
                        <span>失败</span>
                        <strong>{formatCount(progress.filesFailed)}</strong>
                      </div>
                    </div>
                    <div
                      className="scan-metrics document-metrics"
                      aria-label="正文索引统计"
                    >
                      <div>
                        <span>正文写入</span>
                        <strong>
                          {formatCount(progress.documentsSucceeded)}
                        </strong>
                      </div>
                      <div>
                        <span>格式跳过</span>
                        <strong>
                          {formatCount(progress.documentsSkipped)}
                        </strong>
                      </div>
                      <div>
                        <span>正文失败</span>
                        <strong>{formatCount(progress.documentsFailed)}</strong>
                      </div>
                    </div>
                  </div>

                  {summary !== null && rescanPhase === "completed" ? (
                    <div className="scan-result" aria-label="重扫结果">
                      <span className="eyebrow">本次结果</span>
                      <div className="result-grid">
                        <div>
                          <span>写入成功</span>
                          <strong>{formatCount(summary.filesSucceeded)}</strong>
                        </div>
                        <div>
                          <span>正文写入</span>
                          <strong>
                            {formatCount(summary.documentsSucceeded)}
                          </strong>
                        </div>
                        <div>
                          <span>正文失败</span>
                          <strong>
                            {formatCount(summary.documentsFailed)}
                          </strong>
                        </div>
                        <div>
                          <span>移除旧记录</span>
                          <strong>{formatCount(summary.recordsRemoved)}</strong>
                        </div>
                        <div>
                          <span>已提交批次</span>
                          <strong>
                            {formatCount(summary.batchesCommitted)}
                          </strong>
                        </div>
                      </div>
                    </div>
                  ) : null}

                  {rescanPhase === "cancelled" ? (
                    <p className="scan-result-note">
                      本轮任务已停止；已提交的单文件正文索引会保留，未完成的元数据重扫不会应用。
                    </p>
                  ) : null}

                  {incrementalSummary !== null ? (
                    <p className="watch-result-note" aria-live="polite">
                      最近一次自动同步：更新{" "}
                      {formatCount(incrementalSummary.filesUpdated)} 项，移除{" "}
                      {formatCount(incrementalSummary.filesRemoved)} 项，失败{" "}
                      {formatCount(incrementalSummary.filesFailed)} 项。
                    </p>
                  ) : null}

                  {scanId !== null && !isRescanBusy ? (
                    <span className="scan-task-id">任务编号 / {scanId}</span>
                  ) : null}
                </div>
              </section>

              <section
                className="principles"
                aria-labelledby="principles-title"
              >
                <div className="section-heading">
                  <p className="eyebrow">运行原则</p>
                  <h2 id="principles-title">先建立信任。</h2>
                </div>
                <div className="principles-grid">
                  {principles.map((principle) => (
                    <article className="principle" key={principle.index}>
                      <span className="principle-index">{principle.index}</span>
                      <h3>{principle.title}</h3>
                      <p>{principle.description}</p>
                    </article>
                  ))}
                </div>
              </section>

              <section className="next-step" aria-label="下一里程碑">
                <div>
                  <span className="eyebrow">下一步</span>
                  <strong>M4 / 增量索引</strong>
                </div>
                <span className="next-step-copy">
                  让文件变化后，已有内容索引保持同步。
                </span>
                <span className="next-step-arrow" aria-hidden="true">
                  →
                </span>
              </section>
            </>
          )}

          <footer className="page-footer">
            <span>Nexus / 本地优先个人数据操作系统</span>
            <span>
              {activeView === "search" ? "全文搜索" : "文件索引"} · 2026
            </span>
          </footer>
        </div>
      </main>
    </div>
  );
}

export default App;
