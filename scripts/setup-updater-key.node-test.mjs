import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import assert from "node:assert/strict";

test("key setup rejects repository destinations through symlinks before creating files", (t) => {
  const temporary = mkdtempSync(join(tmpdir(), "skills-box-key-path-"));
  t.after(() => rmSync(temporary, { recursive: true, force: true }));
  const repo = join(temporary, "repo");
  mkdirSync(join(repo, "scripts"), { recursive: true });
  const privateDirectory = join(repo, "private");
  mkdirSync(privateDirectory);
  const script = join(repo, "scripts/setup-updater-key.mjs");
  copyFileSync(new URL("./setup-updater-key.mjs", import.meta.url), script);
  const alias = join(temporary, "external-alias");
  symlinkSync(privateDirectory, alias);
  for (const directory of [privateDirectory, alias, join(alias, "new-directory")]) {
    const result = spawnSync(process.execPath, [script, directory], { encoding: "utf8" });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /outside the repository/);
    assert.deepEqual(readdirSync(privateDirectory), []);
  }
  assert.equal(existsSync(join(privateDirectory, "new-directory")), false);
});

test("key setup reuses an existing external key without invoking the signer", (t) => {
  const temporary = mkdtempSync(join(tmpdir(), "skills-box-key-reuse-"));
  t.after(() => rmSync(temporary, { recursive: true, force: true }));
  const repo = join(temporary, "repo");
  mkdirSync(join(repo, "scripts"), { recursive: true });
  mkdirSync(join(repo, "src-tauri"));
  const configPath = join(repo, "src-tauri/tauri.conf.json");
  writeFileSync(configPath, JSON.stringify({ bundle: {}, plugins: {} }));
  const script = join(repo, "scripts/setup-updater-key.mjs");
  copyFileSync(new URL("./setup-updater-key.mjs", import.meta.url), script);
  const directory = join(temporary, "external");
  mkdirSync(directory);
  for (const [name, data] of [["updater.key", "fixture-private"], ["updater.key.pub", "fixture-public"], ["updater.password", "fixture-password"]]) {
    writeFileSync(join(directory, name), data);
  }
  const result = spawnSync(process.execPath, [script, directory], { encoding: "utf8", env: { ...process.env, PATH: "" } });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(readFileSync(join(directory, "updater.key"), "utf8"), "fixture-private");
  assert.equal(JSON.parse(readFileSync(configPath, "utf8")).plugins.updater.pubkey, "fixture-public");
});
