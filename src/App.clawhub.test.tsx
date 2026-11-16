import { StrictMode } from "react";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import { createFakeBackend, type FakeBackendOptions } from "./test/fake-backend";
import { makeSnapshot } from "./test/inventory-fixtures";
import { marketplaceSkill } from "./test/marketplace-fixtures";
import { skillsShSkill } from "./test/skills-sh-fixtures";
import {
  clawhubPageOne,
  clawhubPageTwo,
  clawhubSkill,
} from "./test/clawhub-fixtures";

function setup(options: FakeBackendOptions = {}) {
  const backend = createFakeBackend({ ...options, data: { snapshot: makeSnapshot(), ...options.data } }).install();
  const client = createQueryClient();
  const view = render(<StrictMode><QueryClientProvider client={client}><App /></QueryClientProvider></StrictMode>);
  return { backend, client, ...view };
}
async function openMarket() {
  fireEvent.click(await screen.findByRole("button", { name: "技能市场" }));
  await screen.findByText("来源 · SkillHub");
}
async function openClawhub() {
  await openMarket();
  fireEvent.click(screen.getByRole("button", { name: "ClawHub" }));
  await screen.findByText("来源 · ClawHub");
}
const cardTitles = () =>
  screen.getAllByRole("article").map((card) => within(card).getByRole("heading").textContent);
const cardFor = (title: string) =>
  screen.getAllByRole("article").find((card) => within(card).queryByRole("heading", { name: title }))!;

describe("ClawHub marketplace", () => {
  it("defaults to SkillHub and restores ClawHub from the remembered source after restart", async () => {
    const { backend, unmount } = setup();
    await openMarket();
    expect(screen.getByRole("button", { name: "ClawHub" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: "SkillHub" })).toHaveAttribute("aria-pressed", "true");
    expect(backend.count("list_clawhub_skills")).toBe(0);

    fireEvent.click(screen.getByRole("button", { name: "ClawHub" }));
    await screen.findByText("Weather");
    expect(localStorage.getItem("skills-box.marketplace-source")).toBe("clawhub");
    const calls = backend.count("list_clawhub_skills");

    unmount();
    const client = createQueryClient();
    render(<StrictMode><QueryClientProvider client={client}><App /></QueryClientProvider></StrictMode>);
    fireEvent.click(await screen.findByRole("button", { name: "技能市场" }));
    await screen.findByText("Weather");
    expect(screen.getByRole("button", { name: "ClawHub" })).toHaveAttribute("aria-pressed", "true");
    expect(backend.count("list_clawhub_skills")).toBeGreaterThan(calls);
  });

  it("keeps per-source session state and never lets the expert-pack tab rewrite the source", async () => {
    const { backend } = setup();
    await openMarket();
    fireEvent.change(screen.getByLabelText("搜索 SkillHub"), { target: { value: "hub draft" } });
    const hubList = screen.getByRole("main", { name: "技能市场" });
    fireEvent.scroll(hubList, { target: { scrollTop: 100 } });

    fireEvent.click(screen.getByRole("button", { name: "skills.sh" }));
    await screen.findByRole("button", { name: skillsShSkill.name });
    fireEvent.click(screen.getByRole("button", { name: "ClawHub" }));
    await screen.findByText("Weather");
    fireEvent.click(screen.getByRole("button", { name: "下载量" }));
    await waitFor(() => expect(backend.callsOf("list_clawhub_skills").slice(-1)[0]!.payload.sort).toBe("downloads"));
    const clawList = screen.getByRole("main", { name: "技能市场" });
    fireEvent.scroll(clawList, { target: { scrollTop: 320 } });

    fireEvent.click(screen.getByRole("button", { name: "专家包" }));
    expect(within(screen.getByRole("group", { name: "市场类型" })).getByText("来源 · SkillHub")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "技能" }));
    expect(screen.getByRole("button", { name: "ClawHub" })).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(screen.getByRole("button", { name: "SkillHub" }));
    expect(screen.getByLabelText("搜索 SkillHub")).toHaveValue("hub draft");
    expect(hubList.scrollTop).toBe(100);
    expect(screen.getByRole("button", { name: "下载量" })).toHaveAttribute("aria-pressed", "false");

    fireEvent.click(screen.getByRole("button", { name: "ClawHub" }));
    expect(screen.getByRole("button", { name: "下载量" })).toHaveAttribute("aria-pressed", "true");
    expect(clawList.scrollTop).toBe(320);
    expect(localStorage.getItem("skills-box.marketplace-source")).toBe("clawhub");
  });

  it("exposes exactly the three documented sorts and requests them in upstream order", async () => {
    const { backend } = setup({ commands: {
      list_clawhub_skills: ({ sort }) => ({
        skills: [{ ...clawhubSkill, name: `sort-${sort}` }],
        nextCursor: null,
      }),
    } });
    await openClawhub();
    const group = screen.getByRole("group", { name: "ClawHub 排序" });
    expect(within(group).getAllByRole("button").map((button) => button.textContent)).toEqual([
      "推荐",
      "最近更新",
      "下载量",
    ]);
    await screen.findByText("sort-recommended");
    expect(within(group).getByRole("button", { name: "推荐" })).toHaveAttribute("aria-pressed", "true");
    expect(backend.callsOf("list_clawhub_skills")[0]!.payload).toEqual({ sort: "recommended", cursor: null });

    fireEvent.click(within(group).getByRole("button", { name: "最近更新" }));
    await screen.findByText("sort-updated");
    expect(backend.callsOf("list_clawhub_skills").slice(-1)[0]!.payload).toEqual({ sort: "updated", cursor: null });

    fireEvent.click(within(group).getByRole("button", { name: "下载量" }));
    await screen.findByText("sort-downloads");
    expect(backend.callsOf("list_clawhub_skills").slice(-1)[0]!.payload).toEqual({ sort: "downloads", cursor: null });
    expect(screen.queryByText("sort-recommended")).not.toBeInTheDocument();
  });

  it("appends the next cursor page without duplicates or reordering and shows no fake totals", async () => {
    const { backend } = setup();
    await openClawhub();
    await screen.findByText("Weather");
    const list = screen.getByRole("main", { name: "技能市场" });
    expect(cardTitles()).toEqual(["Weather", "agents-build"]);
    expect(screen.getByText(/推荐 · 已加载 2 项/)).toBeInTheDocument();
    expect(within(list).queryByText(/共\s*[\d,]+\s*项/)).not.toBeInTheDocument();
    expect(within(list).queryByText(/第\s*\d+\s*页/)).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "加载更多" }));
    await screen.findByText("Browser");
    expect(cardTitles()).toEqual(["Weather", "agents-build", "Browser"]);
    expect(backend.callsOf("list_clawhub_skills").slice(-1)[0]!.payload).toEqual({
      sort: "recommended",
      cursor: "cursor-2",
    });
    expect(screen.queryByRole("button", { name: "加载更多" })).not.toBeInTheDocument();
    expect(screen.getByText(/推荐 · 已加载 3 项/)).toBeInTheDocument();
  });

  it("renders available ClawHub fields and marks missing metrics as not provided instead of zero", async () => {
    setup();
    await openClawhub();
    await screen.findByText("Weather");
    const full = cardFor("Weather");
    expect(within(full).getByText("@steipete/weather")).toBeInTheDocument();
    expect(within(full).getByText("天气预报与本地化出行建议。")).toBeInTheDocument();
    expect(within(full).getByText("官方")).toBeInTheDocument();
    expect(within(full).getByText("下载量：1,200")).toBeInTheDocument();
    expect(within(full).getByText("安装量：34")).toBeInTheDocument();
    expect(within(full).getByText("星标：5")).toBeInTheDocument();

    const sparse = cardFor("agents-build");
    expect(within(sparse).getByText("@aws/agents-build")).toBeInTheDocument();
    expect(within(sparse).getAllByText(/未提供/)).toHaveLength(3);
    expect(within(sparse).queryByText("官方")).not.toBeInTheDocument();
    expect(within(sparse).queryByText("下载量：0")).not.toBeInTheDocument();
  });

  it("retains the last successful list under its old sort and retries without switching sources", async () => {
    let broken = true;
    const { backend } = setup({ commands: {
      list_clawhub_skills: ({ sort }) => {
        if (sort === "downloads" && broken) {
          throw { code: "marketplace.unavailable", message: "无法读取 ClawHub 目录。", detail: "HTTP 503" };
        }
        return { skills: [{ ...clawhubSkill, name: `${sort}-page` }], nextCursor: null };
      },
    } });
    await openClawhub();
    await screen.findByText("recommended-page");
    fireEvent.click(screen.getByRole("button", { name: "下载量" }));
    await screen.findByText(/保留上次成功结果：推荐/);
    expect(screen.getByText("recommended-page")).toBeInTheDocument();
    expect(screen.getByText("HTTP 503")).toBeInTheDocument();
    expect(screen.queryByText("downloads-page")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: marketplaceSkill.name })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "ClawHub 官网" }));
    expect(backend.callsOf("open_external_url").slice(-1)[0]!.payload.url).toBe("https://clawhub.ai/skills");

    broken = false;
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    await screen.findByText("downloads-page");
    expect(screen.queryByText(/保留上次成功结果/)).not.toBeInTheDocument();
  });

  it("keeps the first page when loading more fails", async () => {
    let broken = false;
    setup({ commands: {
      list_clawhub_skills: ({ cursor }) => {
        if (cursor && broken) {
          throw { code: "marketplace.unavailable", message: "无法读取 ClawHub 目录。", detail: "HTTP 502" };
        }
        return cursor ? clawhubPageTwo : clawhubPageOne;
      },
    } });
    await openClawhub();
    await screen.findByText("Weather");
    broken = true;
    fireEvent.click(screen.getByRole("button", { name: "加载更多" }));
    await screen.findByText(/保留上次成功结果：推荐/);
    expect(cardTitles()).toEqual(["Weather", "agents-build"]);
    expect(screen.queryByText("Browser")).not.toBeInTheDocument();
  });

  it("shows error, retry and the official link with no prior result and isolates other sources", async () => {
    let broken = true;
    setup({ commands: {
      list_clawhub_skills: () => {
        if (broken) throw { code: "marketplace.unavailable", message: "无法读取 ClawHub 目录。", detail: "HTTP 503" };
        return clawhubPageOne;
      },
    } });
    await openClawhub();
    await screen.findByText(/ClawHub 请求失败。尚无可显示的结果。/);
    expect(screen.queryAllByRole("article")).toHaveLength(0);
    expect(screen.getByRole("button", { name: "ClawHub 官网" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "SkillHub" }));
    await screen.findByRole("button", { name: marketplaceSkill.name });
    fireEvent.click(screen.getByRole("button", { name: "ClawHub" }));
    await screen.findByText(/ClawHub 请求失败。尚无可显示的结果。/, undefined, { timeout: 2000 });
    expect(screen.queryByRole("button", { name: marketplaceSkill.name })).not.toBeInTheDocument();

    broken = false;
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    await screen.findByText("Weather");
    expect(screen.queryByText(/尚无可显示的结果/)).not.toBeInTheDocument();
  });

  it("shows a distinct rate-limit state and backs off retry", async () => {
    const { backend } = setup({ commands: {
      list_clawhub_skills: () => {
        throw {
          code: "marketplace.rate_limited",
          message: "ClawHub 请求受限（HTTP 429），已暂停请求。",
          detail: "上游要求等待约 30 秒后再试。",
          retryAfterSeconds: 30,
          retryable: true,
        };
      },
    } });
    await openClawhub();
    await screen.findByText(/ClawHub 请求受限（HTTP 429）/, { exact: false });
    expect(screen.getByText(/约 30 秒后重试/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重试" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "ClawHub 官网" })).toBeInTheDocument();
    expect(screen.queryByText("ClawHub 暂无技能条目。")).not.toBeInTheDocument();
    const calls = backend.count("list_clawhub_skills");
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(backend.count("list_clawhub_skills")).toBe(calls);
  });

  it("does not fetch ClawHub while SkillHub is selected", async () => {
    const { backend } = setup();
    await openMarket();
    await screen.findByRole("button", { name: marketplaceSkill.name });
    expect(backend.count("list_clawhub_skills")).toBe(0);
    expect(screen.queryByText("@aws/agents-build")).not.toBeInTheDocument();
  });
});
