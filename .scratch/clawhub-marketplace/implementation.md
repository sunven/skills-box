# ClawHub 市场来源实施与验收

日期：2026-11-16。范围按工单 01–10 记录：01–06 交付，07–09 按门槛停止。已确认规格见 [spec.md](spec.md)，上游契约与 CLI 门槛见 [research](research/)。

## 环境

| 项 | 值 |
| --- | --- |
| 日期 | 2026-11-16（Asia/Shanghai） |
| 系统 | macOS 27.2.1（Darwin 27.2.0），arm64 |
| Node / npm / pnpm | v24.15.0 / 11.12.1 / 11.10.0 |
| Rust | rustc 1.98.1，cargo 1.98.1 |
| ClawHub CLI | `clawhub@0.26.0`（build `123d0f6c`），仅用于工单 02 的临时 HOME/prefix 门槛验证；应用不安装、不调用该 CLI |
| 构建目录 | `CARGO_TARGET_DIR=/Users/sunven/cargo-target-skills-box` |
| 网络 | 可用；ClawHub 仅匿名只读请求，保持低请求量 |

## 交付与暂停范围

已交付：

- **01 来源身份**：`MarketplaceSource::Clawhub`、`owner/slug` 坐标与来源 URL；市场记录保存来源、解析引用与 ClawHub 诊断字段；读模型、更新检查分派、卸载、残留记录清理、禁止复制到项目、批量更新引导均按记录来源处理，ClawHub 与 SkillHub 一样归类为原生市场来源。证据：`marketplace/records.rs`、`marketplace/updates.rs`、`read_model.rs`、`tasks/{uninstall,stale_records,copy_to_project,batch}.rs` 中的 ClawHub 测试。
- **03 发布预重构**：`tasks/publication.rs` 的 `MarketplaceInstallation` 计划契约与来源无关；以假市场计划验证备份、规范目录替换、记录写入、基线、Agent 挂载和事后校验（`source_neutral_publication_publishes_a_fake_marketplace_plan` 等）。SkillHub 行为与文案不变。
- **04 目录列表**：应用内第三个来源；三种文档化排序、cursor「加载更多」、`@owner/slug` 卡片、ClawHub 指标、可选官方标记；无伪造总数；错误保留上次成功列表并标注旧排序，429 单独状态与退避；来源选择持久化、会话内位置恢复。证据：`marketplace/clawhub/{list,client}.rs`、`src/components/clawhub-page.tsx`、`src/App.clawhub.test.tsx`。
- **05 搜索**：保留上游顺序，提示返回条数与上游上限；按 `install.kind` 过滤 skills.sh 别名并计数；空结果、错误、旧结果与重试均限定在 ClawHub 来源内。证据：`marketplace/clawhub/search.rs`、`src/App.clawhub.test.tsx`。
- **06 详情**：latest 默认、历史版本切换；概述、文件树、许可证与 changelog 固定同一版本；只读 Markdown/文本/图片预览及大小限制；缺失详情/版本/文件（含单文件预览失败）显式报错、重试与 canonical 回链；archive 条目显示所选 semver 解析引用，GitHub-backed 条目在无版本列表时用一次 resolver 调用展示 `repo@commit:path` 诊断引用（安装通道暂停，无安装入口）；skills.sh 别名识别后引导切换来源；不解析、不展示 `scan`。证据：`marketplace/clawhub/detail.rs`、`src/components/clawhub-detail-page.tsx`、`src/App.clawhub-detail.test.tsx`。

按工单 02 门槛停止，未实现：

- **07 / 08 / 09** 关闭为 wontfix。`clawhub@0.26.0` 能按 `--version` 固定 archive 版本，但无法把 GitHub-backed 条目固定到调用方指定的 commit（`--version <commit>` → `Version not found`，无其他固定参数）；抽样 GitHub-backed 条目为待扫描可疑状态，非交互安装需要规格禁止的 `--force`。按规格第 20 条与 [ADR-0034](../../docs/adr/0034-clawhub-versioned-registry-identity.md)，整个 CLI 安装通道暂停，不静默收窄为仅 archive。原生 HTTP+ZIP 适配器需新 ADR 单独评估。完整证据：[02-clawhub-cli-feasibility.md](research/02-clawhub-cli-feasibility.md)。产品代码没有安装/更新适配器、没有 ClawHub 设置页、没有凭据存储。

## 合规、缓存、限流与隐私验证

| 检查 | 结果与证据 |
| --- | --- |
| 匿名只读 | 应用只发送匿名 GET（`marketplace/clawhub/client.rs`），无账户、令牌、Cookie 或授权流程；产品代码不调用 `clawhub` CLI；无 ClawHub 设置或凭据文件 |
| 代理 | 客户端显式调用 `.no_proxy()`，请求直连 ClawHub，不继承宿主系统/环境代理设置（spec R3-Q4）；应用不配置账户、令牌或应用级代理 |
| 会话缓存 | 仅进程内存、最多 12 条、按最近最少使用淘汰、TTL 取 `Cache-Control` 且上限 300 秒；`no-store`/`no-cache`/`max-age=0` 不缓存。测试：`cache_lifetime_obeys_cache_control_and_is_bounded`、`cache_is_bounded_and_never_serves_expired_entries`、`cache_evicts_the_least_recently_used_entry_not_the_first_key`。2026-11-16 实测目录响应头 `cache-control: public, max-age=0`，因此本次运行实际不产生目录缓存复用 |
| 不落盘/无离线 | ClawHub 模块无文件写入；目录、详情和文件内容只存在于请求/内存。跨重启仅持久化来源选择（localStorage），不持久化目录或内容，不存在离线浏览。市场记录文件只承载安装/卸载等管理状态，不用于目录或内容缓存；安装暂停期间无法产生 ClawHub 记录 |
| 429 / `Retry-After` | `retry_after` 支持秒数与 HTTP 日期、限定 1–300 秒并有单元测试；429 设置共享冷却，后续请求以 `marketplace.rate_limited` 快速失败，错误携带 `retryAfterSeconds`（`rate_limit_errors_carry_the_server_retry_window`）；列表与搜索各有前端限流状态与退避测试。共享冷却的到期行为由代码检查确认，未用 Rust 层 mock HTTP 服务器直接测试。**本次验收与工单 02 均未触发真实上游 429**；工单 02 仅用本地 mock 观察 CLI 的 429 行为 |
| 文档限流 | 读限制 3000 次/分/IP 写入限流错误提示；下载限制 1200 次/分/IP 只存在于规格与调研，因为应用没有下载通道。未做接近阈值的压力验证，避免对上游制造流量 |
| Canonical 回链 | `Coordinate::url()` 固定派生 `https://clawhub.ai/<owner>/skills/<slug>`；列表/详情使用该地址，官方列表入口为 `https://clawhub.ai/skills`。测试：`coordinates_parse_display_and_link_per_source`、`parses_live_shaped_items_and_keeps_missing_fields_missing`、live 列表与详情测试，以及前端「ClawHub 页面/ClawHub 官网」断言 |
| 不暗示背书 | 界面只使用事实性文案（「来源 · ClawHub」「收录于 ClawHub」）和官网回链，没有合作、认证或背书表述；「官方」徽标只在条目自身 `official`/`isOfficial` 为真时显示，不代表 ClawHub 对应用背书 |
| 不镜像隐藏/私密/审核屏蔽内容 | 详情 404 映射为「条目不存在或不可公开访问」并给官网入口，不缓存缺失结果、不解析 `moderation`/`security`。测试：详情缺失分支（`detail()` 的 404 → `clawhub_detail_missing`）、`parses_live_detail_and_never_reads_scan_fields`（序列化不含 `moderation`/`security`）及前端缺失详情测试。未找到可用的线上被屏蔽条目 fixture，因此该分支只有构造数据测试 |
| 失败与来源隔离 | 列表失败保留上次成功页与旧排序；搜索失败保留旧关键词结果；详情/版本刷新失败保留同版本旧内容并标注失败；无旧结果时显示错误、重试与官网入口。前端测试覆盖 `App.clawhub.test.tsx` 的旧结果/加载更多/空结果/限流用例和 `App.clawhub-detail.test.tsx` 的缺失/刷新失败用例；错误文案与查询键均按来源命名，未出现跨来源结果或错误 |
| 扫描字段 | Rust 解析不读取 `scan`/`moderation`/`security`；前端无扫描展示。测试同上，另有详情测试断言页面不含 `scan`/`扫描` 文案 |
| skills.sh 别名 | 搜索按 `install.kind` 过滤 `skills-sh`，其他已知非原生 kind 排除并计数；缺失/无效 kind 且带 `native.skill` 的条目保留为原生，否则排除并计数。列表接口无 kind 字段，无法识别，按规格回退为照常展示、详情时用一次 exact 搜索识别并引导切换来源。测试：`preserves_upstream_order_and_filters_skills_sh_aliases`、`missing_or_invalid_install_kind_keeps_native_rows_and_skips_others`、live 搜索、`classifies_exact_skills_sh_aliases_without_matching_other_owners`、live 详情与前端别名重定向测试 |

## 验收条件 1–13

| # | 结果 | 证据 / 例外原因 |
| --- | --- | --- |
| 1 | **PASS** | 前端 `defaults to SkillHub and restores ClawHub from the remembered source after restart`、`keeps per-source session state and never lets the expert-pack tab rewrite the source`、`restores the search query and scroll position within the session`；专家包页签不改写来源 |
| 2 | **PASS** | 前端 `exposes exactly the three documented sorts and requests them in upstream order`、`appends the next cursor page without duplicates or reordering and shows no fake totals`、`keeps the first page when loading more fails`；Rust live `clawhub_live_catalog_lists_each_documented_sort` 在实现时遍历三种排序并验证 cursor 页（复验时上游超时，见回归表）；无总数/页码，分类只作标签 |
| 3 | **PASS** | Rust `preserves_upstream_order_and_filters_skills_sh_aliases`、`missing_or_invalid_install_kind_keeps_native_rows_and_skips_others`、live `clawhub_live_search_maps_fields_and_filters_aliases`（复验时上游超时）；前端 `searches in upstream order, excludes skills.sh aliases and states the returned limit`、`states that a full upstream limit is a limited result set, never a full search`、`identifies a skills.sh alias and redirects to the skills.sh source without an install affordance` |
| 4 | **PASS** | Rust `parses_live_shaped_items_and_keeps_missing_fields_missing`、`keeps_missing_fields_missing_and_accepts_zero_metrics`；前端 `renders available ClawHub fields and marks missing metrics as not provided instead of zero` 与详情 `opens the latest version from a list card with ClawHub identity, metrics, license and the paused-install note`；许可证缺失标注为「未提供」，指标文案固定 ClawHub 口径 |
| 5 | **PASS** | 前端 `keeps SKILL.md, changelog and the file tree on the selected version when switching`（许可证/changelog 随所选版本）、`never labels a delayed old-version response as the newly selected version`、`shows explicit missing detail and missing version states with retry and the canonical link`、`shows the canonical link and retry when one file preview fails to load`、`states explicitly when a version has no SKILL.md or no files at all`；Rust live `clawhub_live_detail_versions_files_and_alias_contract` 在实现时通过（复验时上游超时）；不展示扫描结论 |
| 6 | **EXCEPTION** | 安装执行未实现（工单 02 NO-GO），未在隔离 Global/Project 中做应用内安装。门槛只证明 archive 固定版本与落盘内容可核验 |
| 7 | **EXCEPTION** | GitHub-backed 显式引用固定未通过硬门槛，安装通道暂停并记录结论；archive 固定版本与 GitHub `contentHash` 复核作为门槛证据，不声称应用内固定安装可用 |
| 8 | **EXCEPTION** | 没有安装入口，因此「CLI 缺失时安装被阻止并给重检」未实现；门槛已证明可疑技能在非交互、无 `--force` 时退出 1 且不落盘。浏览/详情不依赖 CLI（全部浏览测试在无 CLI 环境通过） |
| 9 | **EXCEPTION** | 安装期同名冲突、Unmanaged、Copy-only、Stale lock entry 的实际流程无法触发。身份与原生分类在模型层测试：`clawhub_marketplace_uninstall_is_native_and_previews_without_a_cli`、`clawhub_records_are_native_stale_records_with_blocked_restore_and_native_cleanup`、`clawhub_marketplace_skills_cannot_be_copied_to_a_project`、`clawhub_marketplace_updates_are_guided_to_single_or_update_all`、记录往返/损坏测试 |
| 10 | **PASS** | 列表：`retains the last successful list under its old sort and retries without switching sources`、`shows error, retry and the official link with no prior result and isolates other sources`、`shows a distinct rate-limit state and backs off retry`；搜索：`retains the last successful search under its old keyword, then retries without mixing sources`、`shows error, retry and the official link for a search with no prior result`、`shows a distinct rate-limit state for search and backs off retry`；详情：`keeps the last successful same-version content and marks it stale when a refresh fails` 与两个缺失状态测试。Rust 侧 `retry_after_accepts_seconds_and_http_dates_with_bounds`、`rate_limit_errors_carry_the_server_retry_window`。真实 429 未触发 |
| 11 | **EXCEPTION** | 无 ClawHub 安装记录可创建，引用比较与更新执行未实现。现有证据：ClawHub 记录在更新检查中返回 `Unknown/Uncheckable`、不会查询 SkillHub、不消耗 SkillHub 来源预算（`clawhub_records_never_reach_the_skillhub_provider_and_report_unknown`、`clawhub_rows_do_not_consume_the_skillhub_source_budget`）；本地管理沿用既有流程 |
| 12 | **PASS** | 全量 Rust 与前端套件通过；SkillHub、skills.sh、Git 的目录/详情/安装规则、更新检查、卸载与批量流程均在套件内。实时只读 smoke 首次各出现一次上游数据波动，重跑通过（见下） |
| 13 | **EXCEPTION** | 工单 02 的真实 `clawhub@0.26.0` 验证使用隔离 HOME、临时 npm prefix 与临时 workdir，但没有注册 Project；安装暂停后应用内也没有可运行的真实 CLI 安装/更新。因此「隔离 HOME 与注册 Project 中的真实安装/更新」没有发生，不能记为 PASS。证据见 [02-clawhub-cli-feasibility.md](research/02-clawhub-cli-feasibility.md) |

## 回归与测试结果

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| 全量 Rust 套件（含全部集成测试二进制，跳过两项宿主敏感测试） | 通过：820 项通过、18 项默认忽略、2 项按名称跳过、0 失败；命令 `cargo test -- --skip agents::tests::detects_home_roots_in_catalog_order --skip one_hundred_skill_fixture_meets_the_inventory_budget` |
| ClawHub ignored live 测试（列表/搜索/详情、版本与文件） | 2026-11-16 实现时 3 项全部通过（17.22 秒）；`cargo test --lib marketplace::clawhub -- --ignored --nocapture`；约 21 次匿名 GET，无 429、无重试 |
| ClawHub ignored live 测试（代码审查复验） | **未通过**：3 项均因上游慢响应在客户端 30 秒超时内被截断（`error decoding response body`）。同一时段 `curl` 对 `sort=updated/downloads` 也在 40 秒超时并截断，`sort=recommended` 单页耗时约 22 秒。属于当时上游/网络状态，不计入确定性回归；未据此修改产品逻辑 |
| SkillHub ignored live smoke（目录、分类/排序、API Key 过滤、版本文件、详情） | 首次 4/5：`skillhub_live_sorted_catalogs` 在「按首条名称搜索应包含该坐标」断言失败；单独立即重跑通过。该文件相对 main 未修改，属于上游目录数据波动 |
| skills.sh ignored live smoke | 首次失败：某个榜单条目在当次响应中触发「无效的 skills.sh 条目标识」；立即重跑通过。该文件相对 main 未修改，属于上游数据波动 |
| 专家包 ignored live smoke | 通过 |
| `pnpm test` | 49 个测试文件、517 项全部通过 |
| `pnpm exec tsc --noEmit` | 通过 |

未运行的可选真实 CLI 测试及原因：

- `sandbox_real_cli.rs` 的 4 项、`stale_restore_real_cli.rs` 的 1 项、`generic_git_sources.rs` 的 CNB 项需要网络下载 `skills@latest`（本机无该外部 CLI），且均为显式 opt-in。
- `skillhub_marketplace_real_cli.rs` 的 2 项需要官方 SkillHub CLI kit 与 `SKILLS_BOX_TEST_SKILLHUB_KIT`（本次未设置，未提供 kit）。
- 这些测试都不涉及 ClawHub；ClawHub 安装/更新已暂停，没有新的真实安装路径可验证。未声称真实 CLI 安装或更新通过。

## 偏差与限制

- **canonical URL 为派生值**：记录只保存坐标、解析引用与诊断字段，`source_url` 在投影时由坐标派生，不单独持久化；上游若改变页面路径需改代码。
- **列表无法识别 skills.sh 别名**：`/api/v1/skills` 无 `install.kind`（门槛 128 条抽样与本轮 50 条抽样均无）。列表会照常展示别名条目，只有详情阶段的一次 exact 搜索能识别并引导；不做逐条 resolver 调用。
- **官方徽标未在线上观测到**：本轮 50 条线上列表样本既无 `official` 也无 `isOfficial`；解析器支持二者，界面仅在为真时显示，不能视为线上字段已存在。
- **GitHub-backed 详情无版本列表**：稀疏条目（如 `@aws/agents-build`）的 `/versions` 返回空 `items`；代码审查修正后，`/versions` 的 404 也按「无 ClawHub 版本打包内容」处理，详情保留元数据与 canonical 回链。显式选择的版本或文件 404 仍报错。上游对 `/versions` 的 404 语义仍未文档化，这里记录为契约缺口。
- **解析引用为诊断信息**：archive 条目显示所选版本的 semver；GitHub-backed 条目在版本列表为空时调用一次 `/api/v1/skills/{slug}/install?ownerHandle=...` 并显示 `repo@commit:path`。resolver 失败只降级为「暂不可用」，不隐藏详情；安装暂停期间不提供任何安装入口，也不创建 Git 身份。
- **未文档化的 exact 搜索**：详情别名识别使用 `/api/v1/search` 的 `mode=exact` 参数，该参数不在公开文档中。若上游忽略或拒绝该参数，最坏情况是别名条目在详情阶段显示错误而不是重定向；列表展示不受影响。
- **无 ClawHub 记录可创建**：安装暂停期间 `owner/slug@引用` 记录、唯一身份、同名冲突与原生管理只能做模型级/共享管线测试，未经过真实安装。
- **规格第 43 条（已安装/冲突状态）为 EXCEPTION**：安装暂停期间无法创建任何 ClawHub 市场记录，因此没有可查询的已安装状态或同名冲突；未构建推测性的已安装/冲突 UI。恢复安装时需另行实现并测试。
- **限流验证边界**：真实上游 429 未触发；读限制 3000 次/分/IP 仅出现在限流错误提示，下载限制 1200 次/分/IP 没有代码路径；共享冷却到期未做 Rust 单元测试。
- **代理边界**：ClawHub 客户端显式 `.no_proxy()`，不读取宿主系统/环境代理设置；应用不引入代理、账户或令牌。
- **实时 smoke 的波动**：SkillHub 与 skills.sh 的只读 live 测试各出现一次上游数据波动，重跑通过；这些断言依赖当日上游目录内容，不是确定性回归门禁。

## 代码审查后续

- **ADR-0006 的措辞已过时**：其中「frontend only branches on the lock family」不再成立——前端已按 `marketplace.rate_limited` 等具体错误码分支（ClawHub 列表/搜索），且这是既有先例。按审查结论不改 ADR 与代码，仅记录理由。
- **`Coordinate::from_canonical` 的 SkillHub 默认值**：保留以兼容既有 SkillHub 调用点，不新增别名构造函数；改为在方法文档中明确警告「裸 `@owner/slug` 不携带来源」，ClawHub/未来来源必须使用 `from_canonical_for`。
- 本次已顺手修复的低风险项：ClawHub 与 SkillHub 共用 `marketplace/files.rs` 的路径校验/预览/解码与 `%` 规则；前端共用 `MutationError`、`OfficialLink` 与统一错误通知块；Rust 来源标签/原生分类集中到 `marketplace`；测试记录工厂 `Installation::fixture`；缓存改为真正 LRU。
