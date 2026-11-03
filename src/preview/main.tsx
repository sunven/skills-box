import { discoveryFixture, discoveryProject } from "../test/discovery-fixtures";
import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import App from "../App";
import { createQueryClient } from "../lib/query-client";
import { createFakeBackend } from "../test/fake-backend";
import { initialAppUpdate } from "../lib/app-update";
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

createFakeBackend({
  data: {
    appUpdate: new URLSearchParams(location.search).has("appUpdate") ? initialAppUpdate({
      enabled: true, phase: "available", lastAttemptAt: Date.now(),
      candidate: { version: "0.2.0", notes: "## 更新内容\n\n- 新增应用自动检查和安全更新。\n- 改善安装进度与错误恢复。" },
    }) : initialAppUpdate(),
    discovery: discoveryFixture(),
    projects: { projects: [discoveryProject], lastActiveScope: "global", warnings: [] },
    environment,
    snapshot,
    detail: (skillId) => skillDetail(skillId),
    listDir: listSkillDir,
    readFile: readSkillFile,
    updateReport,
    preview: new URLSearchParams(location.search).has("discovery") ? {
      source: "acme/skills", parseFailed: false, noSkills: false,
      skills: ["alpha", "beta", "gamma"].map((name) => ({ name, description: `${name} description` })),
    } : previewSummary,
  },
  autoTasks: true,
}).install({ shouldMockEvents: true });

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
