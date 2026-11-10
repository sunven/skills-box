import ReactMarkdown from "react-markdown";
import { Download, RefreshCw } from "lucide-react";
import type { AppUpdateModel } from "../hooks/use-app-update";
import { APP_RELEASE_URL } from "../lib/app-update";
import { openExternalUrl } from "../lib/api";
import { Button } from "./ui/button";

const labels = {
  idle: "尚未检查应用更新", checking: "正在检查应用更新…", current: "当前已是最新稳定版",
  available: "有新版可用", downloading: "正在下载更新包…", verifying: "正在验证更新包签名…",
  ready: "更新包已就绪", installing: "正在安装更新…", installed: "已安装，请重启应用",
  restarting: "正在准备重启…", error: "应用更新未完成",
};

export function AppUpdateSettings({ update }: { update: AppUpdateModel }) {
  const state = update.state;
  const phase = state?.phase ?? "idle";
  const enabled = state?.enabled ?? false;
  const total = state?.totalBytes;
  const percent = total && total > 0 ? Math.min(100, Math.round((state!.downloadedBytes / total) * 100)) : null;
  const progress = phase === "downloading" || phase === "verifying";
  const buttonLabel = phase === "ready" ? "安装并重启" : phase === "installed" ? "重启应用"
    : phase === "error" ? "重试更新" : `更新到 ${state?.candidate?.version ?? "新版"}`;
  return (
    <div className="app-update-settings">
      <p className="text-muted-foreground">当前版本：<span className="num">{state?.currentVersion ?? "正在读取…"}</span></p>
      <p role="status">{!enabled && state ? "开发运行不检查真实应用更新" : labels[phase]}</p>
      <p className="text-[12px] text-muted-foreground">启动和每 6 小时自动检查。点击更新后下载安装并重启；已有 Skill 操作会先完成。</p>
      <div className="mt-4 flex flex-wrap gap-2">
        <Button variant="outline" disabled={!update.canCheck}
          onClick={update.check} aria-busy={phase === "checking"}><RefreshCw aria-hidden="true" />检查应用更新</Button>
        {state?.candidate ? <Button disabled={update.busy} onClick={update.update}>
          <Download aria-hidden="true" />{buttonLabel}
        </Button> : null}
        <Button variant="ghost" onClick={() => void openExternalUrl(APP_RELEASE_URL)}>查看发布页面</Button>
      </div>
      {progress ? <div className="mt-4" role="status">
        <p>{phase === "verifying" ? "下载完成，正在验签…" : percent === null ? `已下载 ${Math.round((state?.downloadedBytes ?? 0) / 1024)} KB` : `已下载 ${percent}%`}</p>
        <progress className="app-update-progress" aria-label="应用更新下载进度" max={100} value={phase === "verifying" ? 100 : percent ?? undefined} />
      </div> : null}
      {update.critical ? <p className="notice mt-4" role="status">安装与重启期间无法关闭应用或开始新写入，请稍候。</p> : null}
      {update.error ? <div className="notice mt-4" data-tone="negative" role="alert">
        <div>
          <p>{update.error.message}</p>{update.error.hint ? <p className="mt-1">{update.error.hint}</p> : null}
        </div>
      </div> : null}
      {state?.candidate ? <div className="app-update-notes mt-6">
        <h3 className="font-medium">版本 {state.candidate.version}</h3>
        {state.candidate.notes ? <ReactMarkdown skipHtml components={{
          a: ({ href, children }) => <a href={href} onClick={(event) => {
            event.preventDefault();
            if (href?.startsWith("https://")) void openExternalUrl(href);
          }}>{children}</a>,
          img: () => null,
        }}>{state.candidate.notes}</ReactMarkdown> : <p className="mt-2 text-muted-foreground">此版本未提供更新说明。</p>}
      </div> : null}
    </div>
  );
}

export function AppUpdateReminder({ update, onView }: { update: AppUpdateModel; onView: () => void }) {
  if (!update.showReminder) return null;
  return <aside className="app-update-reminder" aria-label="应用更新提醒" role="status">
    <p className="font-medium">Skills Box {update.state?.candidate?.version} 已发布</p>
    <div className="mt-3 flex gap-2">
      <Button size="sm" onClick={onView}>查看应用更新</Button>
      <Button size="sm" variant="ghost" aria-label="关闭应用更新提醒" onClick={update.dismiss}>稍后</Button>
    </div>
  </aside>;
}
