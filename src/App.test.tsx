import { fireEvent, render, screen, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import type { EnvReport } from "./lib/types";
import { emptySnapshot } from "./test/inventory-fixtures";
import {
  cliMissingReport,
  gitMissingReport,
  healthyReport,
  messages,
  nodeMissingReport,
  nodeTooOldReport,
  npxMissingReport,
} from "./test/fixtures";

function renderApp() {
  const queryClient = createQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>,
  );
}

function mockEnvironment(report: EnvReport | (() => EnvReport)) {
  mockIPC((command) => {
    if (command === "get_environment") {
      return typeof report === "function" ? report() : report;
    }
    if (command === "get_inventory") {
      return emptySnapshot();
    }
    throw new Error(`unexpected command: ${command}`);
  });
}

afterEach(() => {
  clearMocks();
});

describe("app shell", () => {
  it("renders the three-column workspace with a healthy environment", async () => {
    mockEnvironment(healthyReport());
    renderApp();

    expect(await screen.findByText("环境正常")).toBeInTheDocument();
    expect(screen.getByText("PATH · nvm")).toBeInTheDocument();
    expect(screen.getByText("Node v24.15.0")).toBeInTheDocument();
    expect(screen.getByText("npx 11.12.1")).toBeInTheDocument();
    expect(screen.getByText("git 2.54.0")).toBeInTheDocument();
    expect(screen.getByText("skills 1.7.4")).toBeInTheDocument();
    expect(screen.getByText("Agents 2")).toBeInTheDocument();
    expect(screen.getByText("Lock v3 · 61 条")).toBeInTheDocument();

    expect(screen.getByRole("heading", { name: "筛选" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "详情" })).toBeInTheDocument();
    expect(screen.getByText("暂无 skill")).toBeInTheDocument();
    expect(screen.getByText("未选择 skill")).toBeInTheDocument();
    expect(screen.getByText("Claude Code")).toBeInTheDocument();

    expect(
      screen.getByRole("button", { name: "安装 skill" }),
    ).toBeEnabled();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByText(/Welcome to Tauri/)).not.toBeInTheDocument();
  });

  const issueCases: Array<{
    name: string;
    report: () => EnvReport;
    message: string;
  }> = [
    {
      name: "Node missing",
      report: nodeMissingReport,
      message: messages.nodeMissing,
    },
    {
      name: "Node too old",
      report: nodeTooOldReport,
      message: messages.nodeTooOld,
    },
    {
      name: "npx missing",
      report: npxMissingReport,
      message: messages.npxMissing,
    },
    {
      name: "git missing",
      report: gitMissingReport,
      message: messages.gitMissing,
    },
    {
      name: "CLI missing",
      report: cliMissingReport,
      message: messages.cliMissing,
    },
  ];

  for (const issueCase of issueCases) {
    it(`shows a ${issueCase.name} banner and disables mutations`, async () => {
      mockEnvironment(issueCase.report());
      renderApp();

      const banner = await screen.findByRole("alert");
      expect(within(banner).getByText(issueCase.message)).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "安装 skill" }),
      ).toBeDisabled();

      // The read-only shell must stay usable while the environment is broken.
      expect(screen.getByRole("heading", { name: "筛选" })).toBeInTheDocument();
      expect(screen.getByRole("heading", { name: "详情" })).toBeInTheDocument();
      expect(screen.getByText("暂无 skill")).toBeInTheDocument();
    });
  }

  it("survives an environment probe failure and offers a retry", async () => {
    mockIPC(() => {
      throw new Error("后端命令不可用");
    });
    renderApp();

    expect(await screen.findByText("环境检测失败")).toBeInTheDocument();
    expect(screen.getByText("后端命令不可用")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "安装 skill" })).toBeDisabled();
    expect(screen.getByRole("heading", { name: "筛选" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "详情" })).toBeInTheDocument();
  });

  it("renders a structured AppError rejection from the backend", async () => {
    mockIPC(() => {
      throw {
        code: "path.app_data",
        message: "无法创建应用数据目录。",
        detail: "permission denied",
        retryable: false,
      };
    });
    renderApp();

    expect(await screen.findByText("环境检测失败")).toBeInTheDocument();
    expect(
      screen.getByText("无法创建应用数据目录。 · permission denied"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "安装 skill" })).toBeDisabled();
  });

  it("rechecks the environment through the single api gateway", async () => {
    const payloads: Array<{ refresh?: boolean }> = [];
    let calls = 0;
    mockIPC((command, payload) => {
      if (command !== "get_environment") {
        throw new Error(`unexpected command: ${command}`);
      }
      payloads.push(payload as { refresh?: boolean });
      calls += 1;
      return calls === 1 ? cliMissingReport() : healthyReport();
    });

    renderApp();
    const banner = await screen.findByRole("alert");
    fireEvent.click(
      within(banner).getByRole("button", { name: "重新检测" }),
    );

    expect(await screen.findByText("环境正常")).toBeInTheDocument();
    expect(payloads).toHaveLength(2);
    expect(payloads[0]?.refresh).toBe(false);
    expect(payloads[1]?.refresh).toBe(true);
  });
});
