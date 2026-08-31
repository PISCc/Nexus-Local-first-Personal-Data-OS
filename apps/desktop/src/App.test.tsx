import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "./App";

describe("App", () => {
  it("渲染工程基础状态界面", () => {
    render(<App />);

    expect(
      screen.getByRole("heading", {
        name: /个人数据，\s*留在身边。/,
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("基础界面已启动。")).toBeInTheDocument();
  });
});
