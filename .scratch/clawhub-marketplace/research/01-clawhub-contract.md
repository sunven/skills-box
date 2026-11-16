# ClawHub 接入事实

核查日期：2026-11-16。范围：ClawHub 官网、官方 HTTP API 文档、OpenClaw CLI 文档、`openclaw/clawhub` 源码及匿名实测。本次为两轮只读核查，未安装 CLI 或技能，未修改用户环境。实测可用不代表接口稳定性承诺。

## 结论

**ClawHub 可作为第三个 Marketplace source 接入，但安装不能泛化为 Git 安装。** ClawHub 是 OpenClaw 的版本化注册表：托管 ZIP 占多数，部分条目由 resolver 解析到 GitHub 仓库的固定 commit。官方安装器先解析 `owner/slug`，再下载 ZIP 或 codeload GitHub 内容，不是 `git clone`。应用应按 `owner/slug@解析引用` 的市场版本记录建模，使用官方 CLI 作为可选执行适配器，并以"版本固定必须实测"为硬门槛。

## 身份与目录接口

- ClawHub 是 OpenClaw Foundation 项目，站点 © 2026，源码 `openclaw/clawhub`（MIT）。它托管带 semver 版本的 `SKILL.md` 包、插件与模板，并索引 skills.sh 条目（`install.kind: "skills-sh"`）。
- 匿名读接口（实测无需 token），基址 `https://clawhub.ai`：
  - `GET /api/v1/skills?limit=200&sort=updated&cursor=…` → `{items, nextCursor}`，条目使用 `ownerHandle/slug`，带 `tags.latest`、`stats`、`latestVersion`。
  - `GET /api/v1/search?q=…&limit=…`：结果带 `install{kind, reference, sourceUrl}`、`source`、`sourceIdentity`、`trust`、`native.skill`；`category`/`topic` 参数可用但未文档化。
  - `GET /api/v1/skills/{slug}?ownerHandle=…`、`/versions`、`/versions/{version}`（文件、sha256、scan）。
  - `GET /api/v1/skills/{slug}/file?path=SKILL.md&version=…&ownerHandle=…`：10 MB 原文，200 KB 预览。
  - `GET /api/v1/download?slug=…&ownerHandle=…&version=…` → ZIP。
  - `GET /api/v1/skills/{slug}/install?ownerHandle=…` → `installKind: "archive"` + `downloadUrl`，或 `"github"` + `repo/path/commit/contentHash`（未文档化）。
  - `GET /api/v1/feeds/skills`：`{sourceRef: public-clawhub|public-github, package, version, integrity, github{repo,path,commit}}`。
- **slug 不是全局唯一**：`/skills/weather` 返回 HTTP 409，列出 9 个 owner；`ownerHandle` 只在 slug 歧义时必需。界面必须始终使用 `@owner/slug`。
- 读完接口匿名可用；写操作需要 bearer token。
- `/api/v1/skills` 是 skills-only，无 type/kind 字段；插件/模板有独立端点（`/api/v1/plugins`、`/api/v1/code-plugins`、`/api/v1/bundle-plugins`）或统一 `/api/v1/packages?family=…`。
- 排序实测支持：`updated`（默认）、`recommended`/`default`、`createdAt`/`newest`、`downloads`、`stars`/`rating`、`name`、`trending`；`installs*` 别名映射到 downloads；`created` 返回 400。
- 搜索结果 `install.kind` 实测：`clawhub`（385 条）、`skills-sh`（115 条）；`github` 已声明但该轮未观测到。resolver `installKind` 实测：`archive`、`github`。
- GitHub-backed 条目详情常见 `latestVersion: null`、版本列表为空、`/download` 404，必须通过 `/install` resolver 或 `/feeds/skills` 解析；官方 feed 抽样 1000 条中 523 条为 GitHub-backed。

## 官方安装方式

- `npm i -g clawhub`；`clawhub install @owner/slug` 安装到 `<workdir>/<dir>/<slug>`（`--dir` 默认 `skills`），并写 `<workdir>/.clawhub/lock.json`（version、installedAt、ownerHandle、sourceRef、pins；无 digest）与技能包内 `.clawhub/origin.json`（本地指纹）。
- CLI 语法：`clawhub [--workdir W] [--dir D] install <ref> [--version V] [--force] [--force-install]`；ref 支持 `@owner/slug` 或 `skills-sh:owner/repo/slug`。支持显式 owner、目标目录、`--version` 与全局 `--no-input`；**无 `--json`**、无 `--yes`；所有失败只输出纯文本并 `exit 1`；可疑技能在非交互模式下需要 `--force`。
- 完整性：archive 安装**不校验 sha256**（`/api/v1/download` 不返回 `Digest`/`ETag`）；`skills-sh:` GitHub 路径会校验 `contentHash` 并在不匹配时删除；原生 GitHub resolver 路径不校验。
- `openclaw skills install <ref>` 同样存在，ref 支持 `@owner/slug`、`skills-sh:owner/repo/slug`、`git:repo[@ref]`、`./path`，flag 有 `--version`、`--force`、`--force-install`、`--acknowledge-install-policy-warning`、`--global`、`--agent <id>`、`--as`；无 `--json`、无自定义目录、无通用 yes；记录本地计算的 sha256（非服务端验证）。因无法暂存到自定义目录，不适合本应用的发布流程。
- 包内容：`SKILL.md`、`skill-card.md`、生成的 `_meta.json`；必需 `SKILL.md`，可选 YAML frontmatter（`name`、`description`、`version`、`metadata.openclaw`），最大 50 MB。
- 元数据：semver 版本 + `latest` 标签 + changelog；仓库 MIT、发布技能 MIT-0；统计含 downloads/installs/stars/comments/versions；14 个分类/主题；发布者 handle、名称、头像、官方标记。

## 稳定性与完整性样本

- `design@1.0.0` 两次拉取 sha256 相同（`83c6b2ff…46bc`，4,242 bytes），但无文档或响应头契约保证。
- `skills-sh:anthropics/knowledge-work-plugins/design-critique` 两次返回相同 commit（`e91a134584eb798bac7fb0379358006a2b63b965`）与稳定 `contentHash`；其"版本"就是 commit，`?version=` 被忽略，无 semver 选择器。
- 同版本 archive 内容不可变**未被契约保证**；不能把重复样本一致当作长期承诺。

## 合规与限流

- 官方原文（`docs.openclaw.ai/clawhub/http-api#public-catalog-reuse`）：第三方目录可以使用公开读接口；要求缓存结果、遵守 `429`/`Retry-After`、回链 canonical 列表页 `https://clawhub.ai/<owner>/skills/<slug>`、避免暗示 ClawHub 背书；不得镜像隐藏、私密或被审核屏蔽的内容。
- 限流：3000 读/分/IP，1200 下载/分/IP。
- `robots.txt`：`Disallow: /api/`，`Allow: /v1/feeds/skills`；这是爬虫策略，不等同于 API 客户端禁令，文档未显式调和该冲突。
- 仓库 MIT；发布技能 MIT-0；无 `/terms` 页面（404），内容受 Acceptable Usage 约束。

## 与本仓库的关系

- 现有 Git Source 校验只接受公开 HTTPS Git 仓库或 GitHub `owner/repo`；ClawHub 的托管 ZIP 与 resolver 引用不能映射为 Git Source（`src-tauri/src/tasks/source.rs`）。
- 现有 skills.sh 接入把市场条目映射到 Git 安装身份（ADR-0033）；ClawHub 不适用：必须建立应用自有的市场版本记录，复用 ADR-0030 的坐标/版本/基线模型。
- 现有 SkillHub 适配依赖 CLI 的 `--json` 与可校验 lock；ClawHub CLI 无 JSON、无 digest、错误码单一，因此执行适配必须以隔离落盘校验为主，并设"版本固定实测"硬门槛。
- ClawHub 的列表接口无 kind 字段，而搜索有 `install.kind`；skills.sh 别名的过滤只能在可识别处进行，列表组成需实现前验证。

## 未验证项

- `clawhub install --version` 对 archive 与 GitHub-backed 引用是否严格生效；本轮仅确认 flag 存在。
- 可疑技能在 `--no-input` 下的确切错误形态与退出行为。
- archive 条目能否用 `/versions/{version}` 的 sha256 对安装内容做有效校验。
- `/api/v1/skills` 列表是否包含 skills.sh 别名条目。
- 429 响应是否实际携带 `Retry-After`；CLI 下载阶段是否遵守。
- `scan` 字段的语义、来源与可靠性。
- 官方 API 的长期兼容性、限流执行力度与第三方目录许可的持续有效性。

## 主要来源

- [ClawHub HTTP API](https://docs.openclaw.ai/clawhub/http-api)，含 [public-catalog-reuse](https://docs.openclaw.ai/clawhub/http-api#public-catalog-reuse)
- [ClawHub CLI](https://docs.openclaw.ai/clawhub/cli)
- [OpenClaw CLI skills](https://docs.openclaw.ai/cli/skills)
- [CLI 入口](https://github.com/openclaw/clawhub/blob/main/packages/clawhub/src/cli.ts)；[skills 命令](https://github.com/openclaw/clawhub/blob/main/packages/clawhub/src/cli/commands/skills.ts)
- [安装 resolver](https://github.com/openclaw/clawhub/blob/main/convex/lib/installResolver.ts)；[lock/origin schema](https://github.com/openclaw/clawhub/blob/main/packages/clawhub/src/schema/schemas.ts)
- [OpenClaw skills CLI 源码](https://github.com/openclaw/openclaw/blob/main/src/cli/skills-cli.ts)
- [OpenAPI](https://clawhub.ai/api/v1/openapi.json)；[robots.txt](https://clawhub.ai/robots.txt)
- 示例：[@steipete/weather 页面](https://clawhub.ai/steipete/skills/weather)、[版本 ZIP](https://clawhub.ai/api/v1/download?slug=weather&ownerHandle=steipete&version=1.0.0)
