import { render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const mockedInvoke = vi.mocked(invoke);

describe("App", () => {
  beforeEach(() => {
    mockedInvoke.mockReset();
    mockedInvoke.mockResolvedValue({
      phase: "ready",
      message: "本地核心已就绪。",
    });
  });

  it("渲染工程基础状态界面并显示就绪状态", async () => {
    render(<App />);

    expect(
      screen.getByRole("heading", {
        name: /个人数据，\s*留在身边。/,
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
});
