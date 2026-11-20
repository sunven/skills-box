import { StrictMode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import { createFakeBackend, type FakeBackendOptions } from "./test/fake-backend";
import { makeSnapshot } from "./test/inventory-fixtures";
import type { SkillFileContent } from "./lib/types";
import {
  clawhubAliasDetail,
  clawhubAliasSkill,
  clawhubDetail,
  clawhubSkill,
  clawhubSparseSkill,
  clawhubVersionOld,
  clawhubVersionOne,
} from "./test/clawhub-fixtures";
import { skillsShSkill } from "./test/skills-sh-fixtures";

function setup(options: FakeBackendOptions = {}) {
  const backend = createFakeBackend({
    ...options,
    data: { snapshot: makeSnapshot(), ...options.data },
  }).install();
  const client = createQueryClient();
  const view = render(
    <StrictMode>
      <QueryClientProvider client={client}>
        <App />
      </QueryClientProvider>
    </StrictMode>,
  );
  return { backend, client, ...view };
}

async function openClawhub() {
  fireEvent.click(await screen.findByRole("button", { name: "技能市场" }));
  fireEvent.click(await screen.findByRole("button", { name: "ClawHub" }));
  await screen.findByText("来源 · ClawHub");
}

async function openWeatherDetail() {
  await openClawhub();
  fireEvent.click(await screen.findByRole("button", { name: "Weather" }));
  return screen.findByRole("main", { name: "Weather" });
}

const detailMain = (name = "Weather") => screen.getByRole("main", { name });

describe("ClawHub detail", () => {
  it("opens the latest version from a list card with ClawHub identity, metrics, license and an install affordance", async () => {
    const { backend } = setup();
    const main = await openWeatherDetail();
    const detail = within(main);

    expect(detail.getByText("@steipete/weather")).toBeInTheDocument();
    expect(detail.getByText("收录于 ClawHub")).toBeInTheDocument();
    expect(await detail.findByText("发布者：Peter Steinberger")).toBeInTheDocument();
    expect(await detail.findByText("许可证：MIT-0")).toBeInTheDocument();
    expect(detail.getByText("下载量：1,200")).toBeInTheDocument();
    expect(detail.getByLabelText("ClawHub 分类标签")).toHaveTextContent("weather");
    expect(detail.getByLabelText("ClawHub 分类标签")).toHaveTextContent("api");
    expect(detail.getByRole("combobox", { name: "ClawHub 版本" })).toHaveValue(
      clawhubVersionOne,
    );
    expect(
      detail.getByText(`ClawHub 版本 v${clawhubVersionOne} 的打包内容`),
    ).toBeInTheDocument();
    expect(
      detail.getByText(`解析引用：v${clawhubVersionOne}（ClawHub archive 版本；安装按确认时的 latest 版本解析）`),
    ).toBeInTheDocument();
    expect(
      detail.getByText(/不保证与将来安装时重新解析的内容完全一致/),
    ).toBeInTheDocument();
    expect(detail.getByRole("button", { name: "安装" })).toBeEnabled();
    expect(detail.queryByText("ClawHub 安装暂不可用。")).not.toBeInTheDocument();
    expect(detail.queryByText(/scan|扫描/i)).not.toBeInTheDocument();
    expect(
      await detail.findByRole("heading", { name: "Weather 1.2.0" }),
    ).toBeInTheDocument();
    expect(backend.callsOf("get_clawhub_skill_detail")[0].payload.coordinate).toEqual(
      clawhubSkill.coordinate,
    );
    expect(backend.callsOf("list_clawhub_version_files")[0].payload).toEqual({
      coordinate: clawhubSkill.coordinate,
      version: clawhubVersionOne,
    });
    expect(backend.callsOf("read_clawhub_version_file")[0].payload).toEqual({
      coordinate: clawhubSkill.coordinate,
      version: clawhubVersionOne,
      path: "SKILL.md",
    });

    fireEvent.click(detail.getByRole("button", { name: "ClawHub 页面" }));
    expect(backend.callsOf("open_external_url").slice(-1)[0].payload.url).toBe(
      clawhubDetail.canonicalUrl,
    );
  });

  it("installs an archive ClawHub skill through the native flow and attaches it to an agent", async () => {
    const { backend } = setup({ autoTasks: { delayMs: 5 } });
    const main = await openWeatherDetail();
    const detail = within(main);

    fireEvent.click(await detail.findByRole("button", { name: "安装" }));
    const dialog = await screen.findByRole("dialog", { name: "安装 · Weather" });
    fireEvent.click(within(dialog).getByLabelText("Claude Code"));
    fireEvent.click(within(dialog).getByRole("button", { name: "预览安装" }));
    expect(
      await within(dialog).findByText(/由应用原生下载并校验 ClawHub/),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByText(/确认安装 weather · v1\.2\.0/),
    ).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "确认安装" }));
    await waitFor(() =>
      expect(screen.getByTestId("clawhub-result")).toHaveAttribute(
        "data-overall",
        "success",
      ),
    );
    expect(
      backend.callsOf("prepare_clawhub_install")[0].payload.coordinate,
    ).toEqual(clawhubSkill.coordinate);
    expect(backend.callsOf("start_clawhub_install")).toHaveLength(1);

    fireEvent.click(
      within(dialog).getByRole("button", { name: "查看已安装技能" }),
    );
    expect(
      await screen.findByRole("heading", { name: "weather" }),
    ).toBeInTheDocument();
  });

  it("opens a search result and returns to the same query, scroll and list without refetching it", async () => {
    const { backend } = setup();
    await openClawhub();
    await screen.findByText("Weather");
    fireEvent.change(screen.getByLabelText("搜索 ClawHub"), {
      target: { value: "weather" },
    });
    fireEvent.click(screen.getByRole("button", { name: "搜索" }));
    await screen.findByText(/搜索「weather」 · 返回 2 项/);
    const list = screen.getByRole("main", { name: "技能市场" });
    fireEvent.scroll(list, { target: { scrollTop: 160 } });

    fireEvent.click(screen.getByRole("button", { name: "Weather" }));
    const main = await screen.findByRole("main", { name: "Weather" });
    await within(main).findByRole("heading", { name: "Weather 1.2.0" });
    expect(list).not.toBeVisible();
    const searches = backend.count("search_clawhub_skills");

    fireEvent.click(within(screen.getByRole("toolbar", { name: "窗口标题栏" })).getByRole("button", { name: "返回列表" }));
    await waitFor(() =>
      expect(screen.queryByRole("main", { name: "Weather" })).not.toBeInTheDocument(),
    );
    expect(screen.getByLabelText("搜索 ClawHub")).toHaveValue("weather");
    expect(screen.getByText(/搜索「weather」 · 返回 2 项/)).toBeInTheDocument();
    expect(list.scrollTop).toBe(160);
    expect(backend.count("search_clawhub_skills")).toBe(searches);
  });

  it("keeps SKILL.md, changelog and the file tree on the selected version when switching", async () => {
    const { backend } = setup();
    const main = await openWeatherDetail();
    const detail = within(main);
    await detail.findByRole("heading", { name: "Weather 1.2.0" });
    fireEvent.click(detail.getByRole("tab", { name: "文件" }));
    const panel = detail.getByRole("tabpanel", { name: "文件" });
    await within(panel).findByRole("button", { name: /SKILL.md/ });
    fireEvent.click(within(panel).getByRole("button", { name: "references" }));
    fireEvent.click(within(panel).getByRole("button", { name: /guide.md/ }));
    expect(
      await within(panel).findByRole("heading", { name: "天气指南 1.2.0" }),
    ).toBeInTheDocument();

    fireEvent.change(detail.getByRole("combobox", { name: "ClawHub 版本" }), {
      target: { value: clawhubVersionOld },
    });
    await detail.findByText(`ClawHub 版本 v${clawhubVersionOld} 的打包内容`);
    expect(
      detail.queryByText(`ClawHub 版本 v${clawhubVersionOne} 的打包内容`),
    ).not.toBeInTheDocument();
    expect(
      detail.queryByRole("heading", { name: "Weather 1.2.0" }),
    ).not.toBeInTheDocument();
    expect(detail.queryByText("天气指南 1.2.0")).not.toBeInTheDocument();
    expect(
      await detail.findByRole("heading", { name: "Weather 1.0.0" }),
    ).toBeInTheDocument();
    expect(detail.getByText("1.0.0：首个公开版本。")).toBeInTheDocument();
    // License and changelog follow the selected version, never latest.
    expect(detail.getByText("许可证：MIT")).toBeInTheDocument();
    expect(detail.queryByText("许可证：MIT-0")).not.toBeInTheDocument();
    expect(
      detail.getByText(`解析引用：v${clawhubVersionOld}（ClawHub archive 版本；安装按确认时的 latest 版本解析）`),
    ).toBeInTheDocument();

    fireEvent.click(detail.getByRole("tab", { name: "文件" }));
    const oldPanel = detail.getByRole("tabpanel", { name: "文件" });
    fireEvent.click(await within(oldPanel).findByRole("button", { name: "references" }));
    fireEvent.click(within(oldPanel).getByRole("button", { name: /guide.md/ }));
    expect(
      await within(oldPanel).findByRole("heading", { name: "天气指南 1.0.0" }),
    ).toBeInTheDocument();
    expect(
      backend
        .callsOf("list_clawhub_version_files")
        .every((call) =>
          [clawhubVersionOne, clawhubVersionOld].includes(call.payload.version),
        ),
    ).toBe(true);
    expect(
      backend
        .callsOf("read_clawhub_version_file")
        .filter((call) => call.payload.path === "SKILL.md")
        .map((call) => call.payload.version),
    ).toContain(clawhubVersionOld);
  });

  it("never labels a delayed old-version response as the newly selected version", async () => {
    let releaseOld: ((file: SkillFileContent) => void) | undefined;
    const { backend } = setup({
      commands: {
        read_clawhub_version_file: (args, fake) => {
          if (args.version === clawhubVersionOld && args.path === "SKILL.md") {
            return new Promise((resolve) => {
              releaseOld = resolve;
            });
          }
          return fake.dispatch("read_clawhub_version_file", args);
        },
      },
    });
    const main = await openWeatherDetail();
    const detail = within(main);
    await detail.findByRole("heading", { name: "Weather 1.2.0" });

    fireEvent.change(detail.getByRole("combobox", { name: "ClawHub 版本" }), {
      target: { value: clawhubVersionOld },
    });
    await detail.findByText(`ClawHub 版本 v${clawhubVersionOld} 的打包内容`);
    await detail.findByText("正在读取 SKILL.md…");
    // While the old version is still loading, nothing from the previous
    // version may remain under the old version's label.
    expect(detail.queryByRole("heading", { name: "Weather 1.2.0" })).not.toBeInTheDocument();
    expect(
      detail.queryByText(`ClawHub 版本 v${clawhubVersionOne} 的打包内容`),
    ).not.toBeInTheDocument();

    fireEvent.change(detail.getByRole("combobox", { name: "ClawHub 版本" }), {
      target: { value: clawhubVersionOne },
    });
    await detail.findByText(`ClawHub 版本 v${clawhubVersionOne} 的打包内容`);
    await detail.findByRole("heading", { name: "Weather 1.2.0" });

    const old = await backend.dispatch("read_clawhub_version_file", {
      coordinate: clawhubSkill.coordinate,
      version: clawhubVersionOld,
      path: "SKILL.md",
    });
    await act(async () => {
      releaseOld!({ ...old, content: "# 旧版延迟内容" });
    });
    expect(detail.queryByText("旧版延迟内容")).not.toBeInTheDocument();
    expect(detail.getByRole("heading", { name: "Weather 1.2.0" })).toBeInTheDocument();
    expect(
      detail.getByText(`ClawHub 版本 v${clawhubVersionOne} 的打包内容`),
    ).toBeInTheDocument();

    fireEvent.change(detail.getByRole("combobox", { name: "ClawHub 版本" }), {
      target: { value: clawhubVersionOld },
    });
    expect(
      await detail.findByRole("heading", { name: "旧版延迟内容" }),
    ).toBeInTheDocument();
    expect(
      detail.getByText(`ClawHub 版本 v${clawhubVersionOld} 的打包内容`),
    ).toBeInTheDocument();
  });

  it("previews markdown, text, images and marks binary or oversized files, and keeps missing files explicit", async () => {
    const { backend } = setup();
    const main = await openWeatherDetail();
    const detail = within(main);
    await detail.findByRole("heading", { name: "Weather 1.2.0" });
    fireEvent.click(detail.getByRole("tab", { name: "文件" }));
    const panel = detail.getByRole("tabpanel", { name: "文件" });
    await within(panel).findByRole("button", { name: /SKILL.md/ });
    expect(await within(panel).findByTestId("skill-markdown-rendered")).toHaveTextContent(
      "Weather 1.2.0",
    );
    fireEvent.click(within(panel).getByRole("button", { name: "查看原文" }));
    expect(within(panel).getByLabelText("SKILL.md 原文")).toHaveTextContent("name: weather");
    fireEvent.click(within(panel).getByRole("button", { name: "返回渲染" }));

    fireEvent.click(within(panel).getByRole("link", { name: "阅读指南" }));
    expect(
      await within(panel).findByRole("heading", { name: "天气指南 1.2.0" }),
    ).toBeInTheDocument();
    fireEvent.click(within(panel).getByRole("link", { name: "查看脚本" }));
    expect(await within(panel).findByLabelText("scripts/forecast.py 原文")).toHaveTextContent(
      "def forecast",
    );

    fireEvent.click(within(panel).getByRole("button", { name: /large.txt/ }));
    expect(
      await within(panel).findByText(/文件超过 1 MiB 预览上限，当前大小 2 MiB，仅显示元数据/),
    ).toBeInTheDocument();
    fireEvent.click(within(panel).getByRole("button", { name: "assets" }));
    fireEvent.click(within(panel).getByRole("button", { name: /data.bin/ }));
    expect(
      await within(panel).findByText(/二进制或非 UTF-8 文本/),
    ).toBeInTheDocument();
    fireEvent.click(within(panel).getByRole("button", { name: /preview.png/ }));
    expect(await within(panel).findByRole("img", { name: "preview.png" })).toHaveAttribute(
      "src",
      expect.stringContaining("data:image/png;base64,"),
    );

    fireEvent.click(within(panel).getByRole("button", { name: /SKILL.md/ }));
    fireEvent.click(await within(panel).findByRole("link", { name: "缺失文件" }));
    expect(await within(panel).findByText(/此版本中没有该文件/)).toBeInTheDocument();
    fireEvent.click(within(panel).getByRole("button", { name: "ClawHub 页面" }));
    expect(backend.callsOf("open_external_url").slice(-1)[0].payload.url).toBe(
      clawhubDetail.canonicalUrl,
    );
    expect(
      backend
        .callsOf("read_clawhub_version_file")
        .some((call) => call.payload.path === "references/missing.md"),
    ).toBe(false);
    expect(
      backend
        .callsOf("read_clawhub_version_file")
        .every((call) => call.payload.version === clawhubVersionOne),
    ).toBe(true);
  });

  it("shows the canonical link and retry when one file preview fails to load", async () => {
    let broken = true;
    const { backend } = setup({
      commands: {
        read_clawhub_version_file: (args, fake) => {
          if (broken && args.path === "references/guide.md") {
            throw {
              code: "marketplace.unavailable",
              message: "ClawHub 文件读取失败。",
              detail: "上游返回 HTTP 503",
              retryable: true,
            };
          }
          return fake.dispatch("read_clawhub_version_file", args);
        },
      },
    });
    const main = await openWeatherDetail();
    const detail = within(main);
    await detail.findByRole("heading", { name: "Weather 1.2.0" });
    fireEvent.click(detail.getByRole("tab", { name: "文件" }));
    const panel = detail.getByRole("tabpanel", { name: "文件" });
    fireEvent.click(await within(panel).findByRole("button", { name: "references" }));
    fireEvent.click(within(panel).getByRole("button", { name: /guide.md/ }));

    expect(await within(panel).findByText("无法读取该文件。")).toBeInTheDocument();
    expect(within(panel).getByText("ClawHub 文件读取失败。")).toBeInTheDocument();
    expect(within(panel).getByText("上游返回 HTTP 503")).toBeInTheDocument();
    fireEvent.click(within(panel).getByRole("button", { name: "ClawHub 页面" }));
    expect(backend.callsOf("open_external_url").slice(-1)[0].payload.url).toBe(
      clawhubDetail.canonicalUrl,
    );

    broken = false;
    fireEvent.click(within(panel).getByRole("button", { name: "重试读取文件" }));
    expect(
      await within(panel).findByRole("heading", { name: "天气指南 1.2.0" }),
    ).toBeInTheDocument();
  });

  it("shows explicit missing detail and missing version states with retry and the canonical link", async () => {
    let detailBroken = true;
    let oldBroken = true;
    const { backend } = setup({
      commands: {
        get_clawhub_skill_detail: (args, fake) => {
          if (detailBroken) {
            throw {
              code: "marketplace.unavailable",
              message: "ClawHub 条目不存在或不可公开访问。",
              detail: "上游返回 HTTP 404",
              retryable: false,
            };
          }
          return fake.dispatch("get_clawhub_skill_detail", args);
        },
        list_clawhub_version_files: (args, fake) => {
          if (args.version === clawhubVersionOld && oldBroken) {
            throw {
              code: "marketplace.unavailable",
              message: `ClawHub 版本 v${clawhubVersionOld} 不可用。`,
              detail: "上游返回 HTTP 404",
              retryable: false,
            };
          }
          return fake.dispatch("list_clawhub_version_files", args);
        },
      },
    });
    await openClawhub();
    fireEvent.click(await screen.findByRole("button", { name: "Weather" }));
    const main = await screen.findByRole("main", { name: "Weather" });
    const detail = within(main);

    expect(await detail.findByText("无法读取 ClawHub 详情。")).toBeInTheDocument();
    expect(detail.getByText("ClawHub 条目不存在或不可公开访问。")).toBeInTheDocument();
    expect(detail.getByText("上游返回 HTTP 404")).toBeInTheDocument();
    expect(detail.queryByRole("combobox", { name: "ClawHub 版本" })).not.toBeInTheDocument();
    fireEvent.click(
      within(detail.getByRole("alert")).getByRole("button", { name: "ClawHub 页面" }),
    );
    expect(backend.callsOf("open_external_url").slice(-1)[0].payload.url).toBe(
      clawhubSkill.officialUrl,
    );

    detailBroken = false;
    fireEvent.click(detail.getByRole("button", { name: "重试详情" }));
    await detail.findByRole("heading", { name: "Weather 1.2.0" });

    fireEvent.change(detail.getByRole("combobox", { name: "ClawHub 版本" }), {
      target: { value: clawhubVersionOld },
    });
    expect(await detail.findByText("无法读取该版本内容。")).toBeInTheDocument();
    expect(
      detail.getByText(`ClawHub 版本 v${clawhubVersionOld} 不可用。`),
    ).toBeInTheDocument();
    expect(detail.queryByRole("heading", { name: "Weather 1.2.0" })).not.toBeInTheDocument();
    expect(detail.queryByRole("heading", { name: "Weather 1.0.0" })).not.toBeInTheDocument();
    fireEvent.click(
      within(detail.getByRole("alert")).getByRole("button", { name: "ClawHub 页面" }),
    );
    expect(backend.callsOf("open_external_url").slice(-1)[0].payload.url).toBe(
      clawhubDetail.canonicalUrl,
    );

    oldBroken = false;
    fireEvent.click(detail.getByRole("button", { name: "重试版本内容" }));
    expect(await detail.findByRole("heading", { name: "Weather 1.0.0" })).toBeInTheDocument();
  });

  it("states explicitly when a version has no SKILL.md or no files at all", async () => {
    setup({
      commands: {
        list_clawhub_version_files: (args, fake) =>
          args.version === clawhubVersionOld
            ? {
                coordinate: args.coordinate,
                version: args.version,
                license: null,
                changelog: null,
                files: [],
              }
            : fake.dispatch("list_clawhub_version_files", args),
      },
    });
    const main = await openWeatherDetail();
    const detail = within(main);
    await detail.findByRole("heading", { name: "Weather 1.2.0" });
    fireEvent.change(detail.getByRole("combobox", { name: "ClawHub 版本" }), {
      target: { value: clawhubVersionOld },
    });
    expect(
      await detail.findByText("该版本未提供 SKILL.md，可在「文件」中查看该版本的其他文件。"),
    ).toBeInTheDocument();
    fireEvent.click(detail.getByRole("tab", { name: "文件" }));
    expect(await detail.findByText("此版本没有可浏览的文件。")).toBeInTheDocument();
    // SKILL.md may be prefetched in parallel with the file list; the empty
    // listing still wins and no overview content is rendered.
    expect(
      detail.queryByRole("heading", { name: "Weather 1.0.0" }),
    ).not.toBeInTheDocument();
  });

  it("keeps the last successful same-version content and marks it stale when a refresh fails", async () => {
    let refreshBroken = false;
    const { backend } = setup({
      commands: {
        list_clawhub_version_files: (args, fake) => {
          if (refreshBroken && args.version === clawhubVersionOne) {
            throw {
              code: "marketplace.unavailable",
              message: "ClawHub 版本文件刷新失败。",
              detail: "上游返回 HTTP 503",
              retryable: true,
            };
          }
          return fake.dispatch("list_clawhub_version_files", args);
        },
        read_clawhub_version_file: (args, fake) => {
          if (refreshBroken && args.version === clawhubVersionOne && args.path === "SKILL.md") {
            throw {
              code: "marketplace.unavailable",
              message: "SKILL.md 刷新失败。",
              detail: "上游返回 HTTP 503",
              retryable: true,
            };
          }
          return fake.dispatch("read_clawhub_version_file", args);
        },
      },
    });
    const main = await openWeatherDetail();
    const detail = within(main);
    await detail.findByRole("heading", { name: "Weather 1.2.0" });
    fireEvent.click(detail.getByRole("tab", { name: "文件" }));
    await within(detail.getByRole("tabpanel", { name: "文件" })).findByRole("button", {
      name: /SKILL.md/,
    });
    fireEvent.click(detail.getByRole("tab", { name: "概述" }));

    refreshBroken = true;
    fireEvent.click(detail.getByRole("button", { name: "刷新版本内容" }));

    expect(await detail.findByText("文件列表刷新失败，保留上次内容。")).toBeInTheDocument();
    expect(await detail.findByText("SKILL.md 刷新失败，保留上次内容。")).toBeInTheDocument();
    expect(detail.getByRole("heading", { name: "Weather 1.2.0" })).toBeInTheDocument();
    expect(detail.queryByText("无法读取该版本内容。")).not.toBeInTheDocument();
    expect(detail.getByText(`ClawHub 版本 v${clawhubVersionOne} 的打包内容`)).toBeInTheDocument();
    expect(detail.queryByText(/Weather 1\.0\.0/)).not.toBeInTheDocument();
    expect(
      backend
        .callsOf("list_clawhub_version_files")
        .filter((call) => call.payload.version === clawhubVersionOne).length,
    ).toBeGreaterThan(1);

    fireEvent.click(detail.getByRole("tab", { name: "文件" }));
    const filesPanel = detail.getByRole("tabpanel", { name: "文件" });
    expect(
      within(filesPanel).getByText("文件列表刷新失败，保留上次内容。"),
    ).toBeInTheDocument();
    expect(within(filesPanel).getByRole("button", { name: /SKILL.md/ })).toBeInTheDocument();
    expect(within(filesPanel).queryByText("无法读取该版本内容。")).not.toBeInTheDocument();
  });

  it("identifies a skills.sh alias and redirects to the skills.sh source without an install affordance", async () => {
    const { backend } = setup({
      commands: {
        list_clawhub_skills: () => ({ skills: [clawhubAliasSkill], nextCursor: null }),
      },
    });
    await openClawhub();
    fireEvent.click(await screen.findByRole("button", { name: "debug" }));
    expect(
      await screen.findByText(
        "该条目是 ClawHub 索引的 skills.sh 别名，不属于 ClawHub 原生技能。",
      ),
    ).toBeInTheDocument();
    const detail = within(detailMain("debug"));

    expect(detail.getByText(/内容浏览与安装请在 skills.sh 来源中进行/)).toBeInTheDocument();
    expect(detail.getByText(/对应仓库：anthropics\/knowledge-work-plugins/)).toBeInTheDocument();
    expect(detail.queryByRole("button", { name: "安装" })).not.toBeInTheDocument();
    expect(detail.queryByRole("combobox", { name: "ClawHub 版本" })).not.toBeInTheDocument();
    expect(detail.queryByText(/ClawHub 版本 v/)).not.toBeInTheDocument();

    fireEvent.click(detail.getByRole("button", { name: "skills.sh 页面" }));
    expect(backend.callsOf("open_external_url").slice(-1)[0].payload.url).toBe(
      clawhubAliasDetail.sourceUrl,
    );

    fireEvent.click(detail.getByRole("button", { name: "切换到 skills.sh 来源" }));
    expect(screen.getByRole("button", { name: "ClawHub" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(screen.getByRole("button", { name: "skills.sh" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    const skillsShMain = await screen.findByRole("main", { name: "技能市场" });
    expect(within(skillsShMain).getByLabelText("搜索 skills.sh")).toBeVisible();
    expect(
      await within(skillsShMain).findByRole("button", { name: skillsShSkill.name }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("main", { name: "debug" })).not.toBeInTheDocument();
    expect(
      screen.getByText("该条目是 ClawHub 索引的 skills.sh 别名，不属于 ClawHub 原生技能。"),
    ).not.toBeVisible();
  });

  it("states explicitly that a GitHub-backed entry has no ClawHub versions or files", async () => {
    const { backend } = setup({
      commands: {
        get_clawhub_skill_detail: (args, fake) =>
          args.coordinate.namespace === clawhubSparseSkill.coordinate.namespace
            ? {
                kind: "native",
                coordinate: args.coordinate,
                name: clawhubSparseSkill.name,
                description: clawhubSparseSkill.description,
                topics: clawhubSparseSkill.topics,
                metrics: clawhubSparseSkill.metrics,
                ownerDisplayName: null,
                latestVersion: clawhubSparseSkill.latestVersion,
                license: null,
                changelog: null,
                canonicalUrl: clawhubSparseSkill.officialUrl,
                versions: [],
                versionsTruncated: false,
                resolvedReference: {
                  repo: "aws/agent-toolkit-for-aws",
                  path: "plugins/aws-agents/skills/agents-build",
                  commit: "b4416dddac9b6a0cc5412136e6dbf8f07ffdb31c",
                  contentHash:
                    "d79709bf46ca8cd0fffe392a914cd56539b4b7c0ff0fe34c9583f24057300195",
                },
              }
            : fake.dispatch("get_clawhub_skill_detail", args),
      },
    });
    await openClawhub();
    fireEvent.click(
      await screen.findByRole("button", { name: clawhubSparseSkill.coordinate.slug }),
    );
    expect(
      await screen.findByText(
        "该条目由 GitHub 仓库提供，ClawHub 未提供可浏览的版本打包内容。",
      ),
    ).toBeInTheDocument();
    const detail = within(detailMain(clawhubSparseSkill.coordinate.slug));

    expect(
      detail.getByText(
        "安装会固定解析出的 commit 并校验文件夹内容哈希；解析引用不会创建 Git 安装身份。",
      ),
    ).toBeInTheDocument();
    expect(
      detail.getByText(
        /解析引用：aws\/agent-toolkit-for-aws@b4416dddac9b6a0cc5412136e6dbf8f07ffdb31c:plugins\/aws-agents\/skills\/agents-build/,
      ),
    ).toBeInTheDocument();
    expect(detail.queryByText(/许可证/)).not.toBeInTheDocument();
    const metrics = detail.getByRole("group", { name: "ClawHub 指标" });
    expect(within(metrics).getAllByText(/未提供/)).toHaveLength(3);
    expect(detail.queryByRole("combobox", { name: "ClawHub 版本" })).not.toBeInTheDocument();
    expect(detail.queryByText(/ClawHub 版本 v/)).not.toBeInTheDocument();
    expect(detail.getByRole("button", { name: "安装" })).toBeEnabled();
    expect(backend.count("list_clawhub_version_files")).toBe(0);
    expect(backend.count("read_clawhub_version_file")).toBe(0);

    fireEvent.click(detail.getByRole("button", { name: "安装" }));
    const dialog = await screen.findByRole("dialog", {
      name: `安装 · ${clawhubSparseSkill.coordinate.slug}`,
    });
    expect(dialog).toHaveTextContent(
      /aws\/agent-toolkit-for-aws@b4416dddac9b6a0cc5412136e6dbf8f07ffdb31c:plugins\/aws-agents\/skills\/agents-build/,
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "关闭安装" }));

    const links = detail.getAllByRole("button", { name: "ClawHub 页面" });
    fireEvent.click(links[links.length - 1]);
    expect(backend.callsOf("open_external_url").slice(-1)[0].payload.url).toBe(
      clawhubSparseSkill.officialUrl,
    );
  });
});
