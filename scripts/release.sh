#!/usr/bin/env bash

# 用法: ./scripts/release.sh [MAJOR.MINOR.PATCH]
# 不指定版本时自动递增 PATCH；确认后推送并触发 GitHub Actions。
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

fail() {
  printf '错误: %s\n' "$1" >&2
  exit 1
}

[[ $# -le 1 ]] || fail "用法: ./scripts/release.sh [MAJOR.MINOR.PATCH]"
command -v node >/dev/null || fail "未找到 Node.js"
BRANCH=$(git symbolic-ref --quiet --short HEAD) || fail "请先切换到发布分支"
[[ -z "$(git status --porcelain)" ]] || fail "请先提交所有更改，包括未跟踪文件"
git remote get-url origin >/dev/null || fail "请先配置 origin 远程仓库"

VERSION=$(node --input-type=module - "${1:-}" <<'NODE'
import { readFileSync } from "node:fs";
import { releaseVersion } from "./scripts/prepare-release.mjs";
const current = releaseVersion(`v${JSON.parse(readFileSync("package.json", "utf8")).version}`);
const [major, minor, patch] = current.split(".");
const version = process.argv[2] || `${major}.${minor}.${BigInt(patch) + 1n}`;
if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) {
  throw new Error("版本号必须是稳定版本 MAJOR.MINOR.PATCH，例如 0.2.0。");
}
const previous = current.split(".").map(BigInt);
const next = version.split(".").map(BigInt);
const changed = next.findIndex((value, index) => value !== previous[index]);
if (changed !== -1 && next[changed] < previous[changed]) throw new Error("发布版本不能低于当前版本。");
console.log(version);
NODE
)
TAG="v${VERSION}"
if git show-ref --verify --quiet "refs/tags/$TAG"; then
  fail "标签 $TAG 已存在"
fi

printf '准备发布 %s，分支 %s\n' "$TAG" "$BRANCH"
node --input-type=module - "$VERSION" <<'NODE'
import { readFileSync, writeFileSync } from "node:fs";
const version = process.argv[2];
const pkg = JSON.parse(readFileSync("package.json", "utf8"));
const config = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const cargo = readFileSync("src-tauri/Cargo.toml", "utf8");
const lock = readFileSync("src-tauri/Cargo.lock", "utf8");
const lockPattern = /^(\[\[package\]\]\nname = "skills-box"\nversion = ")([^"\n]+)(")/m;
if (lock.match(lockPattern)?.[2] !== pkg.version) throw new Error("Cargo.lock 中的应用版本与 package.json 不一致。");
pkg.version = version;
config.version = version;
const files = [
  ["package.json", `${JSON.stringify(pkg, null, 2)}\n`],
  ["src-tauri/tauri.conf.json", `${JSON.stringify(config, null, 2)}\n`],
  ["src-tauri/Cargo.toml", cargo.replace(/^(version\s*=\s*")[^"]+(".*)$/m, `$1${version}$2`)],
  ["src-tauri/Cargo.lock", lock.replace(lockPattern, `$1${version}$3`)],
];
for (const [path, contents] of files) writeFileSync(path, contents);
NODE

RELEASE_TAG="$TAG" node scripts/prepare-release.mjs check-tag
git add -- package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
if ! git diff --cached --quiet; then
  git commit -m "chore(release): prepare for $TAG"
fi
git tag -a "$TAG" -m "Release $TAG"

printf '\n本地发布提交和标签已准备好。推送将触发 GitHub Actions。\n'
printf '推送命令: git push --atomic origin %q %q\n' "HEAD:refs/heads/$BRANCH" "refs/tags/$TAG"
REPLY=n
read -r -p "是否现在推送？(y/N) " REPLY || REPLY=n
if [[ "$REPLY" =~ ^[Yy]$ ]]; then
  git push --atomic origin "HEAD:refs/heads/$BRANCH" "refs/tags/$TAG"
  printf '\n已推送 %s，构建进度: https://github.com/sunven/skills-box/actions\n' "$TAG"
else
  printf '\n未推送；本地提交和标签已保留，可以稍后执行上面的命令。\n'
fi
