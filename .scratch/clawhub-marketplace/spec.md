# ClawHub 市场来源

Status: browsing/detail, native install and update implemented per ADR-0035; acceptance recorded in implementation.md (tickets 11–15)

日期：2026-11-16。
设计访谈 R1–R7 分轮确认（用户逐轮"按推荐"），整体规格已由用户"确认"。事实依据见 [上游契约调研](research/01-clawhub-contract.md)，身份决策见 [ADR-0034](../../docs/adr/0034-clawhub-versioned-registry-identity.md)。

## 目标与已确认决策

1. **来源定位（R1-Q1）**：ClawHub（`https://clawhub.ai/skills`）作为「技能」页签内继 SkillHub、skills.sh 之后的第三个 Marketplace source，支持浏览、搜索、详情与在应用内安装；不新增独立顶级入口，专家包仍只属 SkillHub。
2. **价值优先级（R4-Q1）**：覆盖 OpenClaw 生态优先于扩大技能数量；不以同类工具的市场来源对齐为目标。
3. **来源切换（R2-Q1）**：沿用现有来源切换器，记住用户上次选择；会话内各来源独立保留搜索词、排序、列表位置和滚动位置；专家包页签不改写技能页签记住的来源。
4. **能力驱动（R2-Q2）**：只展示上游匿名接口真实提供的数据；缺失字段省略或明确标注"未提供"；不伪造描述、版本、安装量、分类筛选或全量总数。
5. **匿名与账户边界（R2-Q3）**：只使用无需登录/Token 的公开读接口；不引入 ClawHub 账户、令牌或网页授权流程。
6. **兼容性边界（R2-Q4）**：只承诺安装生命周期正确（获取、落盘、Scope、挂载、冲突、更新），不承诺技能内容可在所有目标 Agent 中正常运行；详情如实标注来源为 ClawHub。
7. **单一安装身份（R3-Q1）**：同一 ClawHub 条目只有一种安装身份、一条市场记录、一条更新路径；不与 Git 直装或 skills.sh 安装合并身份。
8. **失败与来源隔离（R3-Q2）**：来源级失败隔离；保留上次成功结果并标注旧查询条件与错误；无旧结果时显示错误、重试与官网入口；不静默切换来源。
9. **验收标准（R3-Q3）**：在临时 HOME 与临时注册 Project 中完成真实端到端安装，不触碰真实用户环境；覆盖失败路径并回归 SkillHub 与 skills.sh。
10. **直连与隐私（R3-Q4）**：应用匿名请求 ClawHub，遵循宿主系统/环境代理配置；上游可见代理出口 IP 与搜索词；应用不新增代理配置、账户或令牌；不新增搜索历史存储，只沿用现有诊断日志范围。2026-11-17 用户确认由 ADR-0036 取代原“直连、不引入代理”约束。
11. **外部依赖（R4-Q2）**：至多接受一个官方 CLI 作为可选依赖；缺失时只阻止该来源安装，不影响浏览或其他来源；应用不自动安装依赖。
12. **不引入第三种安装通道（R4-Q3）**：实现优先落在 Git 安装或市场版本记录两种既有模型之一；本来源选择市场版本记录模型（见第 13 条）。
13. **安装身份（R5-Q1）**：统一注册表模型——所有 ClawHub 条目按 `owner/slug@解析引用` 建立市场记录。archive 条目的引用为 semver 版本；GitHub-backed 条目的引用为 resolver 返回的 commit 与子路径。更新按该引用比较；不把 GitHub-backed 条目改写成 Git 安装身份。
14. **skills.sh 别名（R5-Q2、R7-Q4）**：ClawHub 索引的 skills.sh 条目不在 ClawHub 来源中展示。搜索按 `install.kind` 过滤；列表接口是否含别名待实现前验证，若列表无法识别则照常展示、详情识别后引导切换到 skills.sh 来源；不做逐条 resolver 调用。
15. **条目与功能范围（R5-Q3）**：只展示 skills，排除 plugins、templates、评论与发布；不做账户、星标/评论交互、集合。
16. **合规（R5-Q4）**：按官方 HTTP API 文档的第三方目录许可实现缓存、429/`Retry-After`、canonical 回链、限流与不暗示背书；`robots.txt` 与文档冲突以文档许可为准；被阻断时降级官网入口，不绕过。
17. **详情版本锚定（R6-Q1）**：详情默认展示 latest 版本，可切换历史版本；文案标注「ClawHub 版本 vX 的打包内容」；安装固定确认时解析出的引用；不承诺同版本内容不可变。
18. **缓存边界（R6-Q2）**：仅会话级内存缓存并遵守 HTTP 缓存头；目录和内容不写入 App data，不提供离线浏览。
19. **扫描信息（R6-Q3）**：首版不展示 `scan` 字段；语义核实后另立需求。
20. **执行适配器（R7-Q1）**：官方 `clawhub` CLI 作为可选依赖，隔离暂存、显式版本、非交互；绝不自动传 `--force`/`--force-install`；发布前剥离 CLI 生成的包内 `.clawhub/` 元数据；CLI 自己的 lock 不作为身份依据。版本固定必须实测；若不被尊重或结果不可校验，暂停安装并单独立项评估原生 HTTP+ZIP（需新 ADR），不静默切换。
21. **更新语义（R7-Q2）**：比较安装时解析出的市场引用；引用相同不产生更新候选；同版本 archive 内容变化不检测；记录的 commit/contentHash 仅作诊断；本地基线只检测本地改动。
22. **浏览控制（R7-Q3）**：排序只暴露文档化的推荐 / 最近更新 / 下载量；cursor「加载更多」，不显示伪造总数；分类只展示标签不提供筛选；搜索保留上游顺序并提示返回上限。

## 安装执行修订（ADR-0035）

2026-11-17 已确认：以应用原生 HTTP+ZIP 通道取代官方 `clawhub` CLI 执行 ClawHub 安装与更新。浏览、搜索、详情与版本文件行为不变。

- **取代第 11、12、20 条中的执行部分**：ClawHub 安装/更新不再依赖官方 CLI、Node 或 npm；"至多一个可选 CLI 依赖"和"不引入第三种安装通道"的限制不再适用于本来源。安装身份、市场记录、发布管线、挂载与冲突规则（第 6、9、13、21 条）保持不变。
- **archive 条目**：按确认时解析出的显式 semver 版本从 `/api/v1/download` 下载 ZIP，用 `/api/v1/skills/{slug}/versions/{version}` 的 per-file sha256 校验后发布。只允许注册表生成的 `_meta.json` 作为额外文件；拒绝符号链接、特殊文件、路径穿越、额外内容与超限包。
- **GitHub-backed 条目**：按 resolver 的 `repo/path/commit` 从 GitHub codeload 固定 commit 下载子路径，并复现 ClawHub 的文件夹内容哈希与 `contentHash` 对照。该哈希算法未文档化，必须先通过隔离验证门槛；未通过时 GitHub-backed 安装保持阻断并在界面明确标注暂不支持，不回退 CLI。2026-11-17 门槛已通过（AWS 固定 commit 与 6 个抽样文件夹全部匹配，见 [门槛记录](research/03-github-content-hash-gate.md)），GitHub-backed 原生安装已启用；待扫描/可疑条目仍按 moderation 阻断。
- **可疑技能**：安装时读取 skill 级 `moderation` 与版本级 `security`；显式 suspicious/malware 一律阻断并给出官网链接，不提供 `--force` 等价物。GitHub-backed 待扫描条目在上游清理前不可安装。
- **依赖与失败**：表中 `clawhub` CLI 缺失相关行不再适用；改为纯网络依赖，仅阻断 ClawHub 安装/更新，不影响浏览、其他来源与本地管理。
- **更新语义**：仍按第 21 条比较安装引用；更新执行复用同一原生适配器。
- 工单 02/07/08/09 保持历史结论；新工单 11–15 实现本修订。

## 页面与浏览行为

主流程是「技能市场 → 技能 → 选择 ClawHub → 列表或搜索结果 → 市场详情 → 单技能安装确认 → 安装结果」。当前 Global scope 或已注册 Project 决定安装目标；浏览目录与 Scope 无关，已安装状态和冲突跟随当前 Scope。

- 首次使用默认 SkillHub；选择 ClawHub 后重启恢复。同一应用会话内往返来源及详情，分别恢复查询与列表位置。
- 无关键词时展示列表，排序为推荐 / 最近更新 / 下载量；分页使用上游 cursor「加载更多」，不显示并不存在的总页数或全量总数。
- 有关键词时使用独立搜索结果，保留上游顺序；达到接口返回上限时提示本次返回条数有限，不建议更窄范围之外的"全量搜索"承诺。
- 分类只在卡片与详情作为标签展示，不提供筛选，因为 `category`/`topic` 参数未文档化。
- 卡片展示可获得的技能名、`@owner/slug`、描述（若有）、来源标识、ClawHub 指标和官方标记（若有）。匿名字段缺失时省略或标注"未提供"，不按零处理。
- ClawHub 指标（downloads/installs/stars）保持其平台语义，不与 SkillHub 下载量或 skills.sh 安装量比较或合并。
- 已安装状态按当前 Scope 的市场记录判断；同 Git source、同技能名的情况按最近安装身份与冲突规则展示。
- 搜索中的 skills.sh 别名条目（`install.kind: "skills-sh"`）过滤；来源说明注明"skills.sh 条目请在 skills.sh 来源浏览/安装"。

## 市场详情与版本

详情明确展示：收录于 ClawHub、`@owner/slug`、实际安装引用（archive 的 semver 或 GitHub 的 `repo@commit:path`）、官网入口、ClawHub 指标、分类、许可证、版本列表与 changelog（若上游提供）。

- 概述展示 latest 版本或用户所选版本的 `SKILL.md`；文件页签只展示该版本实际返回的文件。文件预览沿用现有只读 Markdown/文本/图片规则，不执行内容。
- 标注「ClawHub 版本 vX 的打包内容」；说明它可能与安装时重新解析的结果不同（同版本内容不可变性无契约保证），安装以实际下载内容为准并记录基线。
- 同一份详情阅读中的概述与文件必须来自同一版本，不能混用不同版本的列表与内容。刷新失败保留旧内容时明确标记失败；无可用内容时显示错误、重试和官网入口。
- 大文件按接口限制处理（预览阈值之外只显示元数据）。
- 不展示 `scan` 字段。若条目实际为 skills.sh 别名，详情标注来源并引导切换来源，不提供 ClawHub 内安装。

## 安装身份、选择与确认

- 市场坐标是 `owner/slug`，版本是解析出的市场引用；本地技能名取包内 `SKILL.md` 的规范名称。二者不混用。
- 点击安装先解析当前条目的权威引用（archive 版本或 GitHub commit），确认页只安装该一个技能及目标 Scope、所选 Agents、仓库/下载来源和实际执行命令。
- 确认页说明：按 ClawHub 解析出的固定引用安装，实际内容以本次下载为准；不保证与浏览内容完全一致。
- 执行使用隔离的暂存目录调用官方 CLI；应用发布 `SKILL.md` 目录到 Canonical store、建立 Agent attachments，并写入包含完整坐标、解析引用和内容基线的市场记录。
- 引用验证必须覆盖 archive 与 GitHub-backed 两类条目；`--version` 对 GitHub-backed 引用是否生效是硬门槛的一部分，未通过则阻止安装。
- 同一 Scope 内同名技能来自 SkillHub、skills.sh、Git 或其他未受管目录时，沿用现有同名冲突规则；不覆盖、不自动改名、不按名字合并来源。
- Stale lock entry 占用名称时使用现有清理入口；同名技能只在其他 Scope 不阻止当前 Scope 安装。
- 上游安全拦截（可疑技能）时如实报错并提供官网入口，不自动传 `--force`；CLI 缺失时浏览与详情仍可用，安装/更新显示依赖问题与重新检测入口。

## 更新语义

- 更新检查读取 ClawHub 元数据，比较安装记录中的解析引用：archive 比较 semver，GitHub-backed 比较 commit；引用相同不产生更新候选。
- 同版本 archive 内容被上游替换不检测，也不自动重装；本地内容基线只负责检测本地修改。
- 记录解析出的 commit/contentHash（可得时）仅用于诊断和完整性核对，不作为更新依据。
- 更新执行复用同一 CLI 适配器与既有市场发布流程；缺少 CLI 只阻止安装/更新，不阻止本地挂载、卸载与清理记录等其他管理操作。市场技能不能从 Stale lock entry 本地恢复，也不能复制到项目；恢复规则是清理记录后到市场重新安装。
- ClawHub 不可用时不伪造更新状态；已安装技能的本地管理不受影响。

## 依赖与失败

| 情况 | 行为 |
| --- | --- |
| `clawhub` CLI 缺失 | 列表、搜索、详情和版本内容仍可用；安装/更新显示依赖问题与重新检测入口，不误伤其他来源 |
| CLI 实测不遵守 `--version` 或结果不可校验 | 硬门槛失败：暂停 ClawHub 安装，记录证据，单独立项评估原生 HTTP+ZIP，不静默切换 |
| 上游安全拦截（可疑技能） | 阻止本次安装，显示上游错误与官网入口；不自动 `--force` |
| 列表/搜索失败 | 保留该来源上次成功结果并标注旧查询条件与错误；无旧结果则显示错误、重试和官网入口 |
| 详情或版本文件失败 | 明确内容读取失败；只要安装引用可解析，仍可按正常确认流程安装 |
| 引用解析失败 | 阻止安装，保留目录信息、重试和官网入口 |
| 裸 slug 返回 409 多 owner | 界面始终以 `@owner/slug` 展示和操作；不猜测 owner |
| 429 / 限流 | 显示限流状态，按 `Retry-After` 退避；不重复轰炸 |
| CLI 退出码只有 1、无 JSON | 展示 stderr 摘要并以落盘校验为准，不把纯文本输出当作可靠结果 |
| 本地清单不可读、项目失效、目标或冲突变化 | 沿用现有 Scope、记录与写前复验规则 |

不同来源及查询的缓存、旧结果和过期响应必须隔离，不静默切换来源。

## 合规、缓存与隐私

- 官方第三方目录许可：使用公开读接口，缓存结果，遵守 429/`Retry-After`，回链 `https://clawhub.ai/<owner>/skills/<slug>`，不暗示 ClawHub 背书，不镜像隐藏、私密或被审核屏蔽的内容。
- 限流：3000 读/分/IP、1200 下载/分/IP；实现退避与显式错误。
- `robots.txt` 禁止 `/api/` 与 API 文档许可冲突，以文档许可为准；若许可移除或访问被阻断，降级为官网入口，不绕过。
- 缓存仅会话级内存，遵守 HTTP 缓存头；不写入 App data，不提供离线浏览。
- 遵循宿主系统/环境代理请求 ClawHub（应用不内置代理配置、账户或令牌）；上游可见代理出口 IP 与搜索词。

## 工程边界

- 新增 ClawHub 目录适配与能力分派；不构建用户可配置的市场插件系统。
- 复用现有市场记录存储与发布管线，扩展来源专属字段（`owner/slug`、解析引用、commit/contentHash 诊断信息）。
- 首版不新增原生 ZIP 下载/解压通道；执行只走官方 CLI 适配器。若硬门槛失败，原生适配器需新 ADR。
- 不改变 SkillHub 的固定版本安装/更新语义，也不改变 skills.sh 的 Git 身份与默认分支安装语义。
- 不新增非 GitHub 安装协议、URL 导入、批量安装、固定历史提交或离线安装。
- 测试覆盖：目录解析、别名过滤、版本详情、失败回退、CLI 结果校验、记录与发布、Scope 隔离；live 测试标记为 ignored。

## 实现前验证项

1. 隔离环境中 `clawhub install --version` 是否严格安装指定 archive 版本；对 GitHub-backed 引用是否同样可固定。
2. 可疑技能在 `--no-input` 下的错误形态，确认不会静默通过或意外修改已有文件。
3. archive 条目能否用 `/api/v1/skills/{slug}/versions/{version}` 的文件 sha256 做安装后内容校验（下载接口本身无 digest）。
4. `/api/v1/skills` 列表是否包含 skills.sh 别名条目、是否携带 `install.kind`；若不含 kind，按 R7-Q4 的详情页引导处理。
5. `--workdir`/`--dir` 的绝对路径行为、flag 顺序、`--no-input` 与交互提示的交互。
6. 429 响应是否实际携带 `Retry-After`，以及 CLI 下载阶段的限流归属。

### 实现前验证结果（2026-11-16，ticket 02）

结论：**NO-GO（CLI 安装通道按现规格暂停）**。完整证据见 [CLI 可行性验证](research/02-clawhub-cli-feasibility.md)。

- archive 固定版本：**通过**。`clawhub@0.26.0` 在隔离环境中按 `--version` 安装 `@steipete/nano-banana-pro@1.0.0`，落盘 `SKILL.md` 的 sha256 与 `/api/v1/skills/{slug}/versions/1.0.0` 一致，且与 latest `1.0.1` 不同。
- GitHub-backed 显式固定：**不通过**。resolver 返回 `repo/path/commit/contentHash`；`install @aws/agents-build --version <commit>` 退出 1 并报 `Version not found`，无其他固定参数；不带 `--version` 时 CLI 只解析并安装调用时的 latest commit。内容可按 resolver `contentHash` 独立复核。
- 可疑技能拦截：**行为符合预期但不满足策略**。真实 `moderation.isSuspicious` 条目在非交互、无 `--force`/`--force-install` 时退出 1、仅 stderr 纯文本、不落任何目标文件；但 `--force-install` 不绕过，GitHub-backed 待扫描条目需要规格禁止的 `--force` 才能安装。
- 内容校验：archive 可用版本文件的 per-file sha256 校验（下载无 digest，`_meta.json` 为生成文件）；GitHub-backed 可用 resolver `contentHash` 复核。CLI 本身不校验 archive，也不校验 native GitHub 路径。
- 429/`Retry-After`：真实上游未触发；本地 mock 429 证明 CLI 共 3 次尝试、遵守 `Retry-After`（上限 60s）后退出 1。文档限流为 3000 读/分/IP、1200 下载/分/IP。
- 目录组成：`/api/v1/skills` 抽样 128 条无 `install`/`kind` 字段，抽样未出现 skills.sh 别名；`/api/v1/search` 返回 `install.kind`（`clawhub`、`skills-sh`）。别名过滤只能在 search 侧；列表按 R7-Q4 回退。
- 落盘布局与失败形态：native ref 落到 `<dir>/@owner/<slug>`（不是 `<dir>/<slug>`）；所有普通失败退出 1、无 JSON、stderr 纯文本且可能重复一行。
- lock/origin：仅含 version/owner/fingerprint，无契约 digest；GitHub-backed 记录无 repo/path；二者为 CLI 本地可变状态，不得作为唯一身份或版本依据。

影响：按本规格第 20 条与 ADR-0034，安装暂停；07/08/09 不启动，04/05/06 浏览与详情可继续。原生 HTTP+ZIP 适配器需新 ADR 单独评估，不静默切换，也不静默收窄为仅 archive。

## 验收条件

1. 首次进入技能市场默认 SkillHub；选择 ClawHub 后重启仍恢复；会话内来源往返与详情返回分别恢复查询和列表位置；专家包始终属 SkillHub。
2. 三种排序与 cursor 加载更多可用；不显示伪造总数；分类只作标签。
3. 搜索保留上游顺序并提示上限；skills.sh 别名不在 ClawHub 来源中展示或在详情页被识别并引导。
4. 卡片与详情正确展示可获得字段；缺失字段明确标注；ClawHub 指标不与 SkillHub/skills.sh 混排。
5. 详情默认 latest、可切换历史版本；概述与文件来自同一版本；缺失/失败有明确状态；不展示扫描结论。
6. 在隔离的测试 Global scope 与 Project 中完成单技能安装；确认页显示真实 `owner/slug`、解析引用、Scope、Agents、命令与内容差异提示；核验市场记录、实际目录和 Agent 挂载。
7. 安装固定确认时的引用；archive 版本或 GitHub commit 可被验证；若 CLI 无法保证固定，安装被阻止并记录硬门槛结论。
8. 可疑技能被上游拦截时不带 `--force`，显示错误与官网入口；CLI 缺失时浏览可用、安装被阻止。
9. 同名不同来源、Unmanaged、Copy-only 与 Stale lock entry 沿用对应冲突/恢复规则；其他 Scope 的安装不阻挡当前 Scope。
10. 列表/搜索/详情失败保留上次成功结果并标注；无旧结果时错误、重试、官网入口；限流按 `Retry-After` 退避。
11. 更新比较解析引用；同版本内容变化不产生更新候选；本地基线检测本地改动；ClawHub 不可用不伪造更新状态。
12. 回归 SkillHub 的筛选、版本详情、文件浏览、固定版本安装/更新与专家包；回归 skills.sh 的 Git 浏览、快照、单技能安装与更新；回归 Git 直接安装与 Source discovery。
13. 真实 CLI 验证在隔离 HOME/Project 中完成，不触碰真实用户环境；验证结果与例外记录在实施记录中。

## 设计树与确认状态

| 分支 | 决策 | 状态 |
| --- | --- | --- |
| R1 来源定位与首版目标 | 第三个 Marketplace source；安装能力以核查为准 | 已确认 |
| R2 导航、能力、匿名、兼容 | 来源切换、能力驱动、无账户、只承诺生命周期 | 已确认 |
| R3 身份唯一、隔离、验收、隐私 | 单一身份、来源级失败隔离、隔离验证、遵循系统代理 | 已确认 |
| R4 动机、依赖、通道 | OpenClaw 优先、至多一个可选 CLI、不新增第三通道 | 已确认 |
| R5 注册表身份、别名、范围、合规 | `owner/slug@引用` 市场记录、过滤 skills.sh 别名、skills-only、按文档许可 | 已确认 |
| R6 详情版本、缓存、扫描 | 版本锚定、会话内存缓存、不展示扫描 | 已确认 |
| R7 执行、更新、浏览、别名机制 | 官方 CLI + 硬门槛、引用比较、文档化排序、可识别处过滤 | 已确认 |
| 整体规格 | 用户明确回复"确认" | 已确认 |

## 相关约束

- [上游契约调研](research/01-clawhub-contract.md)：匿名接口、分发形态、CLI 能力、合规与稳定性事实。
- [ADR-0034](../../docs/adr/0034-clawhub-versioned-registry-identity.md)：ClawHub 版本化注册表身份。
- [ADR-0035](../../docs/adr/0035-clawhub-native-http-zip-installation.md)：ClawHub 原生 HTTP+ZIP 安装（提案）。
- [ADR-0036](../../docs/adr/0036-clawhub-follow-system-proxy.md)：ClawHub 跟随宿主系统代理。
- [ADR-0029](../../docs/adr/0029-marketplace-catalog-boundary.md)：市场目录与 Git 来源发现独立。
- [ADR-0030](../../docs/adr/0030-marketplace-installation-records-and-execution.md)：市场记录、平台版本与发布执行。
- [ADR-0033](../../docs/adr/0033-marketplace-catalog-and-git-installation-identity.md)：skills.sh 目录身份与 Git 安装身份分离。
- [ADR-0001](../../docs/adr/0001-cli-mutations-native-reads.md)：CLI mutation 边界。
- `GLOSSARY.md`：Marketplace source、Marketplace coordinate、Marketplace version、Marketplace snapshot、Managed skill 等术语。

## Comments

- 2026-11-16：完成两轮只读核查（目录/分发/CLI/合规），设计访谈 R1–R7 逐轮由用户确认（"按推荐"），整体规格由用户回复"确认"。状态更新为 `ready-for-agent`。
- 2026-11-16：`GLOSSARY.md` 已随访谈更新：Marketplace source 示例加入 ClawHub；Marketplace version 定义扩展为"版本标签或解析引用（如 commit）"。
- 2026-11-16：按现有市场技能语义修正本地恢复/复制到项目措辞（ClawHub 归类为原生市场来源）；实现工单发布于 `issues/`（01–10）。
- 2026-11-16：实现完成。工单 01/03/04/05/06/10 已交付并关闭；02 门槛结论为 NO-GO；07/08/09 按门槛关闭为 wontfix。两轴代码审查后修复项（版本锚定 license/changelog、`/versions` 404 保留详情、搜索缺 kind 容错、缺失文件回链、GitHub-backed 解析引用诊断、`.no_proxy()`、去重与 ADR-0034 同步）已合入 `b485f38`。验收与例外见 [implementation.md](implementation.md)。
- 2026-11-16：完成浏览、详情、合规与回归验收（工单 10），环境、证据、逐条验收与例外见 [implementation.md](implementation.md)。按工单 02 的 NO-GO，07/08/09 未实现；安装/更新执行与验收条件 6–9、11 记为 EXCEPTION（11 同时包含未实现的引用比较），原生 HTTP+ZIP 适配器需新 ADR 单独评估。未声称真实 CLI 安装或更新通过。
- 2026-11-17：起草"安装执行修订"与 [ADR-0035](../../docs/adr/0035-clawhub-native-http-zip-installation.md)，新增工单 11–15（原生 archive 下载校验、安装发布、GitHub-backed 门槛、更新、验收）。用户已确认：ADR-0035 置为 `accepted`，工单转 `ready-for-agent`。
- 2026-11-17：工单 11–13 实现并关闭：原生 archive 下载/解压/逐文件校验、安装预览与原生发布（Global/Project、挂载、冲突）、GitHub-backed 原生安装（codeload 固定 commit、ICU en-US 复现 `contentHash`、记录诊断、UI 入口）。门槛证据见 [GitHub content-hash gate](research/03-github-content-hash-gate.md)。工单 14（更新检查与执行）与 15（隔离验收与回归）仍为 `ready-for-agent`。
- 2026-11-17：工单 14 实现并关闭：更新检查按来源分派并比较 archive 版本或 GitHub commit；更新预览构建 ClawHub 计划，固定预览后在执行前暂存新引用，由原生动作发布并保留挂载与 `installedAt`；混合更新中 ClawHub 条目不会回退到 CLI。工单 15（隔离验收与回归）仍为 `ready-for-agent`。
- 2026-11-17：工单 15 完成并关闭：隔离 Global/Project 真实安装+更新、GitHub-backed 暂存与哈希、全量回归与合规检查已记录于 [implementation.md](implementation.md)；EXCEPTION 为完整 live GitHub-backed 发布（抽样条目被 moderation 阻断）、真实 429 未触发、未自动化单次 live Manager/UI 编排。
- 2026-11-17：用户确认 [ADR-0036](../../docs/adr/0036-clawhub-follow-system-proxy.md)：ClawHub 与 GitHub-backed 下载改为跟随宿主系统/环境代理，取代 R3-Q4 的直连约束。实测直连约 5.2s/请求、系统代理约 1.4s/请求；同时详情/别名/版本请求并行化、安装预览去掉重复版本请求、超时提高到 60s。
