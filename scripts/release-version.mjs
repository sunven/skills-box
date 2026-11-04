import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export function releaseVersion(tag, directory = root) {
  if (!/^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(tag ?? "")) throw new Error("Release tag must be a stable vX.Y.Z version.");
  const version = tag.slice(1);
  const packageVersion = JSON.parse(readFileSync(join(directory, "package.json"), "utf8")).version;
  const tauriVersion = JSON.parse(readFileSync(join(directory, "src-tauri/tauri.conf.json"), "utf8")).version;
  const cargoVersion = readFileSync(join(directory, "src-tauri/Cargo.toml"), "utf8").match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  const lockVersion = readFileSync(join(directory, "src-tauri/Cargo.lock"), "utf8").match(/^\[\[package\]\]\nname = "skills-box"\nversion = "([^"]+)"/m)?.[1];
  if (![packageVersion, tauriVersion, cargoVersion, lockVersion].every((value) => value === version)) throw new Error("Release tag, package.json, Cargo.toml, Cargo.lock and tauri.conf.json versions must match.");
  return version;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  console.log(`Release version validated: ${releaseVersion(process.argv[2])}`);
}
