import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { NativePluginsPage } from "./native-plugins-page";
import { createQueryClient } from "../lib/query-client";
import { pluginQueryKey, type PluginPageState } from "../lib/native-plugins";
import type { ProjectEntry, ScopeId } from "../lib/types";
import { createFakeBackend } from "../test/fake-backend";
import { nativePluginFixture } from "../test/native-plugin-fixtures";

const a: ProjectEntry = {
  id: "11111111-1111-4111-8111-111111111111",
  root: "/tmp/project-a",
  displayName: "project-a",
  addedAt: 1,
  lastOpenedAt: 1,
  available: true,
  checkRevision: "one",
};
const b: ProjectEntry = {
  ...a,
  id: "22222222-2222-4222-8222-222222222222",
  root: "/tmp/project-b",
  displayName: "project-b",
};
const scopeOf = (project: ProjectEntry): ScopeId => `project:${project.id}`;
const projects = {
  projects: [a, b],
  lastActiveScope: scopeOf(a),
  warnings: [],
};
function page(
  project: ProjectEntry,
  client: ReturnType<typeof createQueryClient>,
) {
  return (
    <QueryClientProvider client={client}>
      <NativePluginsPage project={project} scope={scopeOf(project)} />
    </QueryClientProvider>
  );
}
async function claude() {
  await screen.findByText("workspace-tools");
  fireEvent.click(screen.getByRole("button", { name: "Claude Code" }));
  await screen.findByText("/Users/demo/.claude · 2.1.325");
}
describe("native project contexts", () => {
  it("targets project and local settings separately and refreshes all cached scopes", async () => {
    const backend = createFakeBackend({ data: { projects } }).install();
    const client = createQueryClient();
    const globalKey = pluginQueryKey("claude");
    client.setQueryData(globalKey, nativePluginFixture("claude"));
    render(page(a, client));
    await claude();
    expect(screen.getByText("继承的安装")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "卸载" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "启用" }));
    let dialog = await screen.findByRole("dialog", { name: "确认启用" });
    expect(dialog).toHaveTextContent("project-a · 项目共享");
    expect(dialog).toHaveTextContent(a.root);
    fireEvent.click(within(dialog).getByRole("button", { name: "确认执行" }));
    await screen.findByText("已完成并核对");
    await waitFor(() =>
      expect(client.getQueryState(globalKey)?.isInvalidated).toBe(true),
    );
    expect(screen.getByText("当前层设置：启用")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("combobox", { name: "插件配置层" }), {
      target: { value: "local" },
    });
    await screen.findByText("当前层设置：继承");
    fireEvent.click(screen.getByRole("button", { name: "禁用" }));
    dialog = await screen.findByRole("dialog", { name: "确认禁用" });
    expect(dialog).toHaveTextContent("仅自己在此项目");
    expect(
      backend
        .callsOf("prepare_native_plugin")
        .map((c) => c.payload.request.context),
    ).toEqual([
      { scope: scopeOf(a), layer: "project" },
      { scope: scopeOf(a), layer: "local" },
    ]);
    fireEvent.click(within(dialog).getByRole("button", { name: "确认执行" }));
    await screen.findByText("已完成并核对");
    fireEvent.change(screen.getByRole("combobox", { name: "插件配置层" }), {
      target: { value: "project" },
    });
    await screen.findByText(/被覆盖，生效配置来源/);
    expect(screen.getByText("当前层设置：启用")).toBeInTheDocument();
    expect(screen.getByText("宿主报告：禁用")).toBeInTheDocument();
  });
  it("discards confirmations and details when changing project, layer or root", async () => {
    const backend = createFakeBackend({ data: { projects } }).install();
    const client = createQueryClient();
    const view = render(page(a, client));
    await claude();
    fireEvent.click(screen.getByRole("button", { name: "启用" }));
    await screen.findByRole("dialog", { name: "确认启用" });
    fireEvent.change(screen.getByRole("combobox", { name: "插件配置层" }), {
      target: { value: "local" },
    });
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "确认启用" }),
      ).not.toBeInTheDocument(),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "workspace-tools" }),
    );
    await screen.findByText("project-map、review-guide");
    view.rerender(page(b, client));
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "workspace-tools" }),
      ).not.toBeInTheDocument(),
    );
    await screen.findByText("/Users/demo/.claude · 2.1.325");
    expect(screen.getByRole("combobox", { name: "插件配置层" })).toHaveValue(
      "project",
    );
    fireEvent.click(screen.getByRole("button", { name: "启用" }));
    await screen.findByRole("dialog", { name: "确认启用" });
    const count = backend.callsOf("read_native_plugins").length;
    view.rerender(
      page({ ...b, root: "/tmp/relocated-b", checkRevision: "two" }, client),
    );
    await waitFor(() =>
      expect(backend.callsOf("read_native_plugins").length).toBeGreaterThan(
        count,
      ),
    );
    expect(
      screen.queryByRole("dialog", { name: "确认启用" }),
    ).not.toBeInTheDocument();
    expect(backend.callsOf("execute_native_plugin")).toHaveLength(0);
  });
  it("keeps late reads from replacing the newly selected project", async () => {
    let finishRead!: (state: PluginPageState) => void;
    const backend = createFakeBackend({
      data: { projects },
      commands: {
        read_native_plugins: async (args, fake) => {
          if (args.context?.scope === scopeOf(a))
            return new Promise<PluginPageState>((resolve) => {
              finishRead = resolve;
            });
          return fake.dispatch("read_native_plugins", args);
        },
      },
    }).install();
    const client = createQueryClient();
    const view = render(page(a, client));
    await waitFor(() =>
      expect(backend.callsOf("read_native_plugins")).toHaveLength(1),
    );
    view.rerender(page(b, client));
    await screen.findByText("workspace-tools");
    const stale = nativePluginFixture();
    stale.snapshot!.plugins[0]!.name = "old-project-content";
    await act(async () => finishRead(stale));
    expect(screen.queryByText("old-project-content")).not.toBeInTheDocument();
    expect(screen.getByText(b.root)).toBeInTheDocument();
  });
  it("does not open a late confirmation after switching project", async () => {
    let finish: (() => void) | undefined;
    const backend = createFakeBackend({
      data: { projects },
      commands: {
        prepare_native_plugin: async (args, fake) => {
          const preview = await fake.dispatch("prepare_native_plugin", args);
          return new Promise((resolve) => {
            finish = () => resolve(preview);
          });
        },
      },
    }).install();
    const client = createQueryClient();
    const view = render(page(a, client));
    await claude();
    fireEvent.click(screen.getByRole("button", { name: "启用" }));
    await waitFor(() => expect(finish).toBeDefined());
    view.rerender(page(b, client));
    await screen.findByText("/Users/demo/.claude · 2.1.325");
    await act(async () => finish!());
    expect(
      screen.queryByRole("dialog", { name: "确认启用" }),
    ).not.toBeInTheDocument();
    expect(backend.callsOf("execute_native_plugin")).toHaveLength(0);
  });
});
