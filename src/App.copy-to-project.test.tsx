import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import { pickProjectFolder } from "./lib/folder-picker";
import { createFakeBackend, type FakeBackendOptions } from "./test/fake-backend";
import { makeSkill, makeSnapshot } from "./test/inventory-fixtures";
import type { InventoryAgent, ProjectEntry, ScopeId } from "./lib/types";

vi.mock("./lib/folder-picker", () => ({ pickProjectFolder: vi.fn() }));
const project: ProjectEntry = {
  id: "11111111-1111-4111-8111-111111111111", root: "/tmp/project", displayName: "project",
  addedAt: 1, lastOpenedAt: 1, available: true, checkRevision: "initial",
};
const PROJECT: ScopeId = `project:${project.id}`;

function setup(options: FakeBackendOptions = {}) {
  const backend = createFakeBackend({
    autoTasks: { delayMs: 10 }, ...options,
    data: {
      snapshot: makeSnapshot({ skills: [makeSkill({ name: "alpha" }), makeSkill({ name: "beta" })] }),
      projects: { projects: [project], lastActiveScope: "global", warnings: [] }, ...options.data,
    },
  }).install();
  const client = createQueryClient();
  render(<QueryClientProvider client={client}><App /></QueryClientProvider>);
  return backend;
}
async function openProjectCopy() {
  fireEvent.click(await screen.findByRole("checkbox", { name: "选择技能 alpha" }));
  expect(screen.queryByRole("button", { name: "批量复制到项目" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "批量挂载到 Agent" }));
  fireEvent.click(screen.getByRole("radio", { name: "项目" }));
}
async function chooseExistingProject() {
  fireEvent.change(screen.getByRole("combobox", { name: "选择目标项目" }), { target: { value: project.root } });
  await waitFor(() => expect(screen.getByRole("button", { name: "确认复制到项目 (1)" })).toBeEnabled());
}

describe("Copy to project from global batch attach", () => {
  beforeEach(() => vi.mocked(pickProjectFolder).mockReset());

  it("requires a directory and does not register a newly selected directory on cancellation", async () => {
    const backend = setup();
    await openProjectCopy();
    expect(screen.getByText("请先选择项目或项目目录。")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "确认复制到项目 (0)" })).toBeDisabled();
    vi.mocked(pickProjectFolder).mockResolvedValue("/tmp/new-project");
    fireEvent.click(screen.getByRole("button", { name: "选择目录" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "确认复制到项目 (1)" })).toBeEnabled());
    expect((await backend.dispatch("list_projects", {})).projects).toEqual([project]);
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(screen.queryByTestId("batch-dialog")).not.toBeInTheDocument();
    expect(backend.count("start_batch")).toBe(0);
    expect(backend.count("add_project")).toBe(0);
  });

  it("copies with zero independent Agents, registers on confirmation, and opens the project only on request", async () => {
    const backend = setup();
    await openProjectCopy();
    vi.mocked(pickProjectFolder).mockResolvedValue("/tmp/new-project");
    fireEvent.click(screen.getByRole("button", { name: "选择目录" }));
    const confirm = await screen.findByRole("button", { name: "确认复制到项目 (1)" });
    await waitFor(() => expect(confirm).toBeEnabled());
    fireEvent.click(confirm);
    await screen.findByTestId("batch-result");
    const projects = await backend.dispatch("list_projects", {});
    const added = projects.projects.find((p) => p.root === "/tmp/new-project")!;
    expect(added).toBeDefined();
    expect(projects.lastActiveScope).toBe("global");
    expect(screen.queryByTestId("project-header")).not.toBeInTheDocument();
    expect(within(screen.getByTestId("batch-result")).getByText("已复制到项目")).toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "选择技能 alpha" })).not.toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "查看项目" }));
    const header = await screen.findByTestId("project-header");
    expect(within(header).getByText("new-project")).toBeInTheDocument();
    const snapshot = await backend.dispatch("get_inventory", { scope: `project:${added.id}` });
    expect(snapshot.skills[0]?.canonicalPath).toBe("/tmp/new-project/.agents/skills/alpha");
  });

  it("counts copies and attachments separately and clears Agent selection when the destination changes", async () => {
    const backend = setup();
    await openProjectCopy();
    await chooseExistingProject();
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 Agent Claude Code" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "确认复制到项目 (2)" })).toBeEnabled());
    expect(screen.getByText("1 个复制 · 1 个挂载")).toBeInTheDocument();
    vi.mocked(pickProjectFolder).mockResolvedValue("/tmp/another-project");
    fireEvent.click(screen.getByRole("button", { name: "选择目录" }));
    await waitFor(() => expect(screen.getByRole("checkbox", { name: "选择 Agent Claude Code" })).not.toBeChecked());
    await waitFor(() => expect(screen.getByRole("button", { name: "确认复制到项目 (1)" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "确认复制到项目 (1)" }));
    await screen.findByTestId("batch-result");
    expect(backend.count("add_project")).toBe(0);
    expect(backend.callsOf("preview_batch").some((call) => call.payload.request.projectRoot === "/tmp/another-project" && call.payload.request.agentIds.length === 0)).toBe(true);
  });

  it.each([false, true])("offers independent project targets with CodeBuddy detected: %s", async (codebuddyDetected) => {
    const workbuddy: InventoryAgent = {
      id: "workbuddy", displayName: "WorkBuddy", universal: false, shared: false,
      globalSkillsDir: "~/.workbuddy/skills", projectSkillsDir: ".workbuddy/skills",
      origin: "builtin", detected: true,
    };
    const codebuddy: InventoryAgent = {
      id: "codebuddy", displayName: "CodeBuddy", universal: false, shared: false,
      globalSkillsDir: "~/.codebuddy/skills", origin: "catalog", detected: true,
    };
    const agents = codebuddyDetected ? [codebuddy, workbuddy] : [workbuddy];
    const backend = setup({ data: { snapshot: makeSnapshot({ agents, skills: [makeSkill({ name: "alpha" })] }) } });
    await openProjectCopy();
    await chooseExistingProject();
    const dialog = within(screen.getByTestId("batch-dialog"));
    const target = dialog.getByRole("checkbox", { name: "选择 Agent WorkBuddy" });
    expect(dialog.queryByRole("checkbox", { name: "选择 Agent CodeBuddy / WorkBuddy" })).not.toBeInTheDocument();
    if (codebuddyDetected) {
      fireEvent.click(dialog.getByRole("checkbox", { name: "选择 Agent CodeBuddy" }));
    } else {
      expect(dialog.queryByRole("checkbox", { name: "选择 Agent CodeBuddy" })).not.toBeInTheDocument();
    }
    fireEvent.click(target);
    const count = codebuddyDetected ? 3 : 2;
    await waitFor(() => expect(screen.getByRole("button", { name: `确认复制到项目 (${count})` })).toBeEnabled());
    expect(dialog.getByText("/tmp/project/.workbuddy/skills/alpha")).toBeInTheDocument();
    if (codebuddyDetected) expect(dialog.getByText("/tmp/project/.codebuddy/skills/alpha")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: `确认复制到项目 (${count})` }));
    await screen.findByTestId("batch-result");
    const copied = await backend.dispatch("get_inventory", { scope: PROJECT });
    expect(copied.skills[0]?.attachments).toHaveLength(count - 1);
    expect(copied.skills[0]?.attachments).toEqual(expect.arrayContaining([expect.objectContaining({
      agentId: "workbuddy", agentName: "WorkBuddy", path: "/tmp/project/.workbuddy/skills/alpha",
    })]));
    if (codebuddyDetected) {
      expect(copied.skills[0]?.attachments).toEqual(expect.arrayContaining([expect.objectContaining({
        agentId: "codebuddy", agentName: "CodeBuddy", path: "/tmp/project/.codebuddy/skills/alpha",
      })]));
    }
    expect((await backend.dispatch("get_inventory", { scope: "global" })).agents).toEqual(agents);
  });

  it("reuses existing identical managed content and skips different content without changing project records", async () => {
    const backend = setup({ data: { snapshots: { [PROJECT]: makeSnapshot({ skills: [
      makeSkill({ name: "alpha", id: `${PROJECT}/alpha`, canonicalPath: `${project.root}/.agents/skills/alpha` }),
    ] }) } } });
    await openProjectCopy();
    fireEvent.change(screen.getByRole("combobox", { name: "选择目标项目" }), { target: { value: project.root } });
    await screen.findByText("项目已有相同内容，复用，不重复复制。");
    expect(screen.getByRole("button", { name: "确认复制到项目 (0)" })).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 Agent Claude Code" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "确认复制到项目 (1)" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "确认复制到项目 (1)" }));
    await screen.findByTestId("batch-result");
    expect((await backend.dispatch("get_inventory", { scope: PROJECT })).skills).toHaveLength(1);
    expect((await backend.dispatch("get_inventory", { scope: "global" })).skills[0]?.attachments).toHaveLength(0);
  });
});
