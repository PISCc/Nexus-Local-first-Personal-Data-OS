import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import SearchView from "./SearchView";

const principles = [
  {
    index: "01",
    title: "资料只留在你的设备",
    description: "文件内容不会上传到云端，除非你主动选择。",
  },
  {
    index: "02",
    title: "先找到，再处理",
    description: "先找到准确的原文件，再决定下一步怎么使用。",
  },
  {
    index: "03",
    title: "每条结果都有出处",
    description: "从搜索结果可以直接回到原始文件，不需要凭记忆寻找。",
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

type SourceConfigResponse = {
  rootPath: string | null;
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

type IndexHealthState =
  "not-configured" | "indexing" | "ready" | "degraded" | "failed" | "cancelled";

type IndexHealthResponse = {
  state: IndexHealthState;
  message: string;
  rootPath: string | null;
  filesIndexed: number;
  documentsIndexed: number;
  watchState: "idle" | "running";
  scanId: number | null;
  progress: RescanProgress | null;
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
  message: "正在准备你的资料，请稍候。",
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
    label: "准备中",
    title: "正在准备你的资料。",
    footer: "请稍候",
  },
  ready: {
    label: "已准备好",
    title: "可以开始使用了。",
    footer: "服务正常",
  },
  degraded: {
    label: "暂时不可用",
    title: "暂时无法读取你的资料。",
    footer: "请稍后重试",
  },
};

const indexHealthPresentation: Record<
  IndexHealthState,
  { label: string; title: string; message: string }
> = {
  "not-configured": {
    label: "待开始",
    title: "还没有可搜索的资料。",
    message: "选择一个文件夹后，Nexus 会为你建立可搜索内容。",
  },
  indexing: {
    label: "整理中",
    title: "正在整理你的资料。",
    message: "已经完成的内容仍然可以搜索。",
  },
  ready: {
    label: "已准备好",
    title: "你的资料已经可以搜索。",
    message: "文件发生变化后，内容会在本机自动更新。",
  },
  degraded: {
    label: "需要处理",
    title: "有些资料还没有整理完成。",
    message: "已有内容仍可搜索；重新整理可以补齐缺少的内容。",
  },
  failed: {
    label: "未完成",
    title: "这次整理没有完成。",
    message: "已有内容会保留，请检查文件夹后重新整理。",
  },
  cancelled: {
    label: "已暂停",
    title: "资料整理已暂停。",
    message: "已经保存的内容会保留，可以随时重新开始。",
  },
};

const rescanPhasePresentation: Record<
  RescanPhase,
  { label: string; title: string }
> = {
  idle: { label: "等待开始", title: "选择一个文件夹开始整理。" },
  starting: { label: "准备中", title: "正在准备资料整理。" },
  running: { label: "整理中", title: "正在整理文件并准备搜索。" },
  cancelling: { label: "停止中", title: "正在停止资料整理。" },
  completed: { label: "已完成", title: "这次资料整理已完成。" },
  cancelled: { label: "已暂停", title: "资料整理已暂停。" },
  failed: { label: "未完成", title: "这次资料整理未完成。" },
};

const watchPhasePresentation: Record<
  WatchPhase,
  { label: string; message: string }
> = {
  idle: { label: "未开启", message: "完成首次整理后，文件变化会自动更新。" },
  starting: { label: "正在开启", message: "正在准备文件自动更新。" },
  watching: { label: "已开启", message: "文件变化会自动更新。" },
  stopped: { label: "已停止", message: "文件自动更新已停止。" },
  failed: {
    label: "需要重试",
    message: "文件自动更新暂时不可用，请重新整理。",
  },
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

function isSourceConfigResponse(value: unknown): value is SourceConfigResponse {
  return (
    isRecord(value) &&
    (value.rootPath === null || typeof value.rootPath === "string")
  );
}

function isIndexHealthResponse(value: unknown): value is IndexHealthResponse {
  if (
    !isRecord(value) ||
    (value.state !== "not-configured" &&
      value.state !== "indexing" &&
      value.state !== "ready" &&
      value.state !== "degraded" &&
      value.state !== "failed" &&
      value.state !== "cancelled") ||
    typeof value.message !== "string" ||
    value.message.length === 0 ||
    value.message.length > 160 ||
    /[\r\n]/u.test(value.message)
  ) {
    return false;
  }

  return (
    (value.rootPath === null || typeof value.rootPath === "string") &&
    isSafeCount(value.filesIndexed) &&
    isSafeCount(value.documentsIndexed) &&
    (value.watchState === "idle" || value.watchState === "running") &&
    (value.scanId === null || isScanId(value.scanId)) &&
    (value.progress === null || isRescanProgress(value.progress))
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
    message: "暂时无法读取你的资料，请重新打开应用后再试。",
  };
}

async function loadSourceConfig(): Promise<SourceConfigResponse | null> {
  try {
    const response = await invoke<unknown>("get_source_config");
    return isSourceConfigResponse(response) ? response : null;
  } catch {
    // 浏览器预览或旧版本核心没有来源配置命令，保持空输入。
    return null;
  }
}

async function loadIndexHealth(): Promise<IndexHealthResponse | null> {
  try {
    const response = await invoke<unknown>("get_index_health");
    return isIndexHealthResponse(response) ? response : null;
  } catch {
    // 浏览器预览或旧版本核心没有索引健康命令，保留核心启动状态作为后备。
    return null;
  }
}

function updateWindowTitle(title: string): void {
  document.title = title;

  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    return;
  }

  void getCurrentWindow()
    .setTitle(title)
    .catch(() => {
      // 浏览器预览或旧版本核心没有窗口标题命令，保留文档标题。
    });
}

function App() {
  const [activeView, setActiveView] = useState<ActiveView>("search");
  const [startupStatus, setStartupStatus] =
    useState<StartupStatus>(initialStartupStatus);
  const [indexHealth, setIndexHealth] = useState<IndexHealthResponse | null>(
    null,
  );
  const [rootPath, setRootPath] = useState("");
  const [rescanPhase, setRescanPhase] = useState<RescanPhase>("idle");
  const [rescanMessage, setRescanMessage] = useState(
    rescanPhasePresentation.idle.title,
  );
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
    updateWindowTitle(
      `Nexus — ${activeView === "search" ? "全文搜索" : "整理资料"}`,
    );
  }, [activeView]);

  useEffect(() => {
    let active = true;

    void loadStartupStatus().then((status) => {
      if (active) {
        setStartupStatus(status);
      }
    });

    void loadSourceConfig().then((config) => {
      const savedRootPath = config?.rootPath;
      if (
        active &&
        savedRootPath !== null &&
        savedRootPath !== undefined &&
        savedRootPath.trim() !== ""
      ) {
        setRootPath((current) =>
          current.trim().length === 0 ? savedRootPath : current,
        );
      }
    });

    void loadIndexHealth().then((health) => {
      if (active && health !== null) {
        setIndexHealth(health);
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

    const refreshIndexHealth = async () => {
      const health = await loadIndexHealth();
      if (active && health !== null) {
        setIndexHealth(health);
      }
    };

    const handleProgress = (payload: unknown) => {
      if (!active || !isRescanProgressEvent(payload)) {
        return;
      }

      if (activeScanIdRef.current === null) {
        if (rescanPhaseRef.current !== "starting") {
          return;
        }
        activeScanIdRef.current = payload.scanId;
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
      }

      if (activeScanIdRef.current !== payload.scanId) {
        return;
      }

      activeScanIdRef.current = null;
      rescanPhaseRef.current = payload.status;
      setRescanPhase(payload.status);
      setRescanMessage(rescanPhasePresentation[payload.status].title);
      setSummary(payload.summary);
      void refreshIndexHealth();

      if (payload.status !== "completed") {
        activeWatchIdRef.current = null;
        watchPhaseRef.current = "idle";
        setWatchPhase("idle");
        setWatchMessage(watchPhasePresentation.idle.message);
        void loadWatchStatus();
      }
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
      setWatchMessage(watchPhasePresentation[payload.state].message);
      void refreshIndexHealth();
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
      void refreshIndexHealth();
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
        void refreshIndexHealth();
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

  const rescanPresentation = rescanPhasePresentation[rescanPhase];
  const isRescanBusy =
    rescanPhase === "starting" ||
    rescanPhase === "running" ||
    rescanPhase === "cancelling";
  const statusCard = (() => {
    if (startupStatus.phase !== "ready") {
      const presentation = statusPresentation[startupStatus.phase];
      return {
        phase: startupStatus.phase,
        label: presentation.label,
        title: presentation.title,
        message:
          startupStatus.phase === "loading"
            ? "请稍候，准备完成后就可以开始整理和搜索。"
            : "请重新打开应用后再试。",
        footerLabel: "当前状态",
        footer: presentation.footer,
      };
    }

    const healthState: IndexHealthState = isRescanBusy
      ? "indexing"
      : (indexHealth?.state ?? "ready");
    const healthPresentation = indexHealthPresentation[healthState];
    if (indexHealth === null && !isRescanBusy) {
      const presentation = statusPresentation.ready;
      return {
        phase: "ready",
        label: presentation.label,
        title: presentation.title,
        message: "你的资料已准备好，可以开始整理和搜索。",
        footerLabel: "当前状态",
        footer: presentation.footer,
      };
    }

    const filesIndexed = indexHealth?.filesIndexed ?? 0;
    const documentsIndexed = indexHealth?.documentsIndexed ?? 0;
    const watchState = indexHealth?.watchState ?? "idle";
    return {
      phase: healthState,
      label: healthPresentation.label,
      title: healthPresentation.title,
      message: isRescanBusy
        ? "正在整理你的资料，已经完成的内容仍可搜索。"
        : healthPresentation.message,
      footerLabel: `${formatCount(filesIndexed)} 个文件 / ${formatCount(
        documentsIndexed,
      )} 项可搜索内容`,
      footer:
        healthState === "indexing"
          ? "正在更新"
          : watchState === "running"
            ? "自动更新已开启"
            : "自动更新未开启",
    };
  })();
  const needsIndexRetry =
    rescanPhase === "cancelled" ||
    rescanPhase === "failed" ||
    indexHealth?.state === "cancelled" ||
    indexHealth?.state === "failed" ||
    indexHealth?.state === "degraded";

  const handleStartRescan = async () => {
    const normalizedRootPath = rootPath.trim();
    if (normalizedRootPath.length === 0 || isRescanBusy) {
      if (normalizedRootPath.length === 0) {
        setRescanMessage("请先填写资料文件夹。");
      }
      return;
    }

    setSummary(null);
    setIncrementalSummary(null);
    setProgress(emptyProgress);
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
        rescanPhaseRef.current = "running";
        setRescanPhase("running");
        setRescanMessage(rescanPhasePresentation.running.title);
      }
    } catch (error) {
      activeScanIdRef.current = null;
      rescanPhaseRef.current = "failed";
      setRescanPhase("failed");
      setRescanMessage(
        safeCommandMessage(error, "无法开始整理，请检查文件夹路径后重试。"),
      );

      try {
        const response = await invoke<unknown>("get_watch_status");
        if (
          isWatchStatusResponse(response) &&
          response.state === "running" &&
          response.watchId !== null
        ) {
          activeWatchIdRef.current = response.watchId;
          latestWatchIdRef.current = response.watchId;
          watchPhaseRef.current = "watching";
          setWatchPhase("watching");
          setWatchMessage(watchPhasePresentation.watching.message);
        } else {
          activeWatchIdRef.current = null;
          watchPhaseRef.current = "idle";
          setWatchPhase("idle");
          setWatchMessage(watchPhasePresentation.idle.message);
        }
      } catch {
        activeWatchIdRef.current = null;
        watchPhaseRef.current = "failed";
        setWatchPhase("failed");
        setWatchMessage(watchPhasePresentation.failed.message);
      }
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
        setRescanMessage(
          safeCommandMessage(error, "无法停止整理，请稍后再试。"),
        );
      }
    }
  };

  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-content">
        跳到主要内容
      </a>
      <aside className="side-rail" aria-label="Nexus 导航">
        <div>
          <div className="brand-lockup">
            <img
              className="brand-icon"
              src="/nexus-product-icon.png"
              alt="Nexus 产品图标"
              width="34"
              height="34"
            />
            <div className="brand-copy">
              <span className="brand-index">N / 01</span>
              <span className="brand-name">Nexus</span>
            </div>
          </div>
          <p className="rail-caption">你的资料，随时找回</p>
        </div>

        <nav className="rail-nav" aria-label="主要功能">
          <button
            className={`rail-item ${activeView === "index" ? "active" : "quiet"}`}
            type="button"
            onClick={() => {
              setActiveView("index");
            }}
            aria-current={activeView === "index" ? "page" : undefined}
          >
            <span className="rail-item-index">01</span>
            <span className="rail-item-label">整理资料</span>
            <span className="rail-item-state">
              {activeView === "index" ? "当前" : "打开"}
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
            <span className="rail-item-index">02</span>
            <span className="rail-item-label">全文搜索</span>
            <span className="rail-item-state">
              {activeView === "search" ? "当前" : "打开"}
            </span>
          </button>
        </nav>

        <div className="rail-footer">
          <span className="rail-footer-label">资料位置</span>
          <div className="local-badge">
            <span className="status-dot" aria-hidden="true" />
            只在这台设备
          </div>
          <p>你的文件不会离开这台设备。</p>
        </div>
      </aside>

      <main id="main-content" className="main-surface" tabIndex={-1}>
        <header className="topbar">
          <div className="breadcrumb">
            <span>我的资料</span>
            <span className="breadcrumb-separator" aria-hidden="true">
              /
            </span>
            <strong>{activeView === "search" ? "全文搜索" : "整理资料"}</strong>
          </div>
          <div className="topbar-meta">
            <span className="version-pill">资料留在本机</span>
            <span className="topbar-note">无需联网</span>
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
                  <p className="eyebrow">整理你的资料</p>
                  <h1 id="hero-title">
                    个人数据，
                    <span>留在身边。</span>
                  </h1>
                  <p className="hero-lede">
                    把文件、笔记和代码整理好，需要时马上找得到。
                  </p>
                  <div className="hero-meta">
                    <span>当前功能</span>
                    <strong>整理资料</strong>
                  </div>
                </div>

                <section
                  className={`status-card status-card-${statusCard.phase}`}
                  aria-labelledby="status-title"
                >
                  <div className="status-card-header">
                    <span className="eyebrow">资料状态</span>
                    <span className="status-state" aria-live="polite">
                      <span className="status-dot" aria-hidden="true" />
                      {statusCard.label}
                    </span>
                  </div>
                  <div className="status-card-body">
                    <span className="status-card-index" aria-hidden="true">
                      01
                    </span>
                    <div>
                      <h2 id="status-title">{statusCard.title}</h2>
                      <p>{statusCard.message}</p>
                    </div>
                  </div>
                  <div className="status-card-footer">
                    <span>{statusCard.footerLabel}</span>
                    <span className="footer-rule" aria-hidden="true" />
                    <strong>{statusCard.footer}</strong>
                  </div>
                </section>
              </section>

              <section
                className="rescan-workspace"
                aria-labelledby="rescan-title"
              >
                <div className="section-heading rescan-heading">
                  <p className="eyebrow">整理资料</p>
                  <h2 id="rescan-title">先选择一个文件夹。</h2>
                  <p>
                    Nexus
                    会在这台设备上整理文件内容，供你稍后搜索；原始文件不会被上传。
                  </p>
                </div>

                <div className="rescan-card">
                  <div className="rescan-card-header">
                    <div>
                      <span className="rescan-card-index">
                        第一步 / 选择资料
                      </span>
                      <h3>让这些资料可以被找到</h3>
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
                    aria-label="文件自动更新状态"
                  >
                    <span className="status-dot" aria-hidden="true" />
                    <span>
                      自动更新 · {watchPhasePresentation[watchPhase].label}
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
                      资料文件夹
                      <span>上次使用的文件夹会自动填入</span>
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
                      填写文件夹完整路径。整理完成后会记住它；你的文件和文件名不会离开这台设备。
                    </p>
                    <div className="scan-actions">
                      <button
                        className="button button-primary"
                        type="submit"
                        disabled={isRescanBusy || rootPath.trim().length === 0}
                      >
                        {rescanPhase === "starting"
                          ? "准备中…"
                          : needsIndexRetry
                            ? "重新整理"
                            : "开始整理"}
                      </button>
                      <button
                        className="button button-secondary"
                        type="button"
                        onClick={() => {
                          void handleCancelRescan();
                        }}
                        disabled={!isRescanBusy || rescanPhase === "cancelling"}
                      >
                        {rescanPhase === "cancelling" ? "停止中…" : "停止整理"}
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
                      aria-label="资料整理进度"
                      aria-valuemin={0}
                      aria-valuetext={`${formatCount(progress.processed)} 项已处理`}
                    >
                      <span className="progress-fill" aria-hidden="true" />
                    </div>
                    <div className="scan-metrics" aria-label="整理进度统计">
                      <div>
                        <span>已整理文件</span>
                        <strong>{formatCount(progress.filesSucceeded)}</strong>
                      </div>
                      <div>
                        <span>已跳过</span>
                        <strong>{formatCount(progress.pathsSkipped)}</strong>
                      </div>
                      <div>
                        <span>需要注意</span>
                        <strong>{formatCount(progress.filesFailed)}</strong>
                      </div>
                    </div>
                    <div
                      className="scan-metrics document-metrics"
                      aria-label="可搜索内容统计"
                    >
                      <div>
                        <span>可搜索内容</span>
                        <strong>
                          {formatCount(progress.documentsSucceeded)}
                        </strong>
                      </div>
                      <div>
                        <span>未支持格式</span>
                        <strong>
                          {formatCount(progress.documentsSkipped)}
                        </strong>
                      </div>
                      <div>
                        <span>需要重试</span>
                        <strong>{formatCount(progress.documentsFailed)}</strong>
                      </div>
                    </div>
                  </div>

                  {summary !== null && rescanPhase === "completed" ? (
                    <div className="scan-result" aria-label="整理结果">
                      <span className="eyebrow">整理结果</span>
                      <div className="result-grid">
                        <div>
                          <span>已整理文件</span>
                          <strong>{formatCount(summary.filesSucceeded)}</strong>
                        </div>
                        <div>
                          <span>可搜索内容</span>
                          <strong>
                            {formatCount(summary.documentsSucceeded)}
                          </strong>
                        </div>
                        <div>
                          <span>需要注意</span>
                          <strong>
                            {formatCount(
                              summary.filesFailed + summary.documentsFailed,
                            )}
                          </strong>
                        </div>
                        <div>
                          <span>已移除旧内容</span>
                          <strong>{formatCount(summary.recordsRemoved)}</strong>
                        </div>
                      </div>
                    </div>
                  ) : null}

                  {rescanPhase === "cancelled" ? (
                    <p className="scan-result-note">
                      这次整理已停止；已经保存的内容会保留，未完成部分不会影响原始文件。
                    </p>
                  ) : null}

                  {incrementalSummary !== null ? (
                    <p className="watch-result-note" aria-live="polite">
                      最近一次自动更新：更新{" "}
                      {formatCount(incrementalSummary.filesUpdated)} 项，移除{" "}
                      {formatCount(incrementalSummary.filesRemoved)}{" "}
                      项，需要注意 {formatCount(incrementalSummary.filesFailed)}{" "}
                      项。
                    </p>
                  ) : null}
                </div>
              </section>

              <section
                className="principles"
                aria-labelledby="principles-title"
              >
                <div className="section-heading">
                  <p className="eyebrow">使用方式</p>
                  <h2 id="principles-title">让资料更容易找回。</h2>
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

              <section className="next-step" aria-label="使用提示">
                <div>
                  <span className="eyebrow">小提示</span>
                  <strong>整理完成后，直接搜索就好。</strong>
                </div>
                <span className="next-step-copy">
                  文件发生变化时，Nexus 会在本机自动更新内容。
                </span>
                <span className="next-step-arrow" aria-hidden="true">
                  →
                </span>
              </section>
            </>
          )}

          <footer className="page-footer">
            <span>你的资料只保留在这台设备上。</span>
            <span>
              {activeView === "search" ? "全文搜索" : "整理资料"} · 2026
            </span>
          </footer>
        </div>
      </main>
    </div>
  );
}

export default App;
