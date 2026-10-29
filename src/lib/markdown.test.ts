import { describe, expect, it } from "vitest";
import { isHttpsUrl, safeMarkdownUrl, stripFrontmatter } from "./markdown";

describe("stripFrontmatter", () => {
  it("removes a leading frontmatter block but keeps the body", () => {
    const raw = "---\nname: alpha\ndescription: test\n---\n\n# Alpha\n\nBody\n";
    expect(stripFrontmatter(raw)).toBe("# Alpha\n\nBody\n");
  });

  it("keeps files without frontmatter intact", () => {
    const raw = "# Alpha\n\ndescription: not frontmatter\n";
    expect(stripFrontmatter(raw)).toBe(raw);
  });

  it("keeps malformed or unterminated frontmatter visible", () => {
    const unterminated = "---\nname: alpha\n\n# Alpha\n";
    expect(stripFrontmatter(unterminated)).toBe(unterminated);
  });

  it("handles CRLF and a BOM", () => {
    const raw = "\uFEFF---\r\nname: alpha\r\n---\r\n# Alpha\r\n";
    expect(stripFrontmatter(raw)).toBe("# Alpha\r\n");
  });

  it("does not strip a horizontal rule later in the document", () => {
    const raw = "# Alpha\n\n---\n\nAfter\n";
    expect(stripFrontmatter(raw)).toBe(raw);
  });
});

describe("isHttpsUrl", () => {
  it("accepts only absolute https URLs", () => {
    expect(isHttpsUrl("https://example.com/docs")).toBe(true);
    expect(isHttpsUrl("HTTPS://example.com")).toBe(true);
    expect(isHttpsUrl("http://example.com")).toBe(false);
    expect(isHttpsUrl("javascript:alert(1)")).toBe(false);
    expect(isHttpsUrl("data:text/html,x")).toBe(false);
    expect(isHttpsUrl("/relative/path")).toBe(false);
    expect(isHttpsUrl(" https://example.com")).toBe(false);
    expect(isHttpsUrl(undefined)).toBe(false);
  });
});

describe("safeMarkdownUrl", () => {
  it("allows https, fragments, and relative skill paths", () => {
    expect(safeMarkdownUrl("https://example.com/docs")).toBe(
      "https://example.com/docs",
    );
    expect(safeMarkdownUrl("#section")).toBe("#section");
    expect(safeMarkdownUrl("references/api.md")).toBe(
      "references/api.md",
    );
    expect(safeMarkdownUrl("../assets/logo.png")).toBe(
      "../assets/logo.png",
    );
    expect(safeMarkdownUrl("a%20b.md")).toBe("a%20b.md");
  });

  it("rejects schemes, absolute paths, protocol-relative URLs, and backslashes", () => {
    expect(safeMarkdownUrl("http://example.com")).toBe("");
    expect(safeMarkdownUrl("javascript:alert(1)")).toBe("");
    expect(safeMarkdownUrl("data:text/html,x")).toBe("");
    expect(safeMarkdownUrl("mailto:a@example.com")).toBe("");
    expect(safeMarkdownUrl("file:///etc/passwd")).toBe("");
    expect(safeMarkdownUrl("/etc/passwd")).toBe("");
    expect(safeMarkdownUrl("//evil.test/x")).toBe("");
    expect(safeMarkdownUrl("a\\b")).toBe("");
    expect(safeMarkdownUrl(" https://example.com")).toBe("");
    expect(safeMarkdownUrl("")).toBe("");
    expect(safeMarkdownUrl(undefined)).toBe("");
  });
});
