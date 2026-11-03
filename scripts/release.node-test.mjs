import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import assert from "node:assert/strict";
import { releaseVersion } from "./prepare-release.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const versionFiles = ["package.json", "src-tauri/Cargo.lock", "src-tauri/Cargo.toml", "src-tauri/tauri.conf.json"];

function git(directory, ...args) {
  const result = spawnSync("git", args, { cwd: directory, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}

function fixture(t) {
  const temporary = mkdtempSync(join(tmpdir(), "skills-box-release-script-"));
  t.after(() => rmSync(temporary, { recursive: true, force: true }));
  const repo = join(temporary, "checkout with spaces");
  const remote = join(temporary, "origin.git");
  mkdirSync(join(repo, "scripts"), { recursive: true });
  mkdirSync(join(repo, "src-tauri"));
  for (const path of [".gitignore", "scripts/release.sh", "scripts/prepare-release.mjs"]) {
    copyFileSync(join(root, path), join(repo, path));
  }
  writeFileSync(join(repo, "package.json"), JSON.stringify({ name: "skills-box", version: "1.2.3", type: "module" }, null, 2) + "\n");
  writeFileSync(join(repo, "src-tauri/tauri.conf.json"), JSON.stringify({ version: "1.2.3", plugins: { updater: { pubkey: "fixture-public-key" } } }, null, 2) + "\n");
  writeFileSync(join(repo, "src-tauri/Cargo.toml"), '[package]\nname = "skills-box"\nversion = "1.2.3"\n\n[dependencies]\nexample = { version = "9.8.7" }\n');
  writeFileSync(join(repo, "src-tauri/Cargo.lock"), 'version = 4\n\n[[package]]\nname = "dependency"\nversion = "9.8.7"\n\n[[package]]\nname = "skills-box"\nversion = "1.2.3"\ndependencies = ["dependency"]\n');
  writeFileSync(join(repo, "pnpm-lock.yaml"), "lockfileVersion: '9.0'\n");
  for (const path of ["updater.key", "updater.key.pub", "updater.password"]) writeFileSync(join(repo, path), "ignored signing fixture");
  git(repo, "init", "--quiet", "--initial-branch=publish-test");
  git(repo, "config", "user.name", "Release Test");
  git(repo, "config", "user.email", "release-test@example.com");
  git(repo, "config", "commit.gpgSign", "false");
  git(repo, "config", "tag.gpgSign", "false");
  git(repo, "config", "core.hooksPath", "/dev/null");
  git(repo, "add", ".");
  git(repo, "commit", "--quiet", "-m", "fixture");
  git(temporary, "init", "--quiet", "--bare", remote);
  git(remote, "config", "core.hooksPath", join(remote, "hooks"));
  git(repo, "remote", "add", "origin", remote);
  return { repo, remote, head: git(repo, "rev-parse", "HEAD") };
}

function run(f, args = [], input = "n\n") {
  return spawnSync(join(f.repo, "scripts/release.sh"), args, {
    cwd: join(f.repo, "src-tauri"), input, encoding: "utf8", timeout: 15000,
  });
}

function unchanged(f, head = f.head) {
  assert.equal(git(f.repo, "rev-parse", "HEAD"), head);
  assert.equal(git(f.repo, "status", "--porcelain"), "");
  assert.equal(releaseVersion("v1.2.3", f.repo), "1.2.3");
}

test("default release increments PATCH, preserves dependencies and keys, and keeps a declined push local", (t) => {
  const f = fixture(t);
  const lock = readFileSync(join(f.repo, "src-tauri/Cargo.lock"), "utf8");
  const result = run(f);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(releaseVersion("v1.2.4", f.repo), "1.2.4");
  assert.equal(readFileSync(join(f.repo, "src-tauri/Cargo.lock"), "utf8"), lock.replace('version = "1.2.3"', 'version = "1.2.4"'));
  assert.equal(readFileSync(join(f.repo, "pnpm-lock.yaml"), "utf8"), "lockfileVersion: '9.0'\n");
  assert.equal(JSON.parse(readFileSync(join(f.repo, "src-tauri/tauri.conf.json"), "utf8")).plugins.updater.pubkey, "fixture-public-key");
  assert.deepEqual(git(f.repo, "diff", "--name-only", "HEAD^", "HEAD").split("\n"), versionFiles);
  assert.equal(git(f.repo, "log", "-1", "--format=%s"), "chore(release): prepare for v1.2.4");
  assert.equal(git(f.repo, "cat-file", "-t", "refs/tags/v1.2.4"), "tag");
  assert.equal(git(f.repo, "status", "--porcelain"), "");
  assert.equal(git(f.repo, "ls-files", "updater.key", "updater.key.pub", "updater.password"), "");
  assert.equal(git(f.remote, "for-each-ref"), "");
  assert.match(result.stdout, /git push --atomic origin HEAD:refs\/heads\/publish-test refs\/tags\/v1\.2\.4/);
});

test("an explicit release pushes the current branch and annotated tag to a local remote", (t) => {
  const f = fixture(t);
  const result = run(f, ["2.0.0"], "y\n");
  assert.equal(result.status, 0, result.stderr);
  assert.equal(releaseVersion("v2.0.0", f.repo), "2.0.0");
  assert.equal(git(f.remote, "rev-parse", "refs/heads/publish-test"), git(f.repo, "rev-parse", "HEAD"));
  assert.equal(git(f.remote, "rev-parse", "refs/tags/v2.0.0^{}"), git(f.repo, "rev-parse", "HEAD"));
});

test("a first release can tag the current version without an empty commit or push at EOF", (t) => {
  const f = fixture(t);
  const result = run(f, ["1.2.3"], "");
  assert.equal(result.status, 0, result.stderr);
  unchanged(f);
  assert.equal(git(f.repo, "rev-parse", "refs/tags/v1.2.3^{}"), f.head);
  assert.equal(git(f.remote, "for-each-ref"), "");
});

test("invalid versions, downgrades and extra arguments fail without mutations", (t) => {
  const f = fixture(t);
  for (const args of [["v1.2.4"], ["1.2.4-beta.1"], ["01.2.4"], ["1.2.2"], ["2.0.0", "extra"]]) {
    assert.notEqual(run(f, args).status, 0);
    unchanged(f);
    assert.equal(git(f.repo, "tag", "--list"), "");
  }
});

for (const state of ["unstaged", "staged", "untracked"]) {
  test(`a ${state} change prevents release preparation`, (t) => {
    const f = fixture(t);
    writeFileSync(join(f.repo, state === "untracked" ? "new-file.txt" : "pnpm-lock.yaml"), "uncommitted\n");
    if (state === "staged") git(f.repo, "add", "pnpm-lock.yaml");
    assert.notEqual(run(f).status, 0);
    assert.equal(git(f.repo, "rev-parse", "HEAD"), f.head);
    assert.equal(releaseVersion("v1.2.3", f.repo), "1.2.3");
    assert.equal(git(f.repo, "tag", "--list"), "");
  });
}

test("an existing local tag or detached HEAD fails before modifying versions", (t) => {
  const f = fixture(t);
  git(f.repo, "tag", "v1.2.4");
  assert.notEqual(run(f).status, 0);
  unchanged(f);
  git(f.repo, "checkout", "--quiet", "--detach");
  assert.notEqual(run(f, ["2.0.0"]).status, 0);
  unchanged(f);
});

test("an inconsistent Cargo.lock fails before writing any version files", (t) => {
  const f = fixture(t);
  const path = join(f.repo, "src-tauri/Cargo.lock");
  writeFileSync(path, readFileSync(path, "utf8").replace('version = "1.2.3"', 'version = "9.0.0"'));
  git(f.repo, "add", "src-tauri/Cargo.lock");
  git(f.repo, "commit", "--quiet", "-m", "inconsistent lock");
  const head = git(f.repo, "rev-parse", "HEAD");
  assert.notEqual(run(f).status, 0);
  unchanged(f, head);
  assert.equal(git(f.repo, "tag", "--list"), "");
});

test("a rejected remote tag also prevents the branch from being pushed", (t) => {
  const f = fixture(t);
  const hook = join(f.remote, "hooks/update");
  writeFileSync(hook, '#!/bin/sh\ncase "$1" in refs/tags/*) exit 1;; esac\nexit 0\n');
  chmodSync(hook, 0o755);
  const result = run(f, [], "y\n");
  assert.notEqual(result.status, 0);
  assert.equal(git(f.remote, "for-each-ref"), "");
  assert.equal(releaseVersion("v1.2.4", f.repo), "1.2.4");
  assert.equal(git(f.repo, "rev-parse", "refs/tags/v1.2.4^{}"), git(f.repo, "rev-parse", "HEAD"));
  assert.match(result.stdout, /推送命令:/);
});
