import { createContext, useContext, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { listLocalPacks } from "../lib/api";
import type { LocalPack } from "../lib/expert-packs";
import type { ScopeId } from "../lib/types";
const PackContext = createContext<{
  packs: LocalPack[];
  open: (slug: string) => void;
}>({ packs: [], open: () => {} });
export function ExpertPackProvider({
  scope,
  onOpen,
  children,
}: {
  scope: ScopeId;
  onOpen: (slug: string) => void;
  children: ReactNode;
}) {
  const local = useQuery({
    queryKey: ["expertPacks", "local", scope],
    queryFn: () => listLocalPacks(scope),
    retry: false,
  });
  return (
    <PackContext value={{ packs: local.data ?? [], open: onOpen }}>
      {children}
    </PackContext>
  );
}
export function PackLinks({
  name,
  textOnly = false,
}: {
  name: string;
  textOnly?: boolean;
}) {
  const { packs, open } = useContext(PackContext);
  const related = packs.filter(
    (p) =>
      p.snapshot.workflowName === name ||
      p.snapshot.members.some((m) => m.name === name),
  );
  if (!related.length) return null;
  return (
    <span className="pack-links">
      {related.map((p) =>
        textOnly ? (
          <span
            key={p.snapshot.slug}
            className="badge"
            title={`专家包：${p.snapshot.displayName}`}
          >
            {p.snapshot.displayName}
            {p.snapshot.workflowName === name ? " · 入口" : ""}
          </span>
        ) : (
          <button
            key={p.snapshot.slug}
            className="badge"
            onClick={() => open(p.snapshot.slug)}
          >
            专家包 · {p.snapshot.displayName}
          </button>
        ),
      )}
    </span>
  );
}
export function AffectedPacks({
  names,
  skillIds,
}: {
  names?: string[];
  skillIds?: string[];
}) {
  const { packs } = useContext(PackContext);
  const related = packs.filter((p) =>
    [p.snapshot.workflowName, ...p.snapshot.members.map((m) => m.name)].some(
      (name) =>
        names?.includes(name) ||
        skillIds?.some((id) => id === name || id.endsWith(`/${name}`)),
    ),
  );
  if (!related.length) return null;
  return (
    <div className="notice" role="note">
      <p>此操作会影响以下专家包的完整性：</p>
      <p>{related.map((p) => p.snapshot.displayName).join("、")}</p>
      <p className="text-sm">
        其他成员与工作流保留，可在包详情查看缺失项并补齐。
      </p>
    </div>
  );
}
