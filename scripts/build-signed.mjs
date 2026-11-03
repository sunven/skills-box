import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const directory = process.env.SKILLS_BOX_SIGNING_DIR ?? join(homedir(), ".local/share/skills-box/signing");
const result = spawnSync("pnpm", ["tauri", "build", ...process.argv.slice(2)], {
  stdio: "inherit",
  env: {
    ...process.env,
    TAURI_SIGNING_PRIVATE_KEY: readFileSync(join(directory, "updater.key"), "utf8").trim(),
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: readFileSync(join(directory, "updater.password"), "utf8").trim(),
  },
});
process.exit(result.status ?? 1);
