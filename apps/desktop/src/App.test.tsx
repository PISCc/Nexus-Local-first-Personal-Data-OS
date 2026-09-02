import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

const mockedInvoke = vi.mocked(invoke);
const mockedListen = vi.mocked(listen);

describe("App", () => {
  beforeEach(() => {
    mockedInvoke.mockReset();
    mockedInvoke.mockResolvedValue({
      phase: "ready",
      message: "本地核心已就绪。",
    });
    mockedListen.mockReset();
    mockedListen.mockResolvedValue(() => {});
  });

  it("渲染全文搜索界面并显示就绪状态", async () => {
    render(<App />);

    expect(
      screen.getByRole("heading", {
        name: /把想起的词，\s*找回来。/,
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "正在连接本地核心。" }),
    ).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByText("基础界面已启动。")).toBeInTheDocument();
    });
    expect(mockedInvoke).toHaveBeenCalledWith("get_startup_status");
  });

  it("显示核心不可用时的降级状态", async () => {
    mockedInvoke.mockRejectedValueOnce(new Error("模拟核心不可用"));

    render(<App />);

    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "本地核心暂不可用。" }),
      ).toBeInTheDocument();
    });
    expect(
      screen.getByText("桌面核心暂不可用，当前处于降级模式。"),
    ).toBeInTheDocument();
  });

  it("显示核心主动返回的降级状态", async () => {
    mockedInvoke.mockResolvedValueOnce({
      phase: "degraded",
      message: "本地数据存储暂时不可用。",
    });

    render(<App />);

    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "本地核心暂不可用。" }),
      ).toBeInTheDocument();
    });
    expect(screen.getByText("本地数据存储暂时不可用。")).toBeInTheDocument();
  });

  it("拒绝非法的启动状态响应并显示安全降级状态", async () => {
    mockedInvoke.mockResolvedValueOnce({ phase: "ready" });

    render(<App />);

    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "本地核心暂不可用。" }),
      ).toBeInTheDocument();
    });
    expect(
      screen.getByText("桌面核心暂不可用，当前处于降级模式。"),
    ).toBeInTheDocument();
  });

  it("启动重扫并只接收当前任务的进度与完成事件", async () => {
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_startup_status") {
        return {
          phase: "ready",
          message: "本地核心已就绪。",
        };
      }
      if (command === "get_rescan_status") {
        return {
          state: "idle",
          scanId: null,
          progress: null,
        };
      }
      if (command === "start_rescan") {
        return { scanId: 11 };
      }
      return undefined;
    });

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /文件索引/ }));

    const input = screen.getByRole("textbox", { name: /扫描目录/ });
    fireEvent.change(input, { target: { value: "C:\\Nexus\\资料" } });
    fireEvent.click(screen.getByRole("button", { name: "开始索引" }));

    await waitFor(() => {
      expect(screen.getByText("扫描中")).toBeInTheDocument();
    });
    expect(mockedInvoke).toHaveBeenCalledWith("start_rescan", {
      request: {
        rootPath: "C:\\Nexus\\资料",
        ignoredPaths: [],
        followSymlinks: false,
        indexContent: true,
      },
    });

    const progressCall = mockedListen.mock.calls.find(
      ([eventName]) => eventName === "rescan-progress",
    );
    const finishedCall = mockedListen.mock.calls.find(
      ([eventName]) => eventName === "rescan-finished",
    );
    const progressListener = progressCall?.[1] as
      ((event: { payload: unknown }) => void) | undefined;
    const finishedListener = finishedCall?.[1] as
      ((event: { payload: unknown }) => void) | undefined;

    expect(progressListener).toBeDefined();
    expect(finishedListener).toBeDefined();

    await act(async () => {
      progressListener?.({
        payload: {
          scanId: 99,
          processed: 999,
          filesSucceeded: 999,
          filesFailed: 0,
          pathsSkipped: 0,
          documentsSucceeded: 0,
          documentsFailed: 0,
          documentsSkipped: 0,
        },
      });
    });
    expect(screen.getByText("0 项已处理")).toBeInTheDocument();

    await act(async () => {
      progressListener?.({
        payload: {
          scanId: 11,
          processed: 5,
          filesSucceeded: 4,
          filesFailed: 0,
          pathsSkipped: 1,
          documentsSucceeded: 3,
          documentsFailed: 0,
          documentsSkipped: 1,
        },
      });
    });
    expect(screen.getByText("5 项已处理")).toBeInTheDocument();
    expect(screen.getByText("成功")).toBeInTheDocument();

    await act(async () => {
      finishedListener?.({
        payload: {
          scanId: 11,
          status: "completed",
          message: "手动重扫完成。",
          summary: {
            filesSucceeded: 4,
            filesFailed: 0,
            pathsSkipped: 1,
            documentsSucceeded: 3,
            documentsFailed: 0,
            documentsSkipped: 1,
            recordsRemoved: 2,
            batchesCommitted: 1,
          },
          errorKind: null,
        },
      });
    });

    expect(
      screen.getByText("手动重扫完成。", { exact: true }),
    ).toBeInTheDocument();
    expect(screen.getByText("移除旧记录")).toBeInTheDocument();
    expect(screen.getByText("2", { selector: "strong" })).toBeInTheDocument();
  });

  it("向当前重扫任务发送取消请求", async () => {
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_startup_status") {
        return {
          phase: "ready",
          message: "本地核心已就绪。",
        };
      }
      if (command === "get_rescan_status") {
        return {
          state: "idle",
          scanId: null,
          progress: null,
        };
      }
      if (command === "start_rescan") {
        return { scanId: 12 };
      }
      if (command === "cancel_rescan") {
        return { scanId: 12, accepted: true };
      }
      return undefined;
    });

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /文件索引/ }));
    fireEvent.change(screen.getByRole("textbox", { name: /扫描目录/ }), {
      target: { value: "C:\\Nexus\\资料" },
    });
    fireEvent.click(screen.getByRole("button", { name: "开始索引" }));

    await waitFor(() => {
      expect(screen.getByText("扫描中")).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: "取消任务" }));

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith("cancel_rescan", {
        request: { scanId: 12 },
      });
    });
    expect(screen.getByText("取消中")).toBeInTheDocument();
  });

  it("显示后台自动同步状态与最近一次增量结果", async () => {
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_startup_status") {
        return {
          phase: "ready",
          message: "本地核心已就绪。",
        };
      }
      if (command === "get_rescan_status") {
        return {
          state: "idle",
          scanId: null,
          progress: null,
        };
      }
      if (command === "get_watch_status") {
        return {
          state: "idle",
          watchId: null,
        };
      }
      return undefined;
    });

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /文件索引/ }));

    await waitFor(() => {
      expect(
        screen.getByText("自动同步 · 未开启", { exact: true }),
      ).toBeInTheDocument();
    });

    const watchStatusCall = mockedListen.mock.calls.find(
      ([eventName]) => eventName === "watch-status",
    );
    const incrementalFinishedCall = mockedListen.mock.calls.find(
      ([eventName]) => eventName === "incremental-finished",
    );
    const watchStatusListener = watchStatusCall?.[1] as
      ((event: { payload: unknown }) => void) | undefined;
    const incrementalFinishedListener = incrementalFinishedCall?.[1] as
      ((event: { payload: unknown }) => void) | undefined;

    expect(watchStatusListener).toBeDefined();
    expect(incrementalFinishedListener).toBeDefined();

    await act(async () => {
      watchStatusListener?.({
        payload: {
          watchId: 21,
          state: "watching",
          message: "文件变化会在本地自动同步。",
          errorKind: null,
        },
      });
      incrementalFinishedListener?.({
        payload: {
          watchId: 21,
          changesReceived: 3,
          filesUpdated: 2,
          filesRemoved: 1,
          filesFailed: 0,
          documentsUpdated: 2,
          documentsRemoved: 1,
          retries: 0,
          fullRescan: false,
        },
      });
    });

    expect(
      screen.getByText("自动同步 · 已开启", { exact: true }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("最近一次自动同步：更新 2 项，移除 1 项，失败 0 项。", {
        exact: true,
      }),
    ).toBeInTheDocument();
  });
});
