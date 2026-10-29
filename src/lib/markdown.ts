/** Remove a leading YAML frontmatter block for the rendered SKILL.md view. */
export function stripFrontmatter(raw: string): string {
  const text = raw.replace(/^\uFEFF/, "");
  const lines = text.split("\n");
  if ((lines[0] ?? "").trimEnd() !== "---") return text;
  let end = -1;
  for (let index = 1; index < lines.length; index += 1) {
    if (lines[index]!.trimEnd() === "---") {
      end = index;
      break;
    }
  }
  if (end === -1) return text;
  return lines
    .slice(end + 1)
    .join("\n")
    .replace(/^\n+/, "");
}

/** Links and images may only leave the app over https, via the opener. */
export function isHttpsUrl(value: unknown): boolean {
  if (typeof value !== "string" || value.trim() !== value) return false;
  try {
    const url = new URL(value);
    return url.protocol === "https:";
  } catch {
    return false;
  }
}

/**
 * URLs allowed inside a skill markdown preview: absolute https, a same-document
 * fragment, or a relative path. Relative paths are resolved against the current
 * file and can only select files inside the server-guarded skill folder.
 */
export function safeMarkdownUrl(value: unknown): string {
  if (typeof value !== "string") return "";
  if (isHttpsUrl(value)) return value;
  if (value.trim() !== value || value === "") return "";
  if (value.startsWith("#")) return value;
  if (value.startsWith("/") || value.includes("\\")) return "";
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(value)) return "";
  return value;
}
