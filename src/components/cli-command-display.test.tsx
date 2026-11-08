import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { CliRetryControl, ActualCliCommands } from "./cli-command-display";
import { createQueryClient } from "../lib/query-client";
import { createFakeBackend } from "../test/fake-backend";
import { useCliCommandSubscription } from "../hooks/use-cli-commands";
import { cliCommandsQueryKey, mergeCommandRecord, type CliCommandPreview, type CliCommandRecord } from "../lib/cli-commands";

const command = "cd '/tmp/My project' && '/opt/Node tools/npx' -y skills@latest add 'owner/repo#release/v2' -s alpha -a universal -y --json";
const preview: CliCommandPreview = { id: "backend-preview-id", entries: [{ name: "alpha", command, reason: null }] };
function deferred<T>() {
  let resolve!: (value: T) => void;
  return { promise: new Promise<T>((done) => { resolve = done; }), resolve: (value: T) => resolve(value) };
}

describe("CLI command display", () => {
  it("renders the backend command verbatim and executes with its preview id", async () => {
    const backend = createFakeBackend({ commands: { preview_cli_commands: () => preview } }).install();
    const execute = vi.fn();
    render(<QueryClientProvider client={createQueryClient()}><CliRetryControl scope="global" request={{ action: "update", skills: ["alpha"] }} label="重试 alpha" disabled={false} onExecute={execute} /></QueryClientProvider>);
    await waitFor(() => expect(screen.getByRole("button", { name: "重试 alpha" })).toBeEnabled());
    expect(screen.queryByText(command)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "查看将执行的命令" }));
    expect(screen.getByRole("tooltip")).toHaveTextContent(command);
    fireEvent.click(screen.getByRole("button", { name: "重试 alpha" }));
    expect(execute).toHaveBeenCalledWith(preview.id);
    expect(backend.callsOf("preview_cli_commands")[0]?.payload).toMatchObject({ request: { action: "update", skills: ["alpha"] } });
  });

  it("disables execution after the request changes and ignores a late previous preview", async () => {
    const alpha = deferred<CliCommandPreview>();
    const beta = deferred<CliCommandPreview>();
    createFakeBackend({ commands: { preview_cli_commands: ({ request }) => request.action === "update" && request.skills[0] === "alpha" ? alpha.promise : beta.promise } }).install();
    const execute = vi.fn();
    const client = createQueryClient();
    const view = (name: string) => <QueryClientProvider client={client}><CliRetryControl scope="global" request={{ action: "update", skills: [name] }} label="执行" disabled={false} onExecute={execute} /></QueryClientProvider>;
    const rendered = render(view("alpha"));
    expect(screen.getByRole("button", { name: "执行" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "查看将执行的命令" }));
    rendered.rerender(view("beta"));
    await act(async () => alpha.resolve(preview));
    expect(screen.queryByText(command)).toBeNull();
    expect(screen.getByRole("button", { name: "执行" })).toBeDisabled();
    const next = { id: "beta-id", entries: [{ name: "beta", command: command.replace("-s alpha", "-s beta"), reason: null }] };
    await act(async () => beta.resolve(next));
    await waitFor(() => expect(screen.getByRole("button", { name: "执行" })).toBeEnabled());
    expect(screen.getByRole("tooltip")).toHaveTextContent(next.entries[0]!.command!);
    fireEvent.click(screen.getByRole("button", { name: "执行" }));
    expect(execute).toHaveBeenCalledWith("beta-id");
  });

  it("requires a successful refresh after a preview error", async () => {
    let failed = true;
    createFakeBackend({ commands: { preview_cli_commands: () => { if (failed) throw { code: "cli.command_changed", message: "命令已变化", retryable: true }; return preview; } } }).install();
    render(<QueryClientProvider client={createQueryClient()}><CliRetryControl scope="global" request={{ action: "update", skills: ["alpha"] }} label="执行" disabled={false} onExecute={vi.fn()} /></QueryClientProvider>);
    expect(await screen.findByRole("alert")).toHaveTextContent("命令已变化");
    expect(screen.getByRole("button", { name: "执行" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "查看将执行的命令" }));
    expect(screen.getByRole("tooltip")).toHaveTextContent("命令已变化");
    failed = false;
    fireEvent.click(screen.getByRole("button", { name: "刷新命令" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "执行" })).toBeEnabled());
  });

  it("shows the native operation explanation without inventing a CLI command", async () => {
    const native = { id: "native-preview", entries: [{ name: "alpha", command: null, reason: "由应用直接删除技能目录。" }] };
    const backend = createFakeBackend({ commands: { preview_cli_commands: () => native } }).install();
    render(<QueryClientProvider client={createQueryClient()}><CliRetryControl scope="global" request={{ action: "uninstall", skill: "alpha" }} label="执行" disabled={false} onExecute={vi.fn()} /></QueryClientProvider>);
    await waitFor(() => expect(screen.getByRole("button", { name: "执行" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "查看将执行的命令" }));
    expect(screen.getByRole("tooltip")).toHaveTextContent(native.entries[0]!.reason);
    expect(screen.getByRole("tooltip").querySelector("code")).toBeNull();
    expect(backend.tasks).toHaveLength(0);
  });

  it("merges actual events with an in-flight snapshot without downgrading status", async () => {
    const snapshot = deferred<CliCommandRecord[]>();
    createFakeBackend({ commands: { get_cli_commands: () => snapshot.promise } }).install();
    const record: CliCommandRecord = { id: "call-1", command, status: "running", startedAt: 100, context: { origin: "task", scope: "global", taskId: "task-1", source: null } };
    const client = createQueryClient();
    function Observed() { useCliCommandSubscription(); return <ActualCliCommands taskId="task-1" />; }
    render(<QueryClientProvider client={client}><Observed /></QueryClientProvider>);
    act(() => window.dispatchEvent(new CustomEvent("cli-command", { detail: { ...record, status: "failed" } })));
    await act(async () => snapshot.resolve([record]));
    fireEvent.click(screen.getByRole("button", { name: "查看实际调用命令" }));
    await waitFor(() => expect(screen.getByRole("tooltip")).toHaveTextContent("执行失败的命令"));
    expect(client.getQueryData<CliCommandRecord[]>(cliCommandsQueryKey)?.[0]?.status).toBe("failed");
    expect(screen.getByText(command)).toBeInTheDocument();
  });

  it("retains the most recent background call and terminal status", () => {
    const record: CliCommandRecord = { id: "new", command, status: "succeeded", startedAt: 200, context: { origin: "discovery", scope: "global", taskId: null, source: "owner/repo" } };
    expect(mergeCommandRecord([record], { ...record, status: "running" })).toEqual([record]);
    expect(mergeCommandRecord([record], { ...record, id: "old", startedAt: 100 })).toEqual([record]);
  });
});
