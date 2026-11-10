import { useQuery } from "@tanstack/react-query";
import { getMarketplaceCli, openExternalUrl } from "../lib/api";
import { Button } from "./ui/button";
import { MutationError } from "./mutation/mutation-error";

export function MarketplaceCliHelp({ onReady }: { onReady?: () => void }) {
  const cli = useQuery({
    queryKey: ["marketplace", "cli"],
    queryFn: getMarketplaceCli,
    retry: false,
    staleTime: 0,
  });
  return (
    <div className="panel-well space-y-2" aria-label="SkillHub CLI">
      <p className="font-medium">SkillHub CLI</p>
      {cli.isFetching ? <p role="status">正在检测 SkillHub CLI…</p> : null}
      {cli.data?.available ? (
        <p>
          {cli.data.version} · {cli.data.path}
        </p>
      ) : (
        <>
          <MutationError error={cli.data?.error ?? null} />
          <p className="text-sm">
            安装和更新需要 SkillHub CLI。请在终端执行安装命令，然后重新检测。
          </p>
          <p className="text-xs">仅安装 CLI</p>
          <pre className="batch-command">
            <code>
              {cli.data?.installCommand ??
                "curl -fsSL https://skillhub-1388575217.cos.ap-guangzhou.myqcloud.com/install/install.sh | bash -s -- --cli-only"}
            </code>
          </pre>
        </>
      )}
      {cli.isError ? <p role="alert">无法检测 SkillHub CLI，请重试。</p> : null}
      <div className="flex gap-2">
        <Button
          variant="outline"
          size="sm"
          disabled={cli.isFetching}
          onClick={() =>
            void cli.refetch().then((r) => {
              if (r.data?.available) onReady?.();
            })
          }
        >
          重新检测
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={() =>
            void openExternalUrl(
              cli.data?.instructionsUrl ??
                "https://skillhub.cn/install/skillhub.md",
            )
          }
        >
          官方安装说明
        </Button>
      </div>
    </div>
  );
}
