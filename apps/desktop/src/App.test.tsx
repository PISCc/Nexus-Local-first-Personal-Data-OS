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
    expect(screen.getByRole("img", { name: "Nexus 产品图标" })).toHaveAttribute(
      "src",
      "/nexus-product-icon.png",
    );
    expect(
      screen.getByRole("heading", { name: "正在准备你的资料。" }),
    ).toBeInTheDocument();

    await waitFor(() => {
      expect(document.title).toBe("Nexus — 全文搜索");
    });

    await waitFor(() => {
      expect(screen.getByText("你的资料可以搜索了。")).toBeInTheDocument();
    });
    expect(mockedInvoke).toHaveBeenCalledWith("get_startup_status");
  });

  it("提供跳到主要内容的键盘入口并随视图更新标题", async () => {
    render(<App />);

    expect(screen.getByRole("link", { name: "跳到主要内容" })).toHaveAttribute(
      "href",
      "#main-content",
    );

    fireEvent.click(screen.getByRole("button", { name: /整理资料/ }));

    await waitFor(() => {
      expect(document.title).toBe("Nexus — 整理资料");
    });
  });

  it("显示资料暂时不可用时的用户提示", async () => {
    mockedInvoke.mockRejectedValueOnce(new Error("模拟核心不可用"));

    render(<App />);

    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "暂时无法读取你的资料。" }),
      ).toBeInTheDocument();
    });
    expect(screen.getByText("请重新打开应用后再试。")).toBeInTheDocument();
  });

  it("不把内部启动信息直接展示给用户", async () => {
    mockedInvoke.mockResolvedValueOnce({
      phase: "degraded",
      message: "本地数据存储暂时不可用。",
    });

    render(<App />);

    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "暂时无法读取你的资料。" }),
      ).toBeInTheDocument();
    });
    expect(screen.getByText("请重新打开应用后再试。")).toBeInTheDocument();
    expect(
      screen.queryByText("本地数据存储暂时不可用。"),
    ).not.toBeInTheDocument();
  });

  it("回填已保存的来源目录", async () => {
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_startup_status") {
        return {
          phase: "ready",
          message: "本地核心已就绪。",
        };
      }
      if (command === "get_source_config") {
        return { rootPath: "C:\\Nexus\\资料" };
      }
      if (command === "get_rescan_status") {
        return {
          state: "idle",
          scanId: null,
          progress: null,
        };
      }
      return undefined;
    });

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /整理资料/ }));

    await waitFor(() => {
      expect(screen.getByRole("textbox", { name: /资料文件夹/ })).toHaveValue(
        "C:\\Nexus\\资料",
      );
    });
  });

  it("显示真实索引健康状态与本地统计", async () => {
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "get_startup_status") {
        return {
          phase: "ready",
          message: "本地核心已就绪。",
        };
      }
      if (command === "get_index_health") {
        return {
          state: "ready",
          message: "本地索引可用，文件变化会自动同步。",
          rootPath: "C:\\Nexus\\资料",
          filesIndexed: 42,
          documentsIndexed: 35,
          watchState: "running",
          scanId: null,
          progress: null,
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
          state: "running",
          watchId: 4,
        };
      }
      return undefined;
    });

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /整理资料/ }));

    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "你的资料已经可以搜索。" }),
      ).toBeInTheDocument();
    });
    expect(screen.getByText("42 个文件 / 35 项可搜索内容")).toBeInTheDocument();
    expect(screen.getByText("自动更新已开启")).toBeInTheDocument();
    expect(mockedInvoke).toHaveBeenCalledWith("get_index_health");
  });

  it("拒绝非法的启动状态响应并显示安全降级状态", async () => {
    mockedInvoke.mockResolvedValueOnce({ phase: "ready" });

    render(<App />);

    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "暂时无法读取你的资料。" }),
      ).toBeInTheDocument();
    });
    expect(screen.getByText("请重新打开应用后再试。")).toBeInTheDocument();
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
    fireEvent.click(screen.getByRole("button", { name: /整理资料/ }));

    const input = screen.getByRole("textbox", { name: /资料文件夹/ });
    fireEvent.change(input, { target: { value: "C:\\Nexus\\资料" } });
    fireEvent.click(screen.getByRole("button", { name: "开始整理" }));

    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "正在整理你的资料。" }),
      ).toBeInTheDocument();
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
    expect(screen.getByText("已整理文件")).toBeInTheDocument();

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
      screen.getByText("这次资料整理已完成。", { exact: true }),
    ).toBeInTheDocument();
    expect(screen.getByText("已移除旧内容")).toBeInTheDocument();
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
    fireEvent.click(screen.getByRole("button", { name: /整理资料/ }));
    fireEvent.change(screen.getByRole("textbox", { name: /资料文件夹/ }), {
      target: { value: "C:\\Nexus\\资料" },
    });
    fireEvent.click(screen.getByRole("button", { name: "开始整理" }));

    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "正在整理你的资料。" }),
      ).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: "停止整理" }));

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith("cancel_rescan", {
        request: { scanId: 12 },
      });
    });
    expect(screen.getByText("停止中")).toBeInTheDocument();
  });

  it("重扫启动失败时恢复实际自动同步状态", async () => {
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
          state: "running",
          watchId: 31,
        };
      }
      if (command === "start_rescan") {
        throw new Error("模拟重扫启动失败");
      }
      return undefined;
    });

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /整理资料/ }));

    await waitFor(() => {
      expect(
        screen.getByText("自动更新 · 已开启", { exact: true }),
      ).toBeInTheDocument();
    });

    fireEvent.change(screen.getByRole("textbox", { name: /资料文件夹/ }), {
      target: { value: "C:\\Nexus\\资料" },
    });
    fireEvent.click(screen.getByRole("button", { name: "开始整理" }));

    await waitFor(() => {
      expect(
        screen.getByText("自动更新 · 已开启", { exact: true }),
      ).toBeInTheDocument();
    });
    expect(
      screen.queryByText("自动更新 · 需要重试", { exact: true }),
    ).not.toBeInTheDocument();
  });

  it("重扫取消后不会把自动同步留在准备中", async () => {
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
      if (command === "start_rescan") {
        return { scanId: 32 };
      }
      return undefined;
    });

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /整理资料/ }));
    fireEvent.change(screen.getByRole("textbox", { name: /资料文件夹/ }), {
      target: { value: "C:\\Nexus\\资料" },
    });
    fireEvent.click(screen.getByRole("button", { name: "开始整理" }));

    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "正在整理你的资料。" }),
      ).toBeInTheDocument();
    });

    const finishedCall = mockedListen.mock.calls.find(
      ([eventName]) => eventName === "rescan-finished",
    );
    const finishedListener = finishedCall?.[1] as
      ((event: { payload: unknown }) => void) | undefined;

    await act(async () => {
      finishedListener?.({
        payload: {
          scanId: 32,
          status: "cancelled",
          message: "手动重扫已取消。",
          summary: null,
          errorKind: "rescan_cancelled",
        },
      });
    });

    expect(
      screen.getByText("自动更新 · 未开启", { exact: true }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "重新整理" }),
    ).toBeInTheDocument();
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
    fireEvent.click(screen.getByRole("button", { name: /整理资料/ }));

    await waitFor(() => {
      expect(
        screen.getByText("自动更新 · 未开启", { exact: true }),
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
      screen.getByText("自动更新 · 已开启", { exact: true }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "最近一次自动更新：更新 2 项，移除 1 项，需要注意 0 项。",
        {
          exact: true,
        },
      ),
    ).toBeInTheDocument();
  });
});
