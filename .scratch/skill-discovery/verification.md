# Source discovery 验收记录

Status: Passed
Date: 2026-11-02
Spec: [spec.md](spec.md)

自动发现已完成，五张实施票据均已 resolved。实现遵循已确认的 [ADR 0019](../../docs/adr/0019-source-discovery-catalog.md)，使用实际 pinned CLI 只读预览作为 Source catalog 的内容依据。

## 自动验证

| 检查 | 结果 |
|---|---|
| `npm test` | 29 个文件、255 项通过 |
| `npm test -- src/App.discovery.test.tsx src/App.settings.test.tsx` | 最终定向回归 2 个文件、17 项通过 |
| `cargo test --offline --manifest-path src-tauri/Cargo.toml` | 544 项单元测试和 33 项集成测试通过，4 项显式 opt-in 测试跳过 |
| `cargo test --offline --manifest-path src-tauri/Cargo.toml discovery` | 最终凭据快照修改后的 20 项发现相关测试通过 |
| `npm run build` | TypeScript 和 Vite 构建通过 |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | 通过 |
| `git diff --check` | 通过 |

发现测试使用临时 app-data、registry/lock fixtures、注入 Clock/transport 和 ScriptedExecutor。覆盖首次基线/后续新增、明确空列表与失败保留、版本缓存、45 来源续扫与并发上限、跨 Scope 去重、状态持久化、并发已读/忽略、部分安装成功、Scope 冲突/异常/未知、目录名称与 metadata 名称映射、Project revision 与 A→B→A/Forget 撤销、来源停用恢复、迟到结果和退出取消。API wire 测试覆盖结构化限流恢复时间、认证请求上下文与 token 边界；旧认证响应不能设置或清除当前凭据的限流状态。

## 浏览器验证

通过命名 session `skills-box-discovery-qa` 检查 `preview.html?discovery=1` 的 Fake backend 页面：

- 默认展示当前 Scope 未安装项；「查看全部新增」包含入口数量对应的所有项，切换 Scope 保持数量。
- 搜索、来源筛选、详情及各 Scope 安装状态可用；详情处理单项提醒，忽略/恢复保留已读状态，全部已读不受搜索限制。
- 安装进入现有向导，重新预览并只预选点击项；Scope 切换重新预览，关闭向导回到发现页，没有自动安装。
- 1280px 和 600px 截图已检查，窄窗口没有水平溢出；发现页和详情 axe 检查均为 0 项违规。
- Tab/Enter 可以聚焦并切换发现筛选，已检查选中状态与焦点反馈。

浏览器验证使用演示数据；未向真实上游提交测试 skill，未使用真实 token。桌面窗口关闭的取消行为由 service 取消测试及既有进程/退出回归覆盖，未将 Web preview 记作原生桌面退出测试。

## 验证边界与交付

四项原有 opt-in 测试需要真实网络和 pinned CLI（CNB 全流程及真实 CLI/GitHub sandbox），本次没有显式运行。Cargo 使用缓存依赖 `--offline` 验证；这不影响本地单元/集成测试结果。构建保留 Vite 的大 chunk 提示，构建成功。

首次发现和来源版本变化需要 Node/npx/Git；缺少环境或解析失败保留上次目录。Token 仅用于原生 `api.github.com` 请求，CLI 自有请求不接收 token。应用退出后停止扫描；发现不会自动执行安装或更新。首次成功目录不标「新增」，之后首次出现的新名称才产生共享提醒。

使用行为与限制已写入 [README](../../README.md)，持久化及认证边界已写入 [security.md](../../docs/security.md)。本次验收未部署。
