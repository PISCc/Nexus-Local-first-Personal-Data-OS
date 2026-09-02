import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SearchView from "./SearchView";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const mockedInvoke = vi.mocked(invoke);

const readyProps = {
  startupPhase: "ready" as const,
  startupMessage: "本地核心已就绪。",
};

function makeResult(overrides: Record<string, unknown> = {}) {
  return {
    documentId: "document-1",
    sourcePath: "C:\\Nexus\\notes.md",
    title: "项目计划",
    fileName: "notes.md",
    extension: "md",
    fileType: "text/markdown",
    modifiedAt: Date.UTC(2026, 0, 2),
    createdAt: null,
    accessedAt: null,
    lineStart: null,
    lineEnd: null,
    relevance: 1.23,
    snippet: "⟦项目计划⟧ 的正文",
    semanticSimilarity: 0.91,
    fusionScore: 0.031,
    lexicalRank: 1,
    semanticRank: 2,
    ...overrides,
  };
}

function submitSearch(query = "项目计划") {
  fireEvent.change(screen.getByRole("searchbox", { name: /搜索本地文档/ }), {
    target: { value: query },
  });
  fireEvent.click(screen.getByRole("button", { name: "搜索" }));
}

describe("SearchView", () => {
  beforeEach(() => {
    mockedInvoke.mockReset();
  });

  it("显示空闲搜索状态而不提前调用本地搜索", () => {
    render(<SearchView {...readyProps} />);

    expect(
      screen.getByRole("heading", { name: "把想起的词，找回来。" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("输入关键词或筛选条件后开始搜索。"),
    ).toBeInTheDocument();
    expect(mockedInvoke).not.toHaveBeenCalledWith(
      "search_documents",
      expect.anything(),
    );
  });

  it("提交查询并显示本地结果和匹配片段", async () => {
    mockedInvoke.mockResolvedValueOnce({ results: [makeResult()] });
    render(<SearchView {...readyProps} />);

    submitSearch();

    await waitFor(() => {
      expect(
        screen.getByText("项目计划", { selector: "strong" }),
      ).toBeInTheDocument();
    });
    expect(mockedInvoke).toHaveBeenCalledWith("search_documents", {
      request: { query: "项目计划", limit: 100 },
    });
    expect(screen.getByText("C:\\Nexus\\notes.md")).toBeInTheDocument();
    expect(
      screen.getByText("项目计划", { selector: "mark" }),
    ).toBeInTheDocument();
    expect(screen.getByText("词法 1.23")).toBeInTheDocument();
    expect(screen.getByText("混合 0.0310")).toBeInTheDocument();
  });

  it("显示空结果和后端安全错误", async () => {
    mockedInvoke.mockResolvedValueOnce({ results: [] });
    render(<SearchView {...readyProps} />);
    submitSearch("不存在的词");

    await waitFor(() => {
      expect(screen.getByText("没有找到匹配内容。")).toBeInTheDocument();
    });

    mockedInvoke.mockRejectedValueOnce({
      code: "search_query_invalid",
      message: "搜索条件格式无效，请检查关键词、引号和筛选语法。",
    });
    submitSearch("invalid");

    await waitFor(() => {
      expect(
        screen.getByText("搜索条件格式无效，请检查关键词、引号和筛选语法。"),
      ).toBeInTheDocument();
    });

    mockedInvoke.mockResolvedValueOnce({ results: [{}] });
    submitSearch("malformed");

    await waitFor(() => {
      expect(screen.getByText("本地搜索暂时不可用。")).toBeInTheDocument();
    });
  });

  it("取消查询后忽略晚到的结果", async () => {
    let resolveSearch: ((value: unknown) => void) | undefined;
    const pendingSearch = new Promise<unknown>((resolve) => {
      resolveSearch = resolve;
    });
    mockedInvoke.mockReturnValueOnce(pendingSearch);
    render(<SearchView {...readyProps} />);

    submitSearch();
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "取消查询" })).toBeEnabled();
    });

    fireEvent.click(screen.getByRole("button", { name: "取消查询" }));
    expect(screen.getByText("本次搜索已取消。")).toBeInTheDocument();

    await act(async () => {
      resolveSearch?.({ results: [makeResult()] });
      await pendingSearch;
    });
    expect(screen.queryByText("C:\\Nexus\\notes.md")).not.toBeInTheDocument();
  });

  it("通过文档 ID 请求定位原始文件", async () => {
    mockedInvoke
      .mockResolvedValueOnce({ results: [makeResult()] })
      .mockResolvedValueOnce(undefined);
    render(<SearchView {...readyProps} />);

    submitSearch();
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "打开 notes.md" }),
      ).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: "打开 notes.md" }));

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith("open_document", {
        request: { documentId: "document-1" },
      });
    });
  });
});
