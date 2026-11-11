import { useCallback, useMemo, useRef, useState } from "react";
import type { MarketplaceDetail } from "../lib/marketplace";
import { Button } from "./ui/button";
import { SkillMarkdown } from "./skill-markdown";
import { MarketplaceFilesTab } from "./marketplace-files-tab";

export function MarketplaceDetailTabs({ detail }: { detail: MarketplaceDetail }) {
  const [tab, setTab] = useState<"overview" | "files">("overview");
  const [raw, setRaw] = useState(false);
  const [visitedFiles, setVisitedFiles] = useState(false);
  const [selection, setSelection] = useState({ path: "SKILL.md", expanded: new Set<string>() });
  const tabsRef = useRef<HTMLDivElement>(null);

  const selectFile = useCallback((path: string) => {
    setSelection((current) => {
      const expanded = new Set(current.expanded);
      const parts = path.split("/");
      for (let index = 1; index < parts.length; index++) expanded.add(parts.slice(0, index).join("/"));
      return { path, expanded };
    });
  }, []);
  const navigate = useCallback((path: string) => {
    selectFile(path);
    setVisitedFiles(true);
    setTab("files");
    tabsRef.current?.querySelector<HTMLButtonElement>("#marketplace-tab-files")?.focus({ preventScroll: true });
    tabsRef.current?.scrollIntoView?.({ block: "start" });
  }, [selectFile]);
  const fileLinks = useMemo(() => ({ baseDir: "", onNavigate: navigate }), [navigate]);

  function activate(next: "overview" | "files") {
    setTab(next);
    if (next === "files") setVisitedFiles(true);
  }

  return (
    <>
      <div ref={tabsRef} className="marketplace-detail-tabs" role="tablist" aria-label="市场详情内容"
        onKeyDown={(event) => {
          if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
          event.preventDefault();
          const next = event.key === "Home" ? "overview" : event.key === "End" ? "files" : tab === "overview" ? "files" : "overview";
          activate(next);
          tabsRef.current?.querySelector<HTMLButtonElement>(`#marketplace-tab-${next}`)?.focus();
        }}>
        {(["overview", "files"] as const).map((value) => (
          <button key={value} type="button" role="tab" id={`marketplace-tab-${value}`}
            aria-controls={`marketplace-panel-${value}`} aria-selected={tab === value}
            tabIndex={tab === value ? 0 : -1} onClick={() => activate(value)}>
            {value === "overview" ? "概述" : "文件"}
          </button>
        ))}
      </div>
      <div id="marketplace-panel-overview" role="tabpanel" aria-labelledby="marketplace-tab-overview" hidden={tab !== "overview"} tabIndex={0}>
        {tab === "overview" ? (
          <section className="marketplace-detail-overview" aria-label="技能概述">
            <div className="marketplace-detail-reading-tools">
              <span>SKILL.md</span>
              <Button variant="ghost" size="sm" aria-pressed={raw} onClick={() => setRaw((value) => !value)}>
                {raw ? "返回渲染" : "查看原文"}
              </Button>
            </div>
            {raw ? <pre className="marketplace-detail-source" role="region" aria-label="SKILL.md 原文" tabIndex={0}>{detail.markdown}</pre>
              : <SkillMarkdown content={detail.markdown} fileLinks={fileLinks} />}
          </section>
        ) : null}
      </div>
      <div id="marketplace-panel-files" role="tabpanel" aria-labelledby="marketplace-tab-files" hidden={tab !== "files"} tabIndex={0}>
        {visitedFiles ? (
          <MarketplaceFilesTab detail={detail} active={tab === "files"} selectedPath={selection.path}
            expanded={selection.expanded} onSelectFile={selectFile}
            onToggleDir={(path) => setSelection((current) => {
              const expanded = new Set(current.expanded);
              if (expanded.has(path)) expanded.delete(path); else expanded.add(path);
              return { ...current, expanded };
            })} />
        ) : null}
      </div>
    </>
  );
}
