import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import {
  createFakeBackend,
  type FakeBackendOptions,
} from "./test/fake-backend";
import { expertPack, packSnapshot } from "./test/expert-pack-fixtures";
import { marketplaceSkill } from "./test/marketplace-fixtures";
import type { LocalPack } from "./lib/expert-packs";
function setup(options: FakeBackendOptions = {}) {
  const backend = createFakeBackend(options).install();
  render(
    <QueryClientProvider client={createQueryClient()}>
      <App />
    </QueryClientProvider>,
  );
  return backend;
}
async function openPacks() {
  fireEvent.click(await screen.findByRole("button", { name: "技能市场" }));
  fireEvent.click(await screen.findByRole("button", { name: "专家包" }));
  return screen.findByRole("main", { name: "专家包" });
}
async function openPack() {
  const page = await openPacks();
  fireEvent.click(
    await within(page).findByRole("button", { name: /研究助理.*研究流程/ }),
  );
  await within(page).findByRole("heading", { name: "研究助理" });
  return page;
}
async function previewPack() {
  const page = await openPack();
  const install = within(page).getByRole("button", { name: "安装专家包" });
  await waitFor(() => expect(install).toBeEnabled());
  fireEvent.click(install);
  const dialog = await screen.findByRole("dialog", {
    name: "安装专家包 · 研究助理",
  });
  fireEvent.click(
    within(dialog).getByRole("checkbox", { name: "Claude Code" }),
  );
  fireEvent.click(within(dialog).getByRole("button", { name: "预览安装" }));
  await within(dialog).findByRole("button", { name: "确认安装" });
  return dialog;
}
describe("expert packs", () => {
  it("browses and searches packs and opens member detail without requiring installation", async () => {
    const backend = setup();
    const page = await openPacks();
    fireEvent.change(
      within(page).getByRole("textbox", { name: "搜索专家包" }),
      { target: { value: "研究" } },
    );
    fireEvent.click(within(page).getByRole("button", { name: "搜索" }));
    await waitFor(() =>
      expect(
        backend.callsOf("list_expert_packs").slice(-1)[0]?.payload,
      ).toEqual({ keyword: "研究", page: 1 }),
    );
    fireEvent.click(
      await within(page).findByRole("button", { name: /研究助理.*研究流程/ }),
    );
    fireEvent.click(
      await within(page).findByRole("button", {
        name: "@example/workspace-notes",
      }),
    );
    await screen.findByRole("main", { name: marketplaceSkill.name });
    expect(backend.count("prepare_pack_install")).toBe(0);
    expect(backend.count("get_marketplace_cli")).toBe(0);
  });
  it("shows adopted versions and adapted workflow then installs the pack with one task", async () => {
    const backend = setup({ autoTasks: true });
    const dialog = await previewPack();
    expect(dialog).toHaveTextContent("@example/workspace-notes");
    expect(dialog).toHaveTextContent("1.2.0");
    expect(dialog).toHaveTextContent("本地技能名称对照");
    expect(backend.callsOf("prepare_pack_install")[0].payload).toMatchObject({
      scope: "global",
      agents: ["claude-code"],
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "确认安装" }));
    await within(dialog).findByTestId("expert-pack-result");
    expect(backend.count("start_pack_install")).toBe(1);
    expect(backend.count("start_marketplace_install")).toBe(0);
    fireEvent.click(within(dialog).getByRole("button", { name: "返回专家包" }));
    await screen.findByRole("button", { name: "已安装" });
    fireEvent.click(
      screen.getByRole("button", { name: "本地安装快照 · 完整" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "忘记专家包" }));
    expect(
      screen.getByText(/保留工作流、成员、挂载及来源记录/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "确认忘记" }));
    await waitFor(() => expect(backend.count("forget_expert_pack")).toBe(1));
    const inventory = await backend.dispatch("get_inventory", {
      scope: "global",
    });
    expect(inventory.skills.some((s) => s.sourceType === "skillhub-pack")).toBe(
      true,
    );
  });
  it("keeps local snapshot available offline and supplements only the original targets", async () => {
    const snapshot = packSnapshot(["claude-code"]);
    const local: LocalPack = {
      snapshot,
      complete: false,
      skills: [
        {
          name: snapshot.workflowName,
          present: false,
          missingAgents: ["claude-code"],
          attachedAgents: [],
          version: null,
          versionChanged: false,
          modified: false,
          conflict: false,
        },
      ],
    };
    const backend = setup({
      commands: {
        list_expert_packs: () => {
          throw new Error("offline");
        },
        get_expert_pack: () => {
          throw new Error("removed");
        },
        list_local_packs: () => [local],
      },
    });
    const page = await openPacks();
    fireEvent.click(
      await within(page).findByRole("button", { name: "研究助理 · 未完成" }),
    );
    await within(page).findByText("存在缺失技能或原目标挂载 · 全局");
    expect(page).toHaveTextContent("已确认的工作流");
    expect(page).toHaveTextContent("本地技能名称对照");
    fireEvent.click(within(page).getByRole("button", { name: "补齐安装" }));
    const dialog = await screen.findByRole("dialog", {
      name: "补齐安装 · 研究助理",
    });
    expect(within(dialog).queryByRole("checkbox")).not.toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "预览安装" }));
    await waitFor(() =>
      expect(
        backend.callsOf("prepare_pack_install")[0]?.payload.agents,
      ).toEqual(["claude-code"]),
    );
  });
  it("leaves blocked packages readable with their installation reason", async () => {
    setup({
      commands: {
        get_expert_pack: () => ({
          ...expertPack,
          installError: { message: "不支持 SOUL 组合包", retryable: false },
        }),
      },
    });
    const page = await openPack();
    await within(page).findByText("不支持 SOUL 组合包");
    expect(
      within(page).getByRole("button", { name: "安装专家包" }),
    ).toBeDisabled();
    expect(page).toHaveTextContent("官方工作流");
  });
});
