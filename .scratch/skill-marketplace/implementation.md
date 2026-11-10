# 技能市场实施与验证

日期：2026-11-10。状态：完成并验证。

## 交付行为

原生「技能市场」页面与 Git「发现」并列。本次来源为 SkillHub，使用显式 Marketplace source、namespace 和 slug；目录不混入 Git 发现提醒。支持默认目录、分页、关键词搜索、技能说明与依赖、发布者、官方页面和平台版本。

安装支持全局或已注册项目、一次一个技能和多个 Agent。本地名来自 SKILL.md 的原始 name。同一作用域已有相同坐标显示已安装；其他同名内容、挂载或残留记录阻止覆盖。安装后的记录进入原有清单、内容查看、挂载、卸载及更新流程。

缺少、不能执行或能力不足的 SkillHub CLI 会在安装/更新入口提示原因、官方 CLI-only 命令和重新检测。浏览、检查更新、原生挂载和卸载独立于 SkillHub CLI；市场执行不要求 Node/Git。更新全部按来源处理可用性，预览中的跳过决定保留到执行。

失败目录请求保留上一次成功结果并注明原查询与页码。过期搜索响应不会替换当前结果。安装与更新预览重新读取具体版本内容，确认后保留目标版本。

## 记录和执行

市场安装记录文件名为 `.skills-box-marketplaces.json`，与当前作用域的 Git lock 同级：

- 全局：`~/.agents/.skills-box-marketplaces.json`。
- 项目：`<project-root>/.skills-box-marketplaces.json`。

记录格式 v1 保存原始名、完整坐标、确认请求的平台版本、安装内容基线和时间；统一投影为 Lock entry，不写入 Git CLI 的 lock。记录损坏、符号链接和重复坐标会报错；库存缓存、备份和中断检查纳入市场记录。

实际安装命令形态：

```sh
skillhub --skip-self-upgrade install public-slug@1.0.0 \
  --namespace publisher --dir '<isolated-stage>' --json
```

显式 `--namespace` 强制 CLI 走公开解析路径。`@namespace/slug@version` 虽支持指定版本，但其公开条目失败后存在回退到已保存企业配置的路径，所以应用使用分离的 namespace 参数。执行环境固定官方 host，禁用 CLI 自升级、默认 workspace skills 和 caller 上报。

暂存目录在预览时建立、路径固定，预览有效期为 15 分钟；目录消失或重定向会要求重新预览。CLI 在暂存区生成 namespaced 子目录，应用校验成功 JSON、真实目录、CLI lock 的官方下载地址及请求坐标/版本、SKILL.md 与原始名，拒绝符号链接和特殊文件。CLI JSON/lock 的 version 与包内 _meta 不作为平台版本真值。

验证后先复制到目标卷内临时目录，再发布原始名的规范目录、应用记录和 Agent 挂载。复用单写者、项目身份检查、备份、取消窗口、真实命令预览/记录、结果核验及本地修改确认。暂存失败不替换现有安装；发布后的失败保留备份和实际结果。

更新检查只查询平台版本元数据，Git 和市场共享来源预算及并发上限。当前平台版本与安装记录不同即为候选，同版本重新上传内容不检测。

## 当前资格与边界

- 支持单项更新和「更新全部」。清单勾选式批量更新遇到市场技能会明确提示改用上述入口。
- 市场残留记录支持原生清理；恢复入口提示先清理记录，再从技能市场重新安装。
- 「复制到项目」遇到市场技能提示改为在技能市场选择项目独立安装。
- 详情通过公开 ZIP 读取根目录 SKILL.md，压缩包预览限制为 32 MB，SKILL.md 为 2 MB。
- 本次不支持其他市场、私有/企业登录源、历史版本选择、离线安装或接管已有外部 SkillHub 安装。

## 验证结果

| 验证 | 结果 |
| --- | --- |
| `pnpm test` | 42 个文件、386 项通过 |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 687 项单元测试、33 项集成测试通过；网络测试默认跳过 |
| `pnpm build` | 通过；保留现有主 bundle 超过 500 kB 的体积警告 |
| 市场与更新命令前端定向回归 | 13 项通过 |
| 官方目录与精确版本只读 smoke | 通过：目录、ima-skills 详情/说明、无效版本拒绝 |
| 官方 SkillHub CLI 隔离集成 | 通过：真实安装 → 更新 → 删除临时 CLI 后原生卸载 |
| Chromium 浏览器预览 | 目录 → 详情 → 多 Agent → 预览确认 → 安装完成 → 正确技能详情通过 |
| 窗口与无障碍 | 1100 × 700 无横向溢出；市场和安装结果页 WCAG 2 A/AA 检查均 0 违规；无浏览器运行错误 |
| `git diff --check` | 通过 |

自动化重点覆盖全局/项目记录隔离、无需 Git runtime、同名和残留记录冲突、固定版本及上游元数据不一致、伪造 CLI 结果、错误下载版本、符号链接包、暂存失败、确认后本地内容变化、记录缓存失效、原生卸载与备份、CLI 重新检测恢复、旧结果诚实标记和搜索竞态。

真实 CLI 集成使用已下载的官方 2026.8.5 CLI kit，全部写入 tempfile HOME。测试条目为 `@tencent-adm/ima-skills`，平台版本 `1.1.10`，原始技能名 `ima-skill`，选中 Claude Code 和 Codex。安装/更新后核验平台版本、规范目录与挂载，Git lock 始终不存在；删除沙盒 CLI 后原生卸载成功。未修改真实用户 HOME，也未执行下载技能内的脚本。

可重复执行的真实 CLI 测试：

```sh
SKILLS_BOX_TEST_SKILLHUB_KIT=/path/to/extracted/official/cli \
  cargo test --manifest-path src-tauri/Cargo.toml \
  --test skillhub_marketplace_real_cli -- --ignored --nocapture
```

浏览器验证使用 Fake backend；真实文件变更由 Rust 测试和官方 CLI 隔离集成覆盖。尚未打包或启动签名 Tauri 发布版本。
