import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import { createFakeBackend } from "./test/fake-backend";
import { appDataFixture } from "./test/app-data-fixtures";

async function openSettings() {
  render(<QueryClientProvider client={createQueryClient()}><App /></QueryClientProvider>);
  fireEvent.click(await screen.findByRole("button",{name:"设置"}));
  fireEvent.click(screen.getByRole("button",{name:"应用数据"}));
  await screen.findByText("161 MiB");
  return within(screen.getByRole("region",{name:"应用数据"}));
}
describe("application data management", () => {
  it("loads on demand, changes preferences and preserves old backups", async () => {
    const backend = createFakeBackend({ data:{appData:appDataFixture()} }).install();
    const page = await openSettings();
    expect(page.getByText("140 MiB")).toBeVisible();
    expect(page.getByRole("switch",{name:"保留操作备份"})).toBeChecked();
    fireEvent.click(page.getByRole("switch",{name:"保留操作备份"}));
    await waitFor(() => expect(page.getByRole("switch",{name:"保留操作备份"})).not.toBeChecked());
    expect(backend.callsOf("save_data_preferences")[0].payload.preferences).toEqual({retainBackups:false,operationLogging:true});
    expect(backend.count("clean_app_data")).toBe(0);
    expect(page.getAllByRole("listitem")).toHaveLength(3);
    fireEvent.click(page.getByRole("switch",{name:"记录诊断日志"}));
    await waitFor(() => expect(page.getByRole("switch",{name:"记录诊断日志"})).not.toBeChecked());
  });
  it("previews only cache by default and cancellation deletes nothing", async () => {
    const backend = createFakeBackend({data:{appData:appDataFixture()}}).install();
    const page = await openSettings();
    fireEvent.click(page.getByRole("button",{name:"预览清理"}));
    const modal = within(await screen.findByRole("dialog",{name:"确认清理应用数据"}));
    expect(modal.getByText(/预计释放 90 MiB/)).toBeVisible();
    expect(backend.callsOf("preview_app_data_cleanup")[0].payload.selection).toEqual({backupIds:[],allBackups:false,logs:false,cache:true});
    expect(modal.getByRole("button",{name:"取消"})).toHaveFocus();
    fireEvent.click(modal.getByRole("button",{name:"取消"}));
    expect(backend.count("clean_app_data")).toBe(0);
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(page.getByRole("button",{name:"预览清理"}));
    fireEvent.click(await screen.findByRole("button",{name:"确认清理"}));
    await screen.findByText(/已清理 1 项，释放 90 MiB/);
    expect(await page.findByText("71 MiB")).toBeVisible();
    expect(backend.count("clean_app_data")).toBe(1);
  });
  it("protects referenced backups and deletes only selected snapshots", async () => {
    const backend = createFakeBackend({data:{appData:appDataFixture()}}).install();
    const page = await openSettings();
    expect(page.getByRole("checkbox",{name:"选择备份 research"})).toBeDisabled();
    fireEvent.click(page.getByRole("checkbox",{name:"选择备份 agent-browser"}));
    fireEvent.click(page.getByRole("button",{name:"删除所选备份（1）"}));
    const modal = within(await screen.findByRole("dialog"));
    expect(modal.getByText("backups/agent-browser")).toBeVisible();
    expect(modal.queryByText("backups/research")).toBeNull();
    fireEvent.click(modal.getByRole("button",{name:"确认清理"}));
    await waitFor(() => expect(page.queryByRole("checkbox",{name:"选择备份 agent-browser"})).toBeNull());
    expect(page.getByRole("checkbox",{name:"选择备份 design-taste-frontend"})).toBeVisible();
    expect(backend.callsOf("preview_app_data_cleanup")[0].payload.selection.backupIds).toEqual(["backup-alpha"]);
  });
  it("keeps the old switch value after a save failure and reports the error", async () => {
    createFakeBackend({data:{appData:appDataFixture()},commands:{save_data_preferences:()=>{throw {message:"无法写入设置",retryable:true};}}}).install();
    const page = await openSettings();
    fireEvent.click(page.getByRole("switch",{name:"记录诊断日志"}));
    expect(await page.findByRole("alert")).toHaveTextContent("无法写入设置");
    expect(page.getByRole("switch",{name:"记录诊断日志"})).toBeChecked();
  });
  it("does not offer cleanup retry for an open error and clears it after opening succeeds", async () => {
    let openFails = true;
    const backend = createFakeBackend({data:{appData:appDataFixture()}, commands:{reveal_backup:()=>{
      if (openFails) throw {code:"open.failed", message:"无法打开文件或目录。", hint:"请检查系统默认应用与文件权限。", retryable:true};
    }}}).install();
    const page = await openSettings();
    fireEvent.click(page.getByRole("button",{name:"预览清理"}));
    fireEvent.click(await screen.findByRole("button",{name:"取消"}));
    fireEvent.click(page.getByRole("button",{name:"打开备份 agent-browser"}));
    const alert = await page.findByRole("alert");
    expect(alert).toHaveTextContent("无法打开文件或目录。");
    expect(within(alert).queryByRole("button",{name:"重新预览清理"})).toBeNull();
    openFails = false;
    fireEvent.click(page.getByRole("button",{name:"打开备份 agent-browser"}));
    await waitFor(() => expect(page.queryByRole("alert")).toBeNull());
    expect(backend.count("reveal_backup")).toBe(2);
    expect(backend.count("clean_app_data")).toBe(0);
  });
  it("does not offer cleanup retry for a preference save error after a preview", async () => {
    createFakeBackend({data:{appData:appDataFixture()}, commands:{save_data_preferences:()=>{
      throw {message:"无法写入设置", retryable:true};
    }}}).install();
    const page = await openSettings();
    fireEvent.click(page.getByRole("button",{name:"预览清理"}));
    fireEvent.click(await screen.findByRole("button",{name:"取消"}));
    fireEvent.click(page.getByRole("switch",{name:"记录诊断日志"}));
    const alert = await page.findByRole("alert");
    expect(alert).toHaveTextContent("无法写入设置");
    expect(within(alert).queryByRole("button",{name:"重新预览清理"})).toBeNull();
  });
  it("reports busy cleanup and requires a new preview before retrying", async () => {
    let busy = true;
    const backend = createFakeBackend({data:{appData:appDataFixture()}, commands:{clean_app_data:(args,backend)=>{ if(busy) throw {code:"task.busy",message:"busy",retryable:true}; return backend.dispatch("clean_app_data",args); }}}).install();
    const page = await openSettings();
    fireEvent.click(page.getByRole("button",{name:"清空日志"}));
    fireEvent.click(await screen.findByRole("button",{name:"确认清理"}));
    expect(await page.findByRole("alert")).toHaveTextContent("有技能任务正在执行");
    busy = false;
    fireEvent.click(page.getByRole("button",{name:"重新预览清理"}));
    fireEvent.click(await screen.findByRole("button",{name:"确认清理"}));
    await screen.findByText(/已清理 1 项，释放 10 MiB/);
    expect(backend.count("preview_app_data_cleanup")).toBe(2);
  });
  it("shows failed file details and retries with another confirmation", async () => {
    createFakeBackend({data:{appData:appDataFixture()},commands:{clean_app_data:()=>({freedBytes:1024,items:[{id:"cache",label:"cache/archives",freedBytes:1024,error:"权限不足"}]})}}).install();
    const page = await openSettings();
    fireEvent.click(page.getByRole("button",{name:"清理缓存"}));
    fireEvent.click(await screen.findByRole("button",{name:"确认清理"}));
    expect(await page.findByText(/cache\/archives：权限不足/)).toBeVisible();
    fireEvent.click(page.getByRole("button",{name:"重新预览失败项"}));
    expect(await screen.findByRole("dialog",{name:"确认清理应用数据"})).toBeVisible();
  });
});
