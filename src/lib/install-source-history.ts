const STORAGE_KEY = "skills-box.install-source-history";

export function readInstallSourceHistory(): string[] {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "[]");
    if (!Array.isArray(saved)) return [];
    return [...new Set(saved.filter(
      (source): source is string => typeof source === "string" && source.trim().length > 0,
    ).map((source) => source.trim()))];
  } catch {
    return [];
  }
}

export function rememberInstallSource(source: string): string[] {
  const value = source.trim();
  const history = readInstallSourceHistory();
  if (!value) return history;
  const next = [value, ...history.filter((entry) => entry !== value)];
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    // History storage must not interrupt a successful preview.
  }
  return next;
}
