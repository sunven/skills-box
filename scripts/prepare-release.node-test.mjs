import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import assert from "node:assert/strict";
import { prepareRelease, releaseVersion } from "./prepare-release.mjs";

const version = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version;
const tag = `v${version}`;

function fixture() {
  const dir = mkdtempSync(join(tmpdir(), "skills-box-release-"));
  for (const arch of ["aarch64", "x86_64"]) {
    const target = join(dir, `macos-${arch}`); mkdirSync(target);
    writeFileSync(join(target, "skills-box.app.tar.gz"), arch);
    writeFileSync(join(target, "skills-box.app.tar.gz.sig"), Buffer.from("untrusted comment: fixture\nsignature").toString("base64"));
    writeFileSync(join(target, "skills-box.dmg"), "installer");
  }
  return { dir, output: join(dir, "output"), cleanup: () => rmSync(dir, { recursive: true, force: true }) };
}
test("release tags reject prereleases and inconsistent versions", () => {
  assert.equal(releaseVersion(tag), version);
  assert.throws(() => releaseVersion(`${tag}-beta.1`), /stable/);
  assert.throws(() => releaseVersion("v999.0.0"), /match/);
});
test("a complete release references uniquely named signed artifacts for both architectures", () => {
  const f = fixture();
  try {
    const verified = [];
    const manifest = prepareRelease(tag, f.dir, f.output, "notes", (...args) => verified.push(args));
    assert.equal(verified.length, 2);
    assert.equal(manifest.notes, "notes");
    for (const arch of ["aarch64", "x86_64"]) {
      const platform = manifest.platforms[`darwin-${arch}`];
      assert.match(platform.url, new RegExp(`${arch}\\.app\\.tar\\.gz$`));
      assert.equal(readFileSync(join(f.output, `skills-box_${version}_${arch}.app.tar.gz`), "utf8"), arch);
    }
  } finally { f.cleanup(); }
});
test("a missing architecture cannot generate latest.json", () => {
  const f = fixture();
  try {
    rmSync(join(f.dir, "macos-x86_64"), { recursive: true });
    assert.throws(() => prepareRelease(tag, f.dir, f.output, "", () => {}), /Missing x86_64/);
    assert.throws(() => readFileSync(join(f.output, "latest.json")), /ENOENT/);
  } finally { f.cleanup(); }
});
test("missing signatures and failed cryptographic verification prevent a manifest", () => {
  const f = fixture();
  try {
    assert.throws(() => prepareRelease(tag, f.dir, f.output, "", () => { throw Error("public key mismatch"); }), /public key mismatch/);
    assert.throws(() => readFileSync(join(f.output, "latest.json")), /ENOENT/);
    rmSync(join(f.dir, "macos-aarch64", "skills-box.app.tar.gz.sig"));
    assert.throws(() => prepareRelease(tag, f.dir, f.output, "", () => {}), /Missing aarch64 signature/);
  } finally { f.cleanup(); }
});
