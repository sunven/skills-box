# 启用覆盖清除与项目生命周期核查

日期：2026-11-20。仅读取命令帮助、仓库代码和官方文档；未修改产品实现或用户配置。

## Claude Code 恢复继承

本机 `Claude Code 2.1.325` 的 `plugin --help`、`plugin enable --help`、`plugin disable --help` 没有单独清除启用覆盖的 reset、unset 或 remove-override 操作；启停支持 `--scope user|project|local`。

`plugin configure` 管理插件自身的 `userConfig` 选项，遗漏选项保留原值，不能作为删除 `enabledPlugins` 覆盖的接口。当前查阅的官方 CLI reference 和 loading 文档也未说明单独清除启用覆盖的方法；准确结论是「未发现可用的官方接口」，不是证明所有版本的任何接口均不支持。

官方来源：

- [Plugin CLI reference](https://code.claude.com/docs/en/plugins/cli-reference)
- [Plugin loading](https://code.claude.com/docs/en/plugins/loading)

web 工具仍返回空结果，调查代理以只读 HTTP 获取官方正文补充核对。没有通过卸载、重装、写入继承值或直接编辑配置模拟 reset。

Codex 的对应限制已由[报告 02](02-codex-project-write-probe.md)记录。按照已确认的官方接口边界，首版没有已证明可靠的独立「恢复继承」写操作；界面可以说明当前覆盖及需外部调整后重新读取。

## Project 重新定位、遗忘与进行中操作

- `src-tauri/src/commands.rs:213`、`:224`：Relocate 和 Forget 都获取项目注册表变更 guard。
- `src-tauri/src/tasks/manager.rs:528`、`:546`：注册表变更与原生插件写入共用 mutation permit；冲突时立即返回忙碌错误，当前保护范围是整个应用。
- `src-tauri/src/native_plugins/mod.rs:194`：原生插件 worker 在执行与写后核验期间持有 guard，直到结果记录及任务结束。
- `src-tauri/src/native_plugins/mod.rs:130`：prepare 阶段不持有写入 permit。项目支持仍需将项目身份、根目录和配置层纳入确认，并在执行前重新验证；进行中操作的保护不能代替这一步。
- `src-tauri/src/projects.rs:235`：Relocate 更新注册表 root、revision、显示名及最近使用时间；外层刷新技能检查和 Discovery，不修改原生插件记录。
- `src-tauri/src/native_plugins/inventory.rs:158`、`:523`：Claude 安装从原生 `installed_plugins.json` 读取，`projectPath` 原样映射，不随应用的项目注册表变更。
- `src-tauri/src/projects.rs:260`：Forget 删除注册项并调整 active Scope，不卸载插件或删除宿主配置。

本轮未验证宿主后续启动是否自行修复旧路径，因此不承诺其自动跟随。Q9 已确认以新根观测的实际原生状态为准，旧路径问题由宿主修复或重新安装处理。
