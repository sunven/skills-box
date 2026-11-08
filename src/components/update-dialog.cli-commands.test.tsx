import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { UpdateDialog } from "./update-dialog";
import { createQueryClient } from "../lib/query-client";
import { useCliCommandSubscription } from "../hooks/use-cli-commands";
import { selectUpdateDialog } from "../lib/update-view";
import { createFakeBackend } from "../test/fake-backend";
import { makeSnapshot, makeUpdateViewSkill } from "../test/inventory-fixtures";

function setup() {
  const skills = ["alpha", "beta"].map((name) => makeUpdateViewSkill({ name, source: "owner/repo", update: "update" }));
  const snapshot = makeSnapshot({ skills });
  let rejectRetry = true;
  const backend = createFakeBackend({
    data: { snapshot },
    commands: {
      start_update: (args, fake) => {
        if (args.skillIds.length === 1 && rejectRetry) {
          rejectRetry = false;
          throw { code: "cli.command_changed", message: "执行命令已变化，请查看刷新后的命令并再次执行。", retryable: true };
        }
        return fake.dispatch("start_update", args);
      },
    },
  }).install({ shouldMockEvents: true });
  const view = selectUpdateDialog({ mode: "all", skills, agents: snapshot.agents, summary: { candidates: skills, skipped: [] }, singleName: null });
  function ObservedDialog() {
    useCliCommandSubscription();
    return <UpdateDialog open scope="global" view={view} onClose={vi.fn()} />;
  }
  render(<QueryClientProvider client={createQueryClient()}><ObservedDialog /></QueryClientProvider>);
  return backend;
}

describe("update command previews and retries", () => {
  it("shows every update-all command before starting and retains actual calls", async () => {
    const backend = setup();
    const start = screen.getByRole("button", { name: "开始更新 (2)" });
    expect(start).toBeDisabled();
    await waitFor(() => expect(start).toBeEnabled());
    const preview = await backend.dispatch("preview_cli_commands", { scope: "global", request: { action: "update", skills: ["alpha", "beta"] } });
    for (const entry of preview.entries) expect(screen.queryByText(entry.command!)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "查看将执行的命令" }));
    for (const entry of preview.entries) expect(screen.getByRole("tooltip")).toHaveTextContent(entry.command!);
    fireEvent.click(start);
    await waitFor(() => expect(backend.tasks).toHaveLength(1));
    const task = backend.lastTask!;
    for (const completed of [0, 1]) {
      act(() => task.emit({ taskId: task.taskId, phase: "running_cli", message: "更新中", completed, total: 2 }));
    }
    fireEvent.click(screen.getByRole("button", { name: "查看实际调用命令" }));
    expect(within(screen.getByRole("tooltip")).getAllByText(/正在执行的命令/)).toHaveLength(2);
    act(() => task.emit({ taskId: task.taskId, phase: "completed", message: "更新完成", result: {
      action: "update", scope: "global", overall: "success", skills: ["alpha", "beta"].map((name) => ({ name, status: "updated" })), warnings: [], startedAt: 1, finishedAtEpochMs: 2,
    } }));
    expect(within(screen.getByRole("tooltip")).getAllByText(/已执行的命令/)).toHaveLength(2);
    for (const entry of preview.entries) expect(screen.getByText(entry.command!)).toBeInTheDocument();
    expect(backend.callsOf("start_update")[0]!.payload.commandPreviewId).toBe(preview.id);
  });

  it("keeps retry restricted to the failed target after a changed-command rejection", async () => {
    const backend = setup();
    await waitFor(() => expect(screen.getByRole("button", { name: "开始更新 (2)" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "开始更新 (2)" }));
    await waitFor(() => expect(backend.tasks).toHaveLength(1));
    const task = backend.lastTask!;
    act(() => task.emit({ taskId: task.taskId, phase: "completed", message: "部分更新完成", result: {
      action: "update", scope: "global", overall: "partial", skills: [{ name: "alpha", status: "updated" }, { name: "beta", status: "failed" }], warnings: [], startedAt: 1, finishedAtEpochMs: 2,
    } }));
    await waitFor(() => expect(screen.getByRole("button", { name: "重试失败项" })).toBeEnabled());
    const retry = screen.getByRole("button", { name: "重试失败项" });
    fireEvent.click(within(retry.parentElement!).getByRole("button", { name: "查看将执行的命令" }));
    const retryCommands = [...screen.getByRole("tooltip").querySelectorAll("code")].map((element) => element.textContent);
    expect(retryCommands).toHaveLength(1);
    expect(retryCommands.every((command) => command?.includes("-s beta") && !command.includes("-s alpha"))).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "重试失败项" }));
    expect(await screen.findByTestId("update-start-error")).toHaveTextContent("执行命令已变化");
    expect(backend.tasks).toHaveLength(1);
    const refreshed = screen.getByRole("button", { name: "开始更新 (1)" });
    await waitFor(() => expect(refreshed).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "查看将执行的命令" }));
    const current = [...screen.getByRole("tooltip").querySelectorAll("code")].map((element) => element.textContent);
    expect(current).toHaveLength(1);
    expect(current[0]).toContain("-s beta");
    fireEvent.click(refreshed);
    await waitFor(() => expect(backend.tasks).toHaveLength(2));
    expect(backend.callsOf("start_update").map((call) => call.payload.skillIds)).toEqual([["alpha", "beta"], ["beta"], ["beta"]]);
  });
});
