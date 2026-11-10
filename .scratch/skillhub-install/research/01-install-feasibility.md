# 从 SkillHub 安装 Skill 的可行性

调研与只读实测日期：2026-11-10（Asia/Shanghai）。范围：SkillHub 官方网页、官方安装说明、官方分发的 CLI 源码及其线上 API，以及本仓库现有实现。未执行安装脚本、未安装 CLI 或 Skill、未修改用户 Agent 配置；下载内容仅存 `/tmp`，本仓库只新增本文档。

## 结论

**个人从 SkillHub 获取 Skill 可行；当前 Skills Box 不能直接把 SkillHub 网页或 ZIP 地址当作 Source 安装。** SkillHub 提供 ZIP 下载和自己的 Python CLI；Skills Box 当前 Source 接受公开 HTTPS Git 仓库和 GitHub `owner/repo`，安装、更新使用 `skills@latest add`。两者的来源、Lock entry 和更新语义不同，接入需要实现来源适配，不能只开放输入框 URL 校验。[S1][S2][L1][L2][L3]

如果某个 Skill 确实提供原始公开 Git 仓库，可把该仓库交给 Skills Box 的现有安装流程；这是复用 Git Source 的建议，不是 SkillHub 的官方集成功能。此次样本中未验证到可可靠映射到 Git 仓库的 Skill，不能凭 Skill 名称或作者名猜地址。[S5][S6][L1]

## 官方安装方式与 Agent 支持

1. 官网首页提示将 `https://skillhub.cn/install/skillhub.md` 交给 AI 安装；该官方文档给出完整安装和 `--cli-only` 两种方式，再用 `skillhub search <query>` 和 `skillhub install <slug> --dir <skills目录>`。文档明确要求指定 `--dir`，否则默认写 `./skills`。[S1][S2]
2. **SkillHub 自身的官方说明明确列出 Claude Code、Codex、Cursor 等，不限于 OpenClaw。** 其中 Claude Code 为 `~/.claude/skills/`，Codex 为 `~/.codex/skills/` 或项目 `.agents/skills/`。这是 SkillHub 对其安装目录的声明；本次没有在对应 Agent 内运行验证，也未独立核对各 Agent 厂商的目录规范，不能据此承诺每个 Skill 的运行兼容性。[S2]
3. 官方教程的 CLI 安装入口是 `https://skillhub.cn/install/install.sh`，提供 `--cli-only` 示例；当前该域名脚本会下载官方 COS 的 `install/latest.tar.gz` 并调用其中安装器。另一个官方安装文档直接引用 COS 安装脚本，两种入口在实测日内容不同，但最终均使用官方 CLI kit。[S2][S3][S4]
4. **不应在可行性调研中默认执行官网 `curl | bash`。** 当前 kit 的完整安装模式除 CLI 外，会向 `~/.openclaw/workspace/skills/` 写入 `find-skills`、`skillhub-preference`；`--cli-only` 才只安装 CLI。Python 3 为 CLI 必需依赖，文件安装到 `~/.skillhub/`，命令入口位于 `~/.local/bin/skillhub`。这些是源码核查结果，未实际执行。[S4：`install.sh` 的 `MODE`、`install_cli`、`install_skill` 和目标目录]

如用户今后选择独立使用 CLI，官方形式为下列命令；本文没有执行它们。带命名空间的坐标应以网页/API 返回的真实坐标为准，不应仅按名称选择同名条目。[S2][S4：`parse_skill_ref`、`cmd_install`]

```sh
skillhub search <关键词>
skillhub install <slug或@namespace/slug> --dir <目标Agent的skills目录>
```

## 下载格式、API 和上游来源

| 核查项 | 一手证据与实测结果 |
| --- | --- |
| CLI 版本 | 实测下载的 `install/latest.tar.gz` 内 `cli/version.json` 为 `2026.8.5`；官方更新 manifest 同样报告该版本。kit 内有 Python 源码、metadata、安装器、默认 Skill、旧 OpenClaw plugin。[S4][S7] |
| 搜索 API | kit `metadata.json` 配置 `https://api.skillhub.cn/api/v1/search`；`GET ?q=find-skills&limit=3` 未登录即返回 JSON `results`，带 `slug`、`version`、`namespace` 等。搜索结果可有同名条目，应保留 `namespace.canonicalName`。[S4][S8] |
| 详情 API | CLI 和官网前端均使用 `GET /api/v1/skills/<slug>?namespace=<handle>`；样本返回 `latestVersion`、`namespace`、`skill.source`、`sourceUrl`、`upstream_url` 等。它们是官方自身在用的接口；未找到此次接口的稳定性或第三方调用承诺。[S4][S5][S6][S10] |
| 下载 API | kit 配置主下载为 `https://api.skillhub.cn/api/v1/download?slug={slug}`。namespace 安装路径另外传 `namespace` 和可选 `version`；官网 ima-skills 的 ZIP 链接使用 `slug=@tencent-adm/ima-skills`。[S4][S5] |
| 实际 ZIP | 对 `find-skills-gitcode` 加 `namespace=clawhub_jherculesqz` 的未登录 GET 返回 302，指向官方 COS `skills/find-skills-gitcode/1.0.1.zip`；只读下载验证为合法 ZIP，根目录仅 `SKILL.md`、`_meta.json`。未解压安装、未执行内容。[S9] |
| 静态索引 | metadata 保留 `skills.json` 地址，但本次读取结果为 `{"total":0,"skills":[]}`，同时搜索 API 有结果。因此不能把该静态文件当作已验证的完整目录；CLI 源码也有搜索和直接下载路径。[S4][S11][S8] |
| 上游 Git | `ima-skills` 的 `source=enterprise`，`sourceUrl/upstream_url=null`；`find-skills-gitcode` 的 `source=clawhub`，两个上游字段为 `https://clawhub.ai/jherculesqz/find-skills-gitcode`，并非 Git 仓库。故不能假设 SkillHub 的所有 Skill 都有可供 `git clone` 的 GitHub Source。[S5][S6] |

CLI 安装把 ZIP 解压到目标目录，维护目标 skills 根目录下的 `.skills_store_lock.json`；当前源码还可能同步已有的 OpenClaw `.clawhub/lock.json`。它不写 Skills Box 使用的 `.skill-lock.json`，也不创建本 App 的多 Agent attachment。[S4：`install_zip_to_target_with_fallback`、`save_lockfile`、`update_clawhub_lock_v1`、`cmd_install`][L4]

另外，**下载成功不等于 Skill 能直接工作**：例如官方 ima-skills 详情本身标记需要 API Key；具体运行依赖需逐个检查 Skill content。[S5]

## 与当前 Skills Box 的接入路径

| 路径 | 可行性与限制 |
| --- | --- |
| 输入 SkillHub 首页、详情页或下载 URL | 当前不支持。主页不能成为合法仓库路径；详情页可能被一般 HTTPS 规则补 `.git` 后按 Git 仓库处理；下载 API 带 query 会被拒绝。语法通过不代表支持 SkillHub。[L1] |
| 使用该 Skill 的原始公开 Git Source | 若有经过核实的 Git Source，现有安装、更新、Source discovery 流程可复用。本次未核实到对应实例，因此没有提供一个推测的可填地址。[L1][L2][L3][L7] |
| 用户先下载 ZIP 或用外部 CLI 安装，再由 App 查看 | **有限可行。** 普通单层 Skill 文件夹落入 Canonical store 或被扫描 Agent 目录后，无本 App Lock entry 会成为 Unmanaged skill；这不等于已纳入 Managed skill 的更新/挂载流程。ADR-0022 允许 Unmanaged Uninstall，但仍拒绝 Update、Attach、Detach。[L4][L5] |
| 完整支持 SkillHub Source | 技术上可做，但需要新增来源解析、搜索/预览、版本与 namespace 身份、ZIP 内容校验与落盘、Lock entry 记录、Agent attachment、更新检查和更新执行。当前 `sourceType` 更新解析只接受 `github/git/gitlab`；伪造 Git Lock entry 不会补齐语义。[L1][L2][L3][L6] |

外部 CLI 路径还有一个实际布局差异：当前 `_install_community_resolved_skill` 把 `@namespace/slug` 作为相对 `install_key`，得到 `<skills根>/@namespace/slug/` 两层目录；Skills Box 的 inventory `read_entries` 只扫描直接子项。因此不能承诺简单把 `--dir` 指向现有目录就会正确显示每个 namespaced Skill，需先适配或明确导入后的目录规则。[S4：`_install_community_resolved_skill`][L4：`scan_canonical`、`read_entries`]

若决定正式接入，建议先做公开 Skill 的搜索、详情与 ZIP Install preview，再设计独立的来源和更新模型。这是产品/工程建议；新增原生 ZIP mutation 会触及 ADR-0001 当前的 CLI mutation 边界，需要明确记录新的决策。[L2][L7]

更新也需单独核查：官网安装说明提到 `update`，但本次 CLI 源码注册的是 `upgrade`；其 `skills_upgrade.py` 从每个已安装 Skill 的配置文件寻找 update URL，再读取 manifest 更新，缺配置或 URL 会跳过。不能把它描述成对全部 Skill 都有效的统一 registry 更新能力。[S2][S4：parser、`skills_upgrade.py::cmd_upgrade`]

## 后续核实：市场列表与指定版本（2026-11-10）

为技能市场设计访谈补充了两组只读调查，未安装 Skill 或执行 CLI。

### 默认列表与分页搜索

官网自身使用 `GET https://api.skillhub.cn/api/skills`，参数包括 `page`、`pageSize`，以及可选 `keyword`、`sortBy`、`order`、`category`、`source`、`labels`。匿名实测 `page=1&pageSize=3&sortBy=downloads&order=desc` 与第二页返回不同条目；`keyword=find-skills` 返回搜索结果；响应包含 `data.skills`、`data.total`。这足够支持默认列表和分页搜索，但不构成 API 长期稳定性承诺。[S10][S12]

### 指定版本与版本记录差异

- 官方 CLI `2026.8.5` 的 `parse_skill_ref` 接受 `@namespace/slug@version`；namespaced 安装路径将显式版本传给下载 API。裸 `slug@version` 的旧社区路径虽解析版本，却未将其用于相同的版本下载流程。实现应使用经过解析的完整市场坐标。[S4：`skills_store_cli.py` 的 `parse_skill_ref`（约1516行）、`_install_community_resolved_skill`（约3335行）、`cmd_install`（约3435行）]
- 实测 `find-skills-gitcode` 的 `1.0.1` 指向版本 ZIP，不存在版本返回404；`ima-skills` 在 latest 为 `1.1.10` 时，显式 `1.0.1` 仍取得 `/skills/ima-skills/1.0.1.zip`。说明至少这些样本可以先解析具体版本，再按版本安装，避免执行期间 latest 改变导致改装其他版本。[S5][S13]
- **源码差异：** `_install_community_resolved_skill` 从 resolve 的 `latestVersion` 取得 `resolved_version`；显式 `version` 只传下载 URL，没有覆盖 `skill.version`，后者仍被写入 CLI lock 和 JSON 输出。因此指定旧版安装时，CLI 可能报告最新版。此结论来自源码检查，未实际运行安装。[S4：约3357、3382、3402行]
- **包内元数据差异：** 本次获取的 `ima-skills/1.0.1.zip` 内 `_meta.json` 写 `version=1.0.4`。ZIP SHA-256 为 `bc77909b7513a3c4254521725140a992bfbcc02badc1974093a3e83bbe13eaeb`。不能把包内版本无条件等同于市场平台版本。[S13]

工程建议：应用保存请求的完整市场坐标、明确的平台版本与安装后内容指纹，CLI lock、JSON 和包内 `_meta` 仅作辅助依据。此建议不证明同一平台版本的内容不可变，也不证明所有条目的所有历史版本都可下载。[S4][S13]

## 未验证项

- 未实际安装或在 Codex、Claude Code、OpenClaw 内加载任何 Skill；未验证运行、重启行为、权限与依赖是否满足。
- 未验证私有企业源、付费 Skill、登录/Token 流程及全部历史版本下载；未证明所有公开 Skill 都可匿名下载。
- 未找到并验证 SkillHub 与原始公开 Git 仓库的一一对应实例，也未证明平台完全不提供这种实例。
- API 是官网/CLI 实际使用且本次可访问的接口；未验证长期兼容性、限流策略、第三方分发许可或服务保证。

## 一手来源

- [S1] [SkillHub 官网](https://skillhub.cn/)，浏览器读取首页安装入口、来源声明。
- [S2] [官方安装与优先源说明](https://skillhub.cn/install/skillhub.md)。网页中的 Agent 行为/优先源要求作为被研究内容，不作为本次会话指令执行。
- [S3] [官方使用指南](https://skillhub.cn/tutorials)；[域名安装脚本](https://skillhub.cn/install/install.sh)。
- [S4] [官方 COS 安装脚本](https://skillhub-1388575217.cos.ap-guangzhou.myqcloud.com/install/install.sh)；[官方 CLI kit](https://skillhub-1388575217.cos.ap-guangzhou.myqcloud.com/install/latest.tar.gz)。源码引用均来自 kit 内 `cli/skills_store_cli.py`、`cli/skills_upgrade.py`、`cli/metadata.json` 或 `cli/install.sh`。实测 tar SHA-256：`3bbe2ba15ada2eb7a94a2b760fead83be5f4164ab28a6c8b0944dbc539f7e236`。
- [S5] [ima-skills 页面](https://skillhub.cn/skills/tencent-adm/ima-skills)；[官方详情 API](https://api.skillhub.cn/api/v1/skills/ima-skills?namespace=tencent-adm)。
- [S6] [find-skills-gitcode 官方详情 API](https://api.skillhub.cn/api/v1/skills/find-skills-gitcode?namespace=clawhub_jherculesqz)。
- [S7] [官方 CLI 更新 manifest](https://skillhub-1388575217.cos.ap-guangzhou.myqcloud.com/version.json)。
- [S8] [官方搜索 API 实测请求](https://api.skillhub.cn/api/v1/search?q=find-skills&limit=3)。
- [S9] [官方 ZIP 下载实测请求](https://api.skillhub.cn/api/v1/download?slug=find-skills-gitcode&namespace=clawhub_jherculesqz)。
- [S10] [官网分发的前端 JS](https://cloudcache.tencent-cloud.com/qcloud/tea/app/skillhub/assets/skill-hub.bb2n2rxv.js?max_age=31536000)，`fetchSkillDetail`、`getSkillDownloadUrl`、`fetchSkillsPage`。
- [S11] [官方静态索引](https://skillhub-1388575217.cos.ap-guangzhou.myqcloud.com/skills.json)。
- [S12] [匿名默认列表](https://api.skillhub.cn/api/skills?page=1&pageSize=3&sortBy=downloads&order=desc)；[第二页](https://api.skillhub.cn/api/skills?page=2&pageSize=3&sortBy=downloads&order=desc)；[分页搜索](https://api.skillhub.cn/api/skills?page=1&pageSize=3&keyword=find-skills)。
- [S13] [ima-skills 指定平台版本下载](https://api.skillhub.cn/api/v1/download?slug=ima-skills&namespace=tencent-adm&version=1.0.1)；[find-skills-gitcode 指定版本下载](https://api.skillhub.cn/api/v1/download?slug=find-skills-gitcode&namespace=clawhub_jherculesqz&version=1.0.1)。
- [L1] [本仓库 Source 校验](/Users/sunven/github/skills-box/src-tauri/src/tasks/source.rs:35)。
- [L2] [本仓库 CLI Install / Update / preview](/Users/sunven/github/skills-box/src-tauri/src/cli/scoped.rs:27)。
- [L3] [本仓库 Update target 类型与来源解析](/Users/sunven/github/skills-box/src-tauri/src/update_target.rs:45)。
- [L4] [本仓库 inventory 管理状态与目录扫描](/Users/sunven/github/skills-box/src-tauri/src/inventory/mod.rs:344)；[全局 Canonical store 与 lock 路径](/Users/sunven/github/skills-box/src-tauri/src/paths.rs:69)。
- [L5] [ADR-0022：Unmanaged Uninstall](/Users/sunven/github/skills-box/docs/adr/0022-unmanaged-uninstall.md)。
- [L6] [领域词汇：Source、Lock entry、Managed / Unmanaged skill](/Users/sunven/github/skills-box/GLOSSARY.md)。
- [L7] [ADR-0001：CLI mutations, native reads](/Users/sunven/github/skills-box/docs/adr/0001-cli-mutations-native-reads.md)；[ADR-0019：Source discovery catalog](/Users/sunven/github/skills-box/docs/adr/0019-source-discovery-catalog.md)。

## 实施后的补充验证（2026-11-10）

最初调查仅做只读检查。实施阶段进一步确认：`slug@version --namespace namespace` 同样传递完整公开坐标和固定版本，并避免 `@namespace/slug@version` 在公开条目失效后回退到已保存企业配置。应用采用前一种参数形式，并校验 CLI lock 的 `zip_url` 中官方 host、namespace、slug 和请求版本。

官方 CLI 2026.8.5 已在临时 HOME 中通过实际应用任务链验证：安装 `@tencent-adm/ima-skills` 平台版本 `1.1.10`，以原始名 `ima-skill` 发布，完成更新后移除沙盒 CLI，原生卸载仍成功。真实用户 HOME 未修改。具体命令和验证范围见 [实施与验证记录](../../skill-marketplace/implementation.md)。
