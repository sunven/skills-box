import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export function releaseVersion(tag, directory = root) {
  if (!/^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(tag ?? "")) throw new Error("Release tag must be a stable vX.Y.Z version.");
  const version = tag.slice(1);
  const packageVersion = JSON.parse(readFileSync(join(directory, "package.json"), "utf8")).version;
  const tauriVersion = JSON.parse(readFileSync(join(directory, "src-tauri/tauri.conf.json"), "utf8")).version;
  const cargoVersion = readFileSync(join(directory, "src-tauri/Cargo.toml"), "utf8").match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (![packageVersion, tauriVersion, cargoVersion].every((value) => value === version)) throw new Error("Release tag, package.json, Cargo.toml and tauri.conf.json versions must match.");
  return version;
}

function files(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => entry.isDirectory()
    ? files(join(directory, entry.name)) : [join(directory, entry.name)]);
}
function exactlyOne(paths, suffix) {
  const found = paths.filter((path) => path.endsWith(suffix));
  if (found.length !== 1) throw new Error(`Expected one ${suffix} artifact, found ${found.length}.`);
  return found[0];
}
function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: root, encoding: "utf8", ...options });
  if (result.status !== 0) throw new Error(`${command} failed: ${result.stderr ?? ""}`);
  return result.stdout;
}

export function prepareRelease(tag, artifacts, output, notes = "", verifier = verifyArtifact) {
  const version = releaseVersion(tag);
  mkdirSync(output, { recursive: true });
  const platforms = {};
  for (const arch of ["aarch64", "x86_64"]) {
    const directory = join(artifacts, `macos-${arch}`);
    if (!existsSync(directory)) throw new Error(`Missing ${arch} build.`);
    const paths = files(directory);
    const archive = exactlyOne(paths, ".app.tar.gz");
    const signature = `${archive}.sig`;
    if (!existsSync(signature)) throw new Error(`Missing ${arch} signature.`);
    const dmg = exactlyOne(paths, ".dmg");
    verifier(archive, signature, arch, version);
    const basename = `skills-box_${version}_${arch}`;
    for (const [from, extension] of [[archive, ".app.tar.gz"], [signature, ".app.tar.gz.sig"], [dmg, ".dmg"]]) {
      copyFileSync(from, join(output, `${basename}${extension}`));
    }
    const encoded = readFileSync(signature, "utf8").trim();
    if (!encoded || !Buffer.from(encoded, "base64").toString("utf8").startsWith("untrusted comment:")) throw new Error("Invalid updater signature encoding.");
    platforms[`darwin-${arch}`] = { url: `https://github.com/sunven/skills-box/releases/download/${tag}/${basename}.app.tar.gz`, signature: encoded };
  }
  const manifest = { version, notes, pub_date: new Date().toISOString(), platforms };
  writeFileSync(join(output, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  return manifest;
}

function verifyArtifact(archive, signature, arch, version) {
  run("cargo", ["run", "--locked", "--quiet", "--manifest-path", "src-tauri/Cargo.toml", "--example", "verify-update", "--", archive, signature]);
  const members = run("tar", ["-tzf", archive]).trim().split("\n");
  const binary = exactlyOne(members, "/Contents/MacOS/skills-box");
  const plist = exactlyOne(members, "/Contents/Info.plist");
  const executable = spawnSync("tar", ["-xOzf", archive, binary], { maxBuffer: 128 * 1024 * 1024 });
  if (executable.status !== 0) throw new Error("Cannot read the archived app binary.");
  const type = run("file", ["-b", "-"], { input: executable.stdout });
  const expected = arch === "aarch64" ? "arm64" : "x86_64";
  if (!type.includes(expected)) throw new Error(`Archive architecture differs from ${arch}.`);
  const info = run("tar", ["-xOzf", archive, plist]);
  if (!info.includes(`<key>CFBundleShortVersionString</key>\n\t<string>${version}</string>`)
    && !new RegExp(`<key>CFBundleShortVersionString</key>\\s*<string>${version.replaceAll(".", "\\.")}</string>`).test(info)) {
    throw new Error("Archived app version differs from release tag.");
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [mode, ...args] = process.argv.slice(2);
  const tag = process.env.RELEASE_TAG ?? process.env.GITHUB_REF_NAME;
  if (mode === "check-tag") console.log(`Release version validated: ${releaseVersion(tag)}`);
  else if (mode === "prepare") {
    const [artifacts, output, notesFile] = args;
    if (!artifacts || !output) throw new Error("usage: prepare-release prepare <artifacts> <output> [notes-file]");
    prepareRelease(tag, resolve(artifacts), resolve(output), notesFile ? readFileSync(notesFile, "utf8") : "");
    console.log("Both macOS update artifacts and signatures verified; latest.json is complete.");
  } else throw new Error("Expected check-tag or prepare.");
}
