# skills.sh 接入事实

核查日期：2026-11-14。本文区分官网文档、官方 CLI 源码及匿名 HTTP 实测；实测可用不代表接口稳定性承诺。

## 接口与身份

- 文档化的 `/api/v1/` API 使用 Vercel OIDC 身份；匿名列表、搜索和详情请求实测返回 401。文档提供 `sourceType`、`installUrl` 和条目 URL 等字段。[S1]
- 官网当前使用的匿名榜单接口为 `/api/skills/{all-time|trending|hot}/{page}`，页码从 0 开始。实测返回 `skills`、`total`、`hasMore`、`page`，每批 200 项；请求中的 `per_page` 与 `source` 参数没有按文档化 API 的含义生效。[S2]
- 官方 CLI 的 `src/find.ts` 使用匿名 `/api/search`。实测关键词至少两个字符，`limit=1000` 仍最多返回 200 项，没有全量搜索分页契约。匿名结果包含 `id`、`source`、`skillId`、`name`、`installs`，不提供完整描述或安装地址。[S3, S4]
- GitHub 条目的 `source` 是 `owner/repo`，条目身份为 `owner/repo/skillId`；必须分别保留 `skillId` 和 `name`。官方 CLI 按 source 与选中的 name 安装，不能将 URL slug 当成仓库内路径。[S4]
- skills.sh 也收录独立网站技能。`open.feishu.cn/lark-doc` 官网详情给出的安装来源带路径 `https://open.feishu.cn/lark-cli/skills/regular`，仅凭搜索结果中的域名无法还原这个地址；其详情页路由为 `/site/{domain}/{skillId}`。[S5]

## 内容与统计

- 匿名 `/api/download/{owner}/{repo}/{skillId}` 返回 `{files: [{path, contents}], hash}`；`vercel-labs/skills/find-skills` 实测成功，`uizze.sh/ui-taste` 实测 404。官方 CLI 的 `src/blob.ts` 使用该接口，并在使用快照前校验，不将缓存快照自动视为完整且最新的仓库内容。[S6, S7]
- 官网说明榜单来自 CLI 安装遥测，应称“安装量”。不同榜单的时间范围不同，不能把这些数字当作 SkillHub 下载量或实际用户数。[S8]
- 实施时再次核对官网：累计榜使用累计安装量，趋势榜标注 `Trending (24h)`；热榜页面描述为最近一小时与昨日同小时比较，返回 `installs`、`installsYesterday` 和 `change`。界面将热榜的 `installs` 标为“最近一小时安装量”，保持上游排名，不自行按累计量重新排序。[S2, S9]
- 匿名榜单是当前接口可浏览的榜单窗口；不能声称其 `total` 等于平台收录的所有技能数量。分类、API Key 标签与平台发布版本也不能从当前匿名结果中推导。[S2, S3]

## 与仓库的关系

- 当前 TS 与 Rust 市场坐标仅有 SkillHub；列表 IPC 没有来源参数：`src/lib/marketplace.ts`、`src-tauri/src/marketplace/mod.rs`、`src-tauri/src/commands.rs`。
- 现有市场详情／文件／安装确认按 SkillHub 平台版本组织；skills.sh 的快照 hash 不应冒充 Marketplace version。
- 已有 Git 安装按仓库和技能 name 选择，输入限定为公开 Git 仓库；独立网站的 well-known 安装协议尚未支持：`src-tauri/src/tasks/source.rs`、`src-tauri/src/tasks/install.rs`、`src-tauri/src/cli/scoped.rs`。
- Git 安装默认分支的行为与 SkillHub 固定平台版本的行为不同；界面不能暗示 skills.sh 的目录快照就是最终 Git 安装内容。
- Git 预览结果仅提供 source、技能名、描述、group 和冲突，没有 commit 或内容指纹（`src/lib/types.ts:188`）。安装请求仅携带 source、skills、agents、retry（`src-tauri/src/tasks/install.rs:48`）；命令确认标识校验命令和 Scope，不固定仓库内容（`src-tauri/src/cli/preview.rs:33`）。
- 现有 Discovery 安装 seed 按不区分大小写的技能名要求唯一匹配，但之后仍显示仓库其他技能及全选；失败还会触发 Discovery source 失效处理（`src/components/install-wizard.tsx:175`、`:188`、`:698`）。市场的单技能安装不能原样继承这些额外选择和发现专属副作用。

## 主要来源

- S1：`https://www.skills.sh/docs/api`。
- S2：`https://www.skills.sh/` 及其前端调用；`https://www.skills.sh/api/skills/all-time/0`、`/all-time/1`、`/trending/0`、`/hot/0`。
- S3：`https://www.skills.sh/api/search?q=react&limit=1`，另测短关键词及超限 limit。
- S4：官方仓库 commit `b716f9dc2ba36b6aa8f0080615e5a3a6caeac63b` 的 `src/find.ts`：`https://github.com/vercel-labs/skills/blob/b716f9dc2ba36b6aa8f0080615e5a3a6caeac63b/src/find.ts`。
- S5：`https://www.skills.sh/site/open.feishu.cn/lark-doc`、`https://www.skills.sh/site/uizze.sh/ui-taste`。
- S6：`https://www.skills.sh/api/download/vercel-labs/skills/find-skills`、`https://www.skills.sh/api/download/uizze.sh/ui-taste`。
- S7：同一官方 commit 的 `src/blob.ts`：`https://github.com/vercel-labs/skills/blob/b716f9dc2ba36b6aa8f0080615e5a3a6caeac63b/src/blob.ts`。
- S8：`https://www.skills.sh/docs`。
- S9：`https://www.skills.sh/hot`，2026-11-14 实时 HTML 的 description 与榜单数据。
