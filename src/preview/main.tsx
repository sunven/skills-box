import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import App from "../App";
import { createQueryClient } from "../lib/query-client";
import type { TaskEvent, TaskResult } from "../lib/types";
import {
  environment,
  previewSummary,
  skillDetail,
  snapshot,
  updateReport,
} from "./fixtures";
import "../index.css";

interface MockChannel {
  id: number;
  onmessage?: (message: TaskEvent) => void;
}

const messageIndex = new WeakMap<object, number>();

function emit(channel: MockChannel, event: TaskEvent) {
  const index = messageIndex.get(channel) ?? 0;
  messageIndex.set(channel, index + 1);
  (
    window as unknown as {
      __TAURI_INTERNALS__: {
        runCallback: (id: number, data: unknown) => void;
      };
    }
  ).__TAURI_INTERNALS__.runCallback(channel.id, { index, message: event });
}

function finishedResult(
  action: TaskResult["action"],
  names: string[],
  status: "installed" | "updated",
): TaskResult {
  return {
    action,
    overall: "success",
    skills: names.map((name) => ({ name, status })),
    warnings: [],
    backupPath:
      "~/Library/Application Support/com.sunven.skills-box/backups/2026-10-28T02-18-42",
    logPath:
      "~/Library/Application Support/com.sunven.skills-box/logs/2026-10-28T02-18-42.log",
    startedAt: Date.now() - 4200,
    finishedAtEpochMs: Date.now(),
    preview: action === "preview" ? previewSummary : undefined,
  };
}

function simulate(
  channel: MockChannel,
  kind: "preview" | "install" | "update",
  names: string[],
) {
  window.setTimeout(() => {
    emit(channel, {
      taskId: `preview-${kind}`,
      phase: kind === "preview" ? "cloning" : "preparing",
      message: kind === "preview" ? "正在读取仓库…" : "正在准备写入…",
      completed: 0,
      total: kind === "preview" ? 1 : names.length,
    });
  }, 80);
  window.setTimeout(() => {
    const result = finishedResult(
      kind,
      kind === "preview" ? [] : names,
      kind === "update" ? "updated" : "installed",
    );
    emit(channel, {
      taskId: `preview-${kind}`,
      phase: "completed",
      message:
        kind === "preview"
          ? `找到 ${previewSummary.skills.length} 个 skill`
          : `${names.length} 个 skill 已完成`,
      completed: kind === "preview" ? 1 : names.length,
      total: kind === "preview" ? 1 : names.length,
      result,
    });
  }, 320);
}

mockIPC(
  (command, payload) => {
    switch (command) {
      case "get_environment":
        return environment;
      case "get_inventory":
        return snapshot;
      case "get_skill_detail": {
        const args = payload as { skillId?: string };
        return skillDetail(args.skillId ?? "agent-browser");
      }
      case "check_updates":
        return updateReport;
      case "start_preview": {
        const args = payload as { source: string; onEvent: MockChannel };
        simulate(args.onEvent, "preview", []);
        return "preview-task";
      }
      case "start_install": {
        const args = payload as {
          request: { skills: string[] };
          onEvent: MockChannel;
        };
        simulate(args.onEvent, "install", args.request.skills);
        return "install-task";
      }
      case "start_update": {
        const args = payload as { skillIds: string[]; onEvent: MockChannel };
        simulate(args.onEvent, "update", args.skillIds);
        return "update-task";
      }
      case "get_active_mutation":
        return null;
      case "reveal_app_data":
      case "open_skill_folder":
      case "open_skill_md":
      case "open_external_url":
      case "dismiss_interrupted_mutation":
      case "quit_app":
        return null;
      case "cancel_task":
        return { taskId: "task", accepted: true, message: "已请求取消" };
      default:
        throw new Error(`preview: unmocked command ${command}`);
    }
  },
  { shouldMockEvents: true },
);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={createQueryClient()}>
      <App />
    </QueryClientProvider>
  </React.StrictMode>,
);

// Dev-only driver for inspecting dialog states.
const params = new URLSearchParams(location.search);
const buttonByText = (text: string) =>
  [...document.querySelectorAll("button")].find((candidate) =>
    candidate.textContent?.trim().startsWith(text),
  );
const clickButton = (text: string) => buttonByText(text)?.click();
const pause = (ms: number) =>
  new Promise<void>((resolve) => window.setTimeout(resolve, ms));

async function driveWizard() {
  clickButton("安装 skill");
  await pause(160);
  const input = document.querySelector<HTMLInputElement>(
    'input[aria-label="仓库来源"]',
  );
  if (!input) return;
  const setter = Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    "value",
  )?.set;
  setter?.call(input, "mattpocock/skills");
  input.dispatchEvent(new Event("input", { bubbles: true }));
  await pause(60);
  clickButton("预览仓库");
  const target = Number(params.get("wizardStep") ?? 1);
  if (target <= 1) return;
  await pause(360);
  if (target === 2) return;
  clickButton("下一步");
  await pause(160);
  if (target === 3) return;
  clickButton("下一步");
  await pause(160);
  if (target === 4) return;
  clickButton("开始安装");
  await pause(500);
}

async function driveUpdate() {
  clickButton("更新全部");
  await pause(400);
  clickButton("开始更新");
}

window.setTimeout(() => {
  if (params.has("check")) clickButton("检查更新");
  window.setTimeout(() => {
    const rows = document.querySelectorAll<HTMLElement>(
      '[data-testid="skill-row"]',
    );
    if (rows.length > 0 && (window.innerWidth > 860 || params.has("selectMobile"))) {
      if (params.has("select") || params.has("selectMobile")) rows[0]!.click();
    }
    if (params.has("scroll")) {
      window.setTimeout(() => {
        const pane = document.querySelector<HTMLElement>(
          '[data-testid="detail-content"]',
        );
        pane?.scrollTo(0, pane.scrollHeight);
      }, 500);
    }
    if (params.has("install")) {
      if (params.has("wizardStep")) void driveWizard();
      else clickButton("安装 skill");
    }
    if (params.has("update")) {
      void driveUpdate();
    }
    if (params.has("filters")) {
      document
        .querySelector<HTMLButtonElement>('button[aria-label="打开筛选"]')
        ?.click();
    }
    if (params.has("anomalies")) {
      document
        .querySelector<HTMLButtonElement>('button[aria-label^="只读提示"]')
        ?.click();
    }
    if (params.has("uninstall") || params.has("detach")) {
      window.setTimeout(() => {
        const target =
          params.has("uninstall")
            ? buttonByText("彻底卸载")
            : document.querySelector<HTMLButtonElement>(
                'button[aria-label^="断开 "]',
              );
        target?.click();
      }, 350);
    }
  }, 400);
}, 400);
