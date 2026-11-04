import { randomBytes } from "node:crypto";
import { chmodSync, existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

function realDirectory(path) {
  try { return realpathSync(path); }
  catch (error) {
    if (error.code !== "ENOENT") throw error;
    return join(realDirectory(dirname(path)), basename(path));
  }
}

const root = realpathSync(resolve(dirname(fileURLToPath(import.meta.url)), ".."));
const directory = realDirectory(resolve(process.argv[2] ?? join(homedir(), ".local/share/skills-box/signing")));
if (directory === root || directory.startsWith(`${root}/`)) throw new Error("Signing keys must be stored outside the repository.");
process.umask(0o077);
mkdirSync(directory, { recursive: true, mode: 0o700 });
const key = join(directory, "updater.key");
const passwordFile = join(directory, "updater.password");
if (!existsSync(key) && !existsSync(`${key}.pub`) && !existsSync(passwordFile)) {
  const password = randomBytes(32).toString("hex");
  const result = spawnSync("pnpm", ["tauri", "signer", "generate", "--ci", "--password", password, "--write-keys", key],
    { cwd: root, encoding: "utf8", stdio: "pipe" });
  if (result.status !== 0) throw new Error("Could not generate updater key; CLI output was suppressed to protect key material.");
  writeFileSync(passwordFile, password, { mode: 0o600, flag: "wx" });
} else if (![key, `${key}.pub`, passwordFile].every(existsSync)) {
  throw new Error("Signing directory is incomplete; choose a new directory rather than overwriting existing keys.");
}
for (const path of [key, `${key}.pub`, passwordFile]) chmodSync(path, 0o600);
const configPath = join(root, "src-tauri/tauri.conf.json");
const config = JSON.parse(readFileSync(configPath, "utf8"));
const pubkey = readFileSync(`${key}.pub`, "utf8").trim();
if (config.plugins?.updater?.pubkey && config.plugins.updater.pubkey !== pubkey) {
  throw new Error("Configured public key differs; keep the existing trust root or plan an explicit key rotation.");
}
config.bundle.createUpdaterArtifacts = true;
config.plugins ??= {};
config.plugins.updater = {
  ...config.plugins.updater,
  endpoints: ["https://github.com/sunven/skills-box/releases/latest/download/latest.json"],
  pubkey,
};
writeFileSync(configPath, `${JSON.stringify(config, null, 2)}\n`);
console.log(`Updater public key configured. Private key and password remain in ${directory}.`);
