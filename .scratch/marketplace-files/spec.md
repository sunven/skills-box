# 市场详情的文件页签

状态：已实现并验证。详见[实施与验证记录](implementation.md)。
日期：2026-11-11。

## 用户请求

用户在市场详情页完成后，标出 SkillHub 参考图中的「文件」页签并要求添加。本次是[详情页规格](../marketplace-detail-page/spec.md)的增量，覆盖初版「不做远程文件浏览、只有概述时不显示标签栏」的范围限制。

## 行为

- 详情提供「概述／文件」页签，默认显示概述。支持鼠标和方向键、Home、End 切换。
- 首次打开文件页签或正文中的包内文件链接时才读取当前 Marketplace coordinate 和 Marketplace version 的文件列表。
- 按路径展示可展开的目录树；默认预览 SKILL.md。显示文件路径、大小和平台版本。
- Markdown 可在渲染和原文之间切换；代码与其他 UTF-8 文本按原文显示，PNG、JPEG、GIF、WebP、SVG 使用图片预览。包内 Markdown 的相对文件链接进入同一文件浏览器，包内图片提供打开预览入口。
- 复用既有 Markdown 清理和外链规则；远程图片不自动加载。文件只读，不执行、不安装、不写入本地技能目录，也不依赖 SkillHub CLI。
- 切换页签、Scope 或关闭安装弹窗后保留选择的文件及目录展开状态。版本发生变化时重置选择并使用新版本缓存，旧文件不能冒充新版本。
- 目录和文件加载失败各自可以重试；空目录列表、文件缺失、二进制与超限文件给出明确说明。
- 桌面使用左侧目录、右侧预览；窄窗口上下排列，目录和长代码各自滚动，页面操作保持可达。

## 数据与边界

- 文件列表：`GET https://api.skillhub.cn/api/v1/skills/{slug}/files?version={version}&namespace={namespace}`。
- 文件内容：`GET https://api.skillhub.cn/api/v1/skills/{slug}/file?path={path}&version={version}&namespace={namespace}`，可跳转到官方 COS 文件地址。
- 原生层验证列表返回的完整身份、版本、文件数量、路径、大小与可选 SHA-256；读取文件前确认该文件仍在该版本列表中，再校验响应大小与摘要。
- 请求仅接受市场坐标、固定版本和包内相对路径。禁止绝对路径、路径穿越、编码路径、控制字符和目录／文件冲突。
- 重定向限 HTTPS 的 SkillHub API 与已验证官方 COS 主机；列表最多 2 MiB、2,000 个文件、32 层路径。文本沿用 1 MiB、图片沿用 5 MiB 预览上限，响应流同样受限。
- 浏览缓存按完整市场坐标、平台版本及文件路径隔离，不使用 Scope 作为公共内容身份；闲置文件缓存五分钟后可回收。
- 文件下载结果和摘要用于预览一致性检查，不构成安全评测结论，也不改变既有安装协议。

## 接口实测

2026-11-11，使用独立 agent-browser 会话观察[参考页](https://skillhub.cn/skills/indiv-integrator/dev-expert-pro)的文件入口与请求。

- `@indiv-integrator/dev-expert-pro` 的 `1.1.0` 列表返回 152 个文件，包括 `SKILL.md`、`THIRD_PARTY_NOTICES.md`、`references/` 和 `scripts/`。
- 单文件请求跳转到 `skillhub-1388575217.cos.accelerate.myqcloud.com`，返回原始文件内容。
- 原生适配器的线上只读测试已读取 Markdown 声明文件与 `scripts/skill_lookup.py`，并通过列表大小、摘要校验。

本次沿用 ADR-0029 的公共市场读取边界、ADR-0030 的版本身份及 ADR-0010 的共用 Fake backend。安装、编辑、远程版本历史不属于此增量。
