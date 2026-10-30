# 02: 项目读取模型、锁解析与安全边界

**Parent:** [spec.md](../spec.md)

**What to build:** 让清单、详情与内容浏览支持项目 scope。实现 `skills-lock.json` v1 只读解析（source、sourceUrl?、ref?、sourceType、skillPath?、computedHash；缺失=空；损坏或版本不支持=scope 错误）；项目清单扫描 `<root>/.agents/skills`、全部 catalog agent 的项目目录、已配置且存在的 extra 项目目录；沿用全局的 managed/unmanaged/Copy-only/Stale/断链/异常判定与基线逻辑（项目 lock 无时间戳，相关字段返回空由 UI 显示「—」）。引入 per-scope ReadModel 与缓存指纹（项目锁、canonical、agent 项目目录、agents.json），`get_inventory` 仍为刷新点。技能 ID 采用 scope 复合身份：全局 ID 不变，项目 ID 加项目前缀。所有读取命令按 scope 解析根路径；扩展安全白名单至已注册项目根，确保 canonical、agent 项目目录、extra 项目目录解析后不逃逸根目录，逃逸链接标记异常并拒绝内容读取。项目错误必须隔离在该 scope，不影响全局与其他项目。

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] 项目锁 v1 解析有单测：缺失、合法、损坏、版本不支持、字段缺失
- [ ] 项目清单覆盖 canonical、catalog 项目目录、extra 项目目录（仅存在时）与三类管理状态
- [ ] 项目详情、目录列表、文件读取、打开文件夹均限定在对应项目根内
- [ ] 复合 skill ID 稳定；全局 ID 与既有持久化不回归
- [ ] per-scope ReadModel 缓存按指纹失效，`get_inventory` 刷新目录拓扑
- [ ] 一个 scope 的锁损坏/权限错误只产生该 scope 错误状态，其他 scope 正常
- [ ] 路径不可用的项目返回独立状态而不是错误清单
- [ ] 符号链接逃逸项目根被识别为异常且内容命令拒绝读取
- [ ] 安全白名单与路径归属检查有单元/集成测试
