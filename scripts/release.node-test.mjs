import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import assert from "node:assert/strict";
import { releaseVersion } from "./release-version.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const versionFiles = ["CHANGELOG.md", "package.json", "src-tauri/Cargo.lock", "src-tauri/Cargo.toml", "src-tauri/tauri.conf.json"];

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
  const bin = join(temporary, "bin");
  mkdirSync(bin);
  mkdirSync(join(repo, "scripts"), { recursive: true });
  mkdirSync(join(repo, "src-tauri"));
  for (const path of [".gitignore", "scripts/release.sh", "scripts/bump-version.sh", "scripts/release-version.mjs", "cliff.toml"]) {
    copyFileSync(join(root, path), join(repo, path));
  }
  writeFileSync(join(repo, "package.json"), JSON.stringify({ name: "skills-box", version: "1.2.3", type: "module" }, null, 2) + "\n");
  writeFileSync(join(repo, "src-tauri/tauri.conf.json"), JSON.stringify({ version: "1.2.3", plugins: { updater: { pubkey: "fixture-public-key" } } }, null, 2) + "\n");
  mkdirSync(join(repo, "src-tauri/src"));
  mkdirSync(join(repo, "src-tauri/dependency/src"), { recursive: true });
  writeFileSync(join(repo, "src-tauri/src/lib.rs"), "");
  writeFileSync(join(repo, "src-tauri/dependency/src/lib.rs"), "");
  writeFileSync(join(repo, "src-tauri/dependency/Cargo.toml"), '[package]\nname = "dependency"\nversion = "9.8.7"\n');
  writeFileSync(join(repo, "src-tauri/Cargo.toml"), '[package]\nname = "skills-box"\nversion = "1.2.3"\n\n[dependencies]\ndependency = { version = "9.8.7", path = "dependency" }\n');
  const lock = spawnSync("cargo", ["generate-lockfile", "--offline", "--manifest-path", join(repo, "src-tauri/Cargo.toml")], { encoding: "utf8" });
  assert.equal(lock.status, 0, lock.stderr);
  writeFileSync(join(repo, "pnpm-lock.yaml"), "lockfileVersion: '9.0'\n");
  for (const path of ["updater.key", "updater.key.pub", "updater.password"]) writeFileSync(join(repo, path), "ignored signing fixture");
  // Keep the optional changelog tool deterministic; Cargo and Git run for real.
  const cliff = join(bin, "git-cliff");
  writeFileSync(cliff, '#!/bin/bash\n[[ "$1" == "--tag" && "$3" == "-o" && "$4" == "CHANGELOG.md" ]] || exit 1\nprintf "# Changelog\\n\\n## %s\\n" "$2" > "$4"\n');
  chmodSync(cliff, 0o755);
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
  return { repo, remote, bin, head: git(repo, "rev-parse", "HEAD"), env: { ...process.env, PATH: `${bin}:${process.env.PATH}` } };
}

function run(f, args = [], input = "n\n") {
  return spawnSync(join(f.repo, "scripts/release.sh"), args, {
    cwd: join(f.repo, "src-tauri"), input, env: f.env, encoding: "utf8", timeout: 15000,
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
  assert.match(readFileSync(join(f.repo, "CHANGELOG.md"), "utf8"), /v1\.2\.4/);
  assert.match(git(f.repo, "log", "-1", "--format=%b"), /Update CHANGELOG/);
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

test("a first release can tag the current version and keep its changelog commit local at EOF", (t) => {
  const f = fixture(t);
  const result = run(f, ["1.2.3"], "");
  assert.equal(result.status, 0, result.stderr);
  assert.equal(releaseVersion("v1.2.3", f.repo), "1.2.3");
  assert.equal(git(f.repo, "status", "--porcelain"), "");
  assert.equal(git(f.repo, "rev-parse", "refs/tags/v1.2.3^{}"), git(f.repo, "rev-parse", "HEAD"));
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
  const lock = readFileSync(path, "utf8");
  assert.notEqual(run(f).status, 0);
  assert.equal(git(f.repo, "rev-parse", "HEAD"), head);
  assert.equal(git(f.repo, "status", "--porcelain"), "");
  assert.equal(readFileSync(path, "utf8"), lock);
  assert.equal(JSON.parse(readFileSync(join(f.repo, "package.json"), "utf8")).version, "1.2.3");
  assert.equal(git(f.repo, "tag", "--list"), "");
});

test("a failed changelog generation does not create a release commit, tag or push", (t) => {
  const f = fixture(t);
  writeFileSync(join(f.bin, "git-cliff"), "#!/bin/bash\nexit 1\n");
  assert.notEqual(run(f, [], "y\n").status, 0);
  assert.equal(git(f.repo, "rev-parse", "HEAD"), f.head);
  assert.equal(git(f.repo, "tag", "--list"), "");
  assert.equal(git(f.remote, "for-each-ref"), "");
});

test("a missing git-cliff preserves an existing manual changelog", (t) => {
  const f = fixture(t);
  rmSync(join(f.bin, "git-cliff"));
  // Supply only the tools the reference scripts need, without git-cliff.
  for (const name of ["bash", "node", "git", "cargo", "rustc", "dirname", "grep", "jq", "mv", "sed"]) {
    const location = spawnSync("/bin/sh", ["-c", `command -v ${name}`], { encoding: "utf8" });
    assert.equal(location.status, 0, location.stderr);
    symlinkSync(location.stdout.trim(), join(f.bin, name));
  }
  f.env.PATH = f.bin;
  const changelog = "# Changelog\n\nManual release notes.\n";
  writeFileSync(join(f.repo, "CHANGELOG.md"), changelog);
  git(f.repo, "add", "CHANGELOG.md");
  git(f.repo, "commit", "--quiet", "-m", "docs: add manual release notes");
  const result = run(f);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /跳过 CHANGELOG/);
  assert.equal(readFileSync(join(f.repo, "CHANGELOG.md"), "utf8"), changelog);
  assert.equal(releaseVersion("v1.2.4", f.repo), "1.2.4");
  assert.equal(git(f.repo, "status", "--porcelain"), "");
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
