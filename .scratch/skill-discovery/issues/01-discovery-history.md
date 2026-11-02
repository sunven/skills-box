# 01 — Discovery history 与记录存储

Status: resolved
Type: task
Parent: [spec.md](../spec.md)

## 工作

建立独立的 Discovery 领域记录与 app-data `discovery.json` 存储，落实规格中的首次基线、同源同名身份、新增/已读/忽略、移除/恢复和来源休眠行为。目录观察和用户状态分别建模，避免用某次网络快照覆盖扫描期间的用户决定。

- Source identity 使用现有 SourceSpec 规则；Discovered skill identity 为来源加大小写不敏感的名称，不依赖 Scope 或路径。
- 保存有效来源快照、catalog、观察历史、尝试/成功时间与用户状态；历史独立于当前目录成员。
- 原子写入、版本与合理大小校验；已有文件读错不能静默初始化。状态写入失败不宣告已读/忽略成功。
- 全部已读处理命令执行时的活跃、可用未读项，不吞掉之后才发现的项。
- 保留 Scope/revision 信息用于后续资格校验，不把旧存盘关联当成当前注册表或安装事实。
- 不存 token、CLI 环境或未经验证的 URL；不改安装 lock。

## 验证

通过公开状态转换和存储接口验证首次 60 项不标新、后续新增、大小写/移动、不重提醒、忽略恢复、源停用与同名回归、重启恢复、全部已读并发和读写失败。使用临时 app-data 路径，不触碰真实安装目录。

## Comments

- 2026-11-02：规格已准备；状态仅因最终共享理解确认仍为 needs-info，未开始实现。

- 2026-11-02：用户确认完整规格，已可实施。

## Answer

已实现独立的 Source catalog 与 Discovery history、版本化原子存储、基线/新增/已读/忽略及恢复状态。存储损坏显式保留，用户状态写入失败不报告成功；公开状态转换、并发合并与重启测试通过。

验收：[verification.md](../verification.md)。
