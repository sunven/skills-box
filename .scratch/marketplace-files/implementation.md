# 市场文件页签实施与验证

日期：2026-11-11。依据：[规格](spec.md)。

## 实施

- 市场详情新增「概述／文件」页签。文件请求按需发起，完整坐标、版本、路径共同确定缓存身份。
- 原生 `marketplace/files.rs` 接入官方文件列表与单文件接口；IPC 不接收 URL 或本地路径。浏览不依赖 CLI，不安装、不执行、不落盘技能文件。
- 目录树、Markdown 渲染／原文、UTF-8 代码文本、图片、文件大小和相对路径导航已接通。文件名和代码超出可用宽度时在各自区域截断或滚动。
- 页面保留安装入口和既有 Mutation surface。切换 Scope、页签及关闭安装弹窗保留选中的文件和目录状态；文件链接导航后焦点移到文件页签或预览区。
- 原生层核对完整市场坐标、返回版本、文件数量、相对路径、文件大小与平台提供的 SHA-256。重定向只允许已验证的官方 HTTPS 主机，流式读取受大小限制。
- 文本、图片分类和大小限制复用本地内容浏览的定义。HTML、外链与远程图片处理继续复用 SkillMarkdown；包内图片通过「打开文件」进入单文件预览。
- 预览入口：`/preview.html?marketplaceFiles`，与 App 测试使用相同 Fake backend。

## 验证

| 检查 | 结果 |
| --- | --- |
| `pnpm test` | 43 个测试文件、404 项通过 |
| 市场相关回归 | 最终焦点及可访问性调整后，2 个文件、28 项通过 |
| `pnpm build` | TypeScript 和生产构建通过；仍有既存主包超过 500 kB 的提示 |
| `cargo test --lib marketplace::files::tests` | 5 项通过；线上用例默认忽略，另行显式执行 |
| `cargo test --lib marketplace::files::tests::skillhub_live_versioned_files -- --ignored` | 1 项通过；读取 `@indiv-integrator/dev-expert-pro@1.1.0` 的 Markdown 声明文件和 Python 脚本，校验大小及摘要 |
| `rustfmt --check --edition 2024 src/marketplace/files.rs` | 通过 |
| `git diff --check` | 通过 |
| 浏览器布局 | 1440、1024、768、390、320px 的文档、主阅读区和预览区无横向溢出；见[尺寸记录](browser-layout.json) |
| axe-core 4.12.1 / WCAG 2 A、AA | 顶部阅读状态 0 violations、0 incomplete、23 passes；见[结果](a11y.json) |

交互测试覆盖首次打开时加载、嵌套目录、Markdown 原文、包内链接、图片、错误重试、无旧内容串用、缺失文件、空列表、大文件、二进制、版本缓存隔离、Scope 切换、关闭安装后保留文件，以及键盘页签和焦点。

真实浏览器另外验证了图片可以解码显示，代码在 390px 窗口可见，滚动后精简操作栏仍与应用页头相接。滚动到文件区时，axe 曾将被吸顶栏遮挡的旧标题对比度列为需人工检查；标题回到正常可见区域时检查通过。

截图：[桌面文件阅读](desktop.png)、[390px 目录](narrow.png)、[390px 代码预览](narrow-code.png)、[320px 布局](code-320.png)。

浏览器交互使用 Fake backend；真实公开接口通过原生只读测试验证。本次没有重新执行 SkillHub 安装，也没有打包或发布桌面安装包。代码仍在工作区，未提交。
