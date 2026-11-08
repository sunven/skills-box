import { afterEach, describe, expect, it, vi } from "vitest";
import { readInstallSourceHistory, rememberInstallSource } from "./install-source-history";

afterEach(() => vi.restoreAllMocks());

describe("install source history", () => {
  it("persists trimmed inputs and moves repeated sources to the front", () => {
    rememberInstallSource("  phuryn/pm-skills  ");
    rememberInstallSource("https://github.com/acme/skills");
    rememberInstallSource("phuryn/pm-skills");

    expect(readInstallSourceHistory()).toEqual([
      "phuryn/pm-skills",
      "https://github.com/acme/skills",
    ]);
    expect(rememberInstallSource(" ")).toEqual(readInstallSourceHistory());
  });

  it.each(["not json", "null", '{"source":"acme/skills"}'])("ignores invalid stored history: %s", (saved) => {
    localStorage.setItem("skills-box.install-source-history", saved);
    expect(readInstallSourceHistory()).toEqual([]);
  });

  it("ignores invalid entries and duplicates in stored history", () => {
    localStorage.setItem("skills-box.install-source-history", JSON.stringify([
      " acme/skills ", null, 12, "", " ", "acme/skills", "phuryn/pm-skills",
    ]));
    expect(readInstallSourceHistory()).toEqual(["acme/skills", "phuryn/pm-skills"]);
  });

  it("keeps history storage failures from interrupting the wizard", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("Storage unavailable"); });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("Storage unavailable"); });
    expect(readInstallSourceHistory()).toEqual([]);
    expect(rememberInstallSource("acme/skills")).toEqual(["acme/skills"]);
  });
});
