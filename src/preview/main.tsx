import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import App from "../App";
import { createQueryClient } from "../lib/query-client";
import type { TaskEvent, TaskResult } from "../lib/types";
import {
  environment,
  listSkillDir,
  previewSummary,
  readSkillFile,
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
      case "list_skill_dir": {
        const args = payload as { skillId?: string; path?: string };
        return listSkillDir(
          args.skillId ?? "agent-browser",
          args.path ?? "",
        );
      }
      case "read_skill_file": {
        const args = payload as { skillId?: string; path: string };
        return readSkillFile(args.skillId ?? "agent-browser", args.path);
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
      case "attach_agent": {
        const args = payload as { skillId: string; agentId: string };
        return {
          skillId: args.skillId,
          agentId: args.agentId,
          agentName: args.agentId === "workbuddy" ? "WorkBuddy" : args.agentId,
          attachmentPath: `~/${args.agentId}/skills/${args.skillId}`,
          canonicalPath: `~/.agents/skills/${args.skillId}`,
          logPath: "~/Library/Application Support/com.sunven.skills-box/logs/preview.log",
        };
      }
      case "add_extra_agent": {
        const args = payload as {
          agent: { id: string; displayName: string; skillsDir: string };
        };
        return { ...args.agent, builtin: false };
      }
      case "remove_extra_agent": {
        const args = payload as { agentId: string };
        return {
          id: args.agentId,
          displayName: "Acme Agent",
          detached: ["agent-browser"],
        };
      }
      case "reveal_app_data":
      case "open_skill_folder":
      case "open_skill_file":
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

async function selectDetailTab(label: "内容" | "操作") {
  const tab = [
    ...document.querySelectorAll<HTMLButtonElement>('[role="tab"]'),
  ].find((candidate) => candidate.textContent?.trim() === label);
  tab?.click();
  await pause(120);
}

async function driveContent() {
  const file = params.get("file");
  if (!file) return;
  const parts = file.split("/").filter(Boolean);
  for (const part of parts.slice(0, -1)) {
    const dir = [...document.querySelectorAll<HTMLElement>(".tree-row")].find(
      (candidate) => candidate.textContent?.includes(part),
    );
    dir?.click();
    await pause(180);
  }
  const last = parts[parts.length - 1] ?? "";
  const target = [...document.querySelectorAll<HTMLElement>(".tree-row")].find(
    (candidate) => candidate.textContent?.includes(last),
  );
  target?.click();
  await pause(180);
}

window.setTimeout(async () => {
  if (params.has("check")) clickButton("检查更新");
  await pause(400);
  const rows = document.querySelectorAll<HTMLElement>(
    '[data-testid="skill-row"]',
  );
  if (rows.length > 0 && (window.innerWidth > 860 || params.has("selectMobile"))) {
    if (params.has("select") || params.has("selectMobile")) rows[0]!.click();
  }
  await pause(300);
  if (params.has("file")) {
    await selectDetailTab("内容");
    await driveContent();
  }
  if (params.get("tab") === "actions") await selectDetailTab("操作");
  if (params.has("scroll")) {
    const pane =
      params.get("tab") === "actions"
        ? document.querySelector<HTMLElement>(
            '[data-testid="detail-actions-panel"]',
          )
        : document.querySelector<HTMLElement>(".content-preview-body");
    pane?.scrollTo(0, pane.scrollHeight);
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
    await pause(350);
    await selectDetailTab("操作");
    const target = params.has("uninstall")
      ? buttonByText("彻底卸载")
      : document.querySelector<HTMLButtonElement>(
          'button[aria-label^="断开 "]',
        );
    target?.click();
  }
  if (params.has("agents")) {
    document
      .querySelector<HTMLButtonElement>('button[aria-label="管理 agent"]')
      ?.click();
  }
  if (params.has("attach")) {
    await pause(350);
    await selectDetailTab("操作");
    document
      .querySelector<HTMLButtonElement>('[data-testid="detail-attach"]')
      ?.click();
  }
}, 400);
