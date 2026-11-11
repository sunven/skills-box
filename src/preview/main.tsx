import { appDataFixture } from "../test/app-data-fixtures";
import { detailForSkill } from "../test/detail-fixtures";
import { discoveryFixture, discoveryProject } from "../test/discovery-fixtures";
import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import App from "../App";
import { createQueryClient } from "../lib/query-client";
import { createFakeBackend } from "../test/fake-backend";
import { initialAppUpdate } from "../lib/app-update";
import { marketplaceSkill } from "../test/marketplace-fixtures";
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

const params = new URLSearchParams(location.search);
const marketplaceMarkdown = params.has("marketplaceLong")
  ? `${marketplaceSkill.markdown}\n\n${[
      ["记录项目决策", "在项目目录中保存决策背景、采用的方案与后续检查事项。每条记录包含日期和关联文件，便于在任务交接时追溯依据。"],
      ["检索已有知识", "先查阅已有笔记与相关代码，再补充新的结论。引用文件时保留相对路径，并说明结论适用的项目范围。"],
      ["整理任务交接", "交接记录说明已完成工作、当前验证结果和剩余事项。将需要人工处理的步骤列出，并关联相关决策。"],
      ["维护笔记目录", "按主题整理笔记，定期合并重复内容。保留仍然有效的历史依据，避免将临时观察写成长期约定。"],
    ].map(([title, text]) => `## ${title}\n\n${text}\n\n1. 查阅当前项目的约定与相关记录。\n2. 保存与本次任务有关的事实和验证结果。\n3. 为后续任务提供明确的文件位置。`).join("\n\n")}\n\n## 命令示例\n\n\`\`\`bash\nnotes search --project workspace --include-decisions --include-handoffs --format markdown --output ./docs/project-notes/search-results.md\n\`\`\`\n\n| 项目记录 | 用途 | 关联信息 |\n| --- | --- | --- |\n| 项目约定 | 当前约束 | 记录来源与生效时间 |\n| 决策记录 | 方案依据 | 关联讨论与验证结果 |\n| 任务交接 | 后续工作 | 提供文件位置和检查步骤 |`
  : marketplaceSkill.markdown;

createFakeBackend({
  commands: params.has("marketplaceLong") ? {
    get_marketplace_skill: ({ coordinate }) => ({ ...marketplaceSkill, coordinate, markdown: marketplaceMarkdown }),
    prepare_marketplace_install: async (args, fake) => {
      const preview = await fake.dispatch("prepare_marketplace_install", args);
      return { ...preview, detail: { ...preview.detail, markdown: marketplaceMarkdown } };
    },
  } : undefined,
  data: {
    appData: appDataFixture(),
    appUpdate: new URLSearchParams(location.search).has("appUpdate") ? initialAppUpdate({
      enabled: true, phase: "available", lastAttemptAt: Date.now(),
      candidate: { version: "0.2.0", notes: "## 更新内容\n\n- 新增应用自动检查和安全更新。\n- 改善安装进度与错误恢复。" },
    }) : initialAppUpdate(),
    discovery: discoveryFixture(),
    projects: { projects: [discoveryProject], lastActiveScope: "global", warnings: [] },
    environment,
    snapshot: new URLSearchParams(location.search).has("staleRecords") ? {
      ...snapshot, skills: snapshot.skills.filter((s) => s.name !== "caveman"), anomalies: { ...snapshot.anomalies, total: snapshot.anomalies.total + 20,
        stale: { count: 21, examples: ["caveman", "design-an-interface", "diagnose"] } },
    } : snapshot,
    staleRecords: new URLSearchParams(location.search).has("staleRecords") ? { global: Array.from({ length: 21 }, (_, i) => {
      const name = ["caveman", "design-an-interface", "diagnose"][i] ?? `missing-skill-${i + 1}`;
      return { id: name, name, source: "mattpocock/skills", reference: null, cleanReason: null, restoreReason: null };
    }) } : undefined,
    detail: (skillId, current) => {
      const installed = current.skills.find((skill) => skill.id === skillId || skill.name === skillId);
      return ["skillhub", "skillhub-pack"].includes(installed?.sourceType ?? "") ? detailForSkill(installed!) : skillDetail(skillId);
    },
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
  if (params.has("marketplace") || params.has("marketplaceLong") || params.has("marketplaceFiles")) {
    clickButton("技能市场");
    await pause(160);
    clickButton(marketplaceSkill.name);
    if (params.has("marketplaceFiles")) {
      await pause(160);
      document.querySelector<HTMLButtonElement>("#marketplace-tab-files")?.click();
    }
    return;
  }
  if (params.has("check")) clickButton("检查更新");
  await pause(400);
  if (params.has("batchUninstall")) {
    document.querySelector<HTMLInputElement>('input[aria-label="选择技能 agent-browser"]')?.click();
    await pause(120);
    clickButton("批量卸载");
    await pause(120);
    if (params.get("batchUninstall") !== "all") {
      document.querySelector<HTMLInputElement>('input[name="batch-uninstall-mode"][value="agents"]')?.click();
      await pause(120);
      document.querySelector<HTMLInputElement>('input[aria-label="选择 Agent Claude Code"]')?.click();
      document.querySelector<HTMLInputElement>('input[aria-label="选择 Agent Gemini CLI"]')?.click();
    }
    return;
  }
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
      ? document.querySelector<HTMLButtonElement>(
          'button[aria-label="彻底卸载"]',
        )
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
