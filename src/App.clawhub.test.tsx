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
  clawhubSearch,
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
async function searchClawhub(keyword: string) {
  fireEvent.change(screen.getByLabelText("搜索 ClawHub"), { target: { value: keyword } });
  fireEvent.click(screen.getByRole("button", { name: "搜索" }));
}

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

  it("keeps per-source session state and shows the expert-pack switch only for SkillHub", async () => {
    const { backend } = setup();
    await openMarket();
    fireEvent.change(screen.getByLabelText("搜索 SkillHub"), { target: { value: "hub draft" } });
    const hubList = screen.getByRole("main", { name: "技能市场" });
    fireEvent.scroll(hubList, { target: { scrollTop: 100 } });

    fireEvent.click(screen.getByRole("button", { name: "skills.sh" }));
    await screen.findByRole("button", { name: skillsShSkill.name });
    expect(screen.queryByRole("group", { name: "市场类型" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "专家包" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "ClawHub" }));
    await screen.findByText("Weather");
    fireEvent.click(screen.getByRole("button", { name: "下载量" }));
    await waitFor(() => expect(backend.callsOf("list_clawhub_skills").slice(-1)[0]!.payload.sort).toBe("downloads"));
    const clawList = screen.getByRole("main", { name: "技能市场" });
    fireEvent.scroll(clawList, { target: { scrollTop: 320 } });
    expect(screen.queryByRole("group", { name: "市场类型" })).not.toBeInTheDocument();

    // Expert packs are reachable only from SkillHub.
    fireEvent.click(screen.getByRole("button", { name: "SkillHub" }));
    expect(screen.getByRole("group", { name: "市场类型" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "专家包" }));
    expect(within(screen.getByRole("group", { name: "市场类型" })).getByText("来源 · SkillHub")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "技能" }));
    expect(screen.getByRole("button", { name: "SkillHub" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByLabelText("搜索 SkillHub")).toHaveValue("hub draft");
    expect(hubList.scrollTop).toBe(100);

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

  it("searches in upstream order, excludes skills.sh aliases and states the returned limit", async () => {
    const { backend } = setup();
    await openClawhub();
    await screen.findByText("Weather");
    await searchClawhub("weather");

    await screen.findByText(/搜索「weather」 · 返回 2 项（上游上限 50） · 按上游顺序/);
    expect(cardTitles()).toEqual(["Weather", "agents-build"]);
    expect(backend.callsOf("search_clawhub_skills")[0]!.payload).toEqual({ query: "weather" });
    expect(screen.getByText(/已排除 skills.sh 别名条目；skills.sh 条目请在 skills.sh 来源浏览\/安装。/)).toBeInTheDocument();
    expect(screen.getByText("本次已排除 1 个 skills.sh 别名条目。")).toBeInTheDocument();
    const clawhub = within(screen.getByRole("main", { name: "技能市场" }));
    expect(clawhub.queryByText(/共\s*[\d,]+\s*项/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "加载更多" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: marketplaceSkill.name })).not.toBeInTheDocument();
  });

  it("states that a full upstream limit is a limited result set, never a full search", async () => {
    setup({ commands: {
      search_clawhub_skills: ({ query }) => ({
        ...clawhubSearch,
        skills: [{ ...clawhubSkill, name: `hit-${query}` }],
        returned: 1,
        upstreamReturned: 50,
        limit: 50,
        filteredAliases: 49,
      }),
    } });
    await openClawhub();
    await searchClawhub("design");

    await screen.findByText("hit-design");
    expect(screen.getByText(/返回 1 项（上游上限 50）/)).toBeInTheDocument();
    expect(screen.getByText(/上游仅返回本次上限内的有限结果，不代表全量搜索/)).toBeInTheDocument();
    expect(screen.getByText("本次已排除 49 个 skills.sh 别名条目。")).toBeInTheDocument();
  });

  it("shows an explicit empty search state and keeps the alias notice", async () => {
    setup({ commands: {
      search_clawhub_skills: () => ({
        skills: [], returned: 0, upstreamReturned: 0, limit: 50, filteredAliases: 0, filteredOthers: 0,
      }),
    } });
    await openClawhub();
    await searchClawhub("nothing-matches");

    await screen.findByText("未找到匹配的 ClawHub 技能。请尝试其他关键词。");
    expect(screen.getByText(/返回 0 项（上游上限 50）/)).toBeInTheDocument();
    expect(screen.getByText(/已排除 skills.sh 别名条目/)).toBeInTheDocument();
    expect(screen.queryAllByRole("article")).toHaveLength(0);
  });

  it("retains the last successful search under its old keyword, then retries without mixing sources", async () => {
    let broken = false;
    const { backend } = setup({ commands: {
      search_clawhub_skills: ({ query }) => {
        if (query === "broken" && broken) {
          throw { code: "marketplace.unavailable", message: "无法读取 ClawHub 目录。", detail: "HTTP 503" };
        }
        return {
          ...clawhubSearch,
          skills: [{ ...clawhubSkill, name: `search-${query}` }],
          returned: 1, upstreamReturned: 1, filteredAliases: 0,
        };
      },
    } });
    await openClawhub();
    await screen.findByText("Weather");
    await searchClawhub("good");
    await screen.findByText("search-good");

    broken = true;
    await searchClawhub("broken");
    await screen.findByText(/ClawHub 搜索失败。保留上次成功结果：搜索「good」，返回 1 项。/);
    expect(screen.getByText("search-good")).toBeInTheDocument();
    expect(screen.queryByText("search-broken")).not.toBeInTheDocument();
    expect(screen.getByText("HTTP 503")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: marketplaceSkill.name })).not.toBeInTheDocument();
    expect(screen.queryByText(skillsShSkill.name)).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "ClawHub 官网" }));
    expect(backend.callsOf("open_external_url").slice(-1)[0]!.payload.url).toBe("https://clawhub.ai/skills");

    broken = false;
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    await screen.findByText("search-broken");
    expect(screen.queryByText(/保留上次成功结果/)).not.toBeInTheDocument();
  });

  it("shows error, retry and the official link for a search with no prior result", async () => {
    let broken = true;
    setup({ commands: {
      search_clawhub_skills: ({ query }) => {
        if (broken) throw { code: "marketplace.unavailable", message: "无法读取 ClawHub 目录。", detail: "HTTP 503" };
        return { ...clawhubSearch, skills: [{ ...clawhubSkill, name: `search-${query}` }], returned: 1, upstreamReturned: 1, filteredAliases: 0 };
      },
    } });
    await openClawhub();
    await screen.findByText("Weather");
    await searchClawhub("weather");

    await screen.findByText(/ClawHub 搜索失败。尚无可显示的结果。/);
    expect(screen.queryAllByRole("article")).toHaveLength(0);
    expect(screen.getByRole("button", { name: "ClawHub 官网" })).toBeInTheDocument();
    expect(screen.queryByText(/SkillHub 请求失败/)).not.toBeInTheDocument();
    expect(screen.queryByText(/skills.sh 请求失败/)).not.toBeInTheDocument();

    broken = false;
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    await screen.findByText("search-weather");
    expect(screen.queryByText(/尚无可显示的结果/)).not.toBeInTheDocument();
  });

  it("returns to the list with its sort, loaded pages and scroll when the query is cleared", async () => {
    const { backend } = setup();
    await openClawhub();
    await screen.findByText("Weather");
    fireEvent.click(screen.getByRole("button", { name: "下载量" }));
    await waitFor(() => expect(backend.callsOf("list_clawhub_skills").slice(-1)[0]!.payload.sort).toBe("downloads"));
    fireEvent.click(await screen.findByRole("button", { name: "加载更多" }));
    await screen.findByText("Browser");
    const list = screen.getByRole("main", { name: "技能市场" });
    fireEvent.scroll(list, { target: { scrollTop: 300 } });
    expect(cardTitles()).toEqual(["Weather", "agents-build", "Browser"]);

    await searchClawhub("weather");
    await screen.findByText(/搜索「weather」 · 返回 2 项/);
    expect(cardTitles()).toEqual(["Weather", "agents-build"]);
    fireEvent.scroll(list, { target: { scrollTop: 120 } });

    fireEvent.change(screen.getByLabelText("搜索 ClawHub"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "搜索" }));
    await waitFor(() => expect(screen.queryByText(/搜索「weather」/)).not.toBeInTheDocument());
    expect(cardTitles()).toEqual(["Weather", "agents-build", "Browser"]);
    expect(screen.getByRole("button", { name: "下载量" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByLabelText("搜索 ClawHub")).toHaveValue("");
    expect(list.scrollTop).toBe(300);
    expect(screen.queryByText(/正在加载更多/)).not.toBeInTheDocument();
  });

  it("restores the search query and scroll position within the session", async () => {
    setup();
    await openClawhub();
    await screen.findByText("Weather");
    await searchClawhub("weather");
    await screen.findByText(/搜索「weather」 · 返回 2 项/);
    const list = screen.getByRole("main", { name: "技能市场" });
    fireEvent.scroll(list, { target: { scrollTop: 140 } });

    fireEvent.click(screen.getByRole("button", { name: "SkillHub" }));
    await screen.findByRole("button", { name: marketplaceSkill.name });
    fireEvent.click(screen.getByRole("button", { name: "ClawHub" }));

    expect(screen.getByLabelText("搜索 ClawHub")).toHaveValue("weather");
    await screen.findByText(/搜索「weather」 · 返回 2 项/);
    expect(cardTitles()).toEqual(["Weather", "agents-build"]);
    expect(list.scrollTop).toBe(140);
  });

  it("shows a distinct rate-limit state for search and backs off retry", async () => {
    const { backend } = setup({ commands: {
      search_clawhub_skills: () => {
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
    await screen.findByText("Weather");
    await searchClawhub("weather");

    await screen.findByText(/ClawHub 搜索受限（HTTP 429）/);
    expect(screen.getByText(/约 30 秒后重试/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重试" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "ClawHub 官网" })).toBeInTheDocument();
    const calls = backend.count("search_clawhub_skills");
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(backend.count("search_clawhub_skills")).toBe(calls);
  });
});
