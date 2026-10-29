/**
 * Resolve a markdown link or image source against the directory of the file
 * being previewed. Returns a skill-relative path, or null when the target is
 * absolute, scheme-based, or escapes the skill root.
 */
export function resolveSkillRelativePath(
  baseDir: string,
  href: string,
): string | null {
  const raw = href.split("#")[0]?.split("?")[0] ?? "";
  if (!raw || raw.startsWith("/") || raw.includes("\\")) return null;
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(raw)) return null;

  const segments = baseDir.split("/").filter(Boolean);
  for (const part of raw.split("/")) {
    let segment: string;
    try {
      segment = decodeURIComponent(part);
    } catch {
      return null;
    }
    if (segment === "" || segment === ".") continue;
    if (segment === "..") {
      if (segments.length === 0) return null;
      segments.pop();
      continue;
    }
    if (segment.includes("/") || segment.includes("\\") || segment.includes("\0")) {
      return null;
    }
    segments.push(segment);
  }
  return segments.length > 0 ? segments.join("/") : null;
}

export function parentDir(path: string): string {
  const index = path.lastIndexOf("/");
  return index === -1 ? "" : path.slice(0, index);
}

export function fileName(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

export function isImagePath(path: string): boolean {
  return /\.(png|jpe?g|gif|webp|svg)$/i.test(path);
}

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined) return "—";
  if (bytes < 1024) return `${bytes} B`;
  const kib = bytes / 1024;
  if (kib < 1024) return `${trim(kib)} KiB`;
  return `${trim(kib / 1024)} MiB`;
}

function trim(value: number): string {
  return value.toFixed(1).replace(/\.0$/, "");
}
