import { describe, expect, it } from "vitest";
import {
  fileName,
  formatBytes,
  isImagePath,
  parentDir,
  resolveSkillRelativePath,
} from "./skill-content";

describe("resolveSkillRelativePath", () => {
  it("resolves relative paths against the current file directory", () => {
    expect(resolveSkillRelativePath("", "references/api.md")).toBe(
      "references/api.md",
    );
    expect(resolveSkillRelativePath("references", "api.md")).toBe(
      "references/api.md",
    );
    expect(
      resolveSkillRelativePath("references/nested", "../api.md"),
    ).toBe("references/api.md");
    expect(resolveSkillRelativePath("", "./a/./b.md")).toBe("a/b.md");
    expect(resolveSkillRelativePath("", "a%20b.md")).toBe("a b.md");
  });

  it("strips fragments and queries before resolving", () => {
    expect(resolveSkillRelativePath("", "api.md#section")).toBe("api.md");
    expect(resolveSkillRelativePath("", "api.md?raw=1")).toBe("api.md");
  });

  it("rejects absolute, scheme-based, and escaping paths", () => {
    expect(resolveSkillRelativePath("", "../secret")).toBeNull();
    expect(resolveSkillRelativePath("references", "../../secret")).toBeNull();
    expect(resolveSkillRelativePath("", "/etc/passwd")).toBeNull();
    expect(resolveSkillRelativePath("", "//evil.test/x")).toBeNull();
    expect(resolveSkillRelativePath("", "https://example.com")).toBeNull();
    expect(resolveSkillRelativePath("", "mailto:a@example.com")).toBeNull();
    expect(resolveSkillRelativePath("", "a\\b")).toBeNull();
    expect(resolveSkillRelativePath("", "%2E%2E/secret")).toBeNull();
    expect(resolveSkillRelativePath("", "a/%2E%2E/b")).toBe("b");
    expect(resolveSkillRelativePath("", "")).toBeNull();
    expect(resolveSkillRelativePath("", "#fragment")).toBeNull();
  });
});

describe("path helpers", () => {
  it("returns the parent directory and file name", () => {
    expect(parentDir("SKILL.md")).toBe("");
    expect(parentDir("references/api.md")).toBe("references");
    expect(fileName("references/api.md")).toBe("api.md");
    expect(fileName("SKILL.md")).toBe("SKILL.md");
  });

  it("recognizes image paths", () => {
    expect(isImagePath("logo.png")).toBe(true);
    expect(isImagePath("photo.JPG")).toBe(true);
    expect(isImagePath("diagram.svg")).toBe(true);
    expect(isImagePath("notes.md")).toBe(false);
  });
});

describe("formatBytes", () => {
  it("formats bytes, KiB, and MiB", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1023)).toBe("1023 B");
    expect(formatBytes(1024)).toBe("1 KiB");
    expect(formatBytes(1536)).toBe("1.5 KiB");
    expect(formatBytes(1024 * 1024)).toBe("1 MiB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5 MiB");
    expect(formatBytes(null)).toBe("—");
  });
});
