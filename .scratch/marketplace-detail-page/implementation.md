GATE: OPEN 11 (rounds used up)

# 市场详情页实施与验证

日期：2026-11-11。
依据：[已确认规格](spec.md)。

## 实施结果

- 市场列表与详情使用不同主内容视图。列表保留挂载及查询状态，返回时恢复滚动位置和打开详情的按钮焦点。
- `marketplace-detail-page.tsx` 展示平台身份、版本、真实统计、运行要求和完整 SKILL.md；支持原文切换。精简操作栏吸顶，阅读区域使用应用既有白色表面与字体。
- `marketplace-install-dialog.tsx` 复用既有安装预览、版本核实、Mutation surface、依赖引导、进度与结果；安装预览返回的详情更新同一市场条目的查询缓存。
- 安装弹窗打开时固定 Scope，背景页面与应用页头设为 inert，Scope 切换禁用。关闭后恢复主要操作按钮焦点且不滚动页面。
- 阅读页不按 Scope 重建；切换全局与项目仅更新安装状态及目标。新 Scope 清单未就绪时安装不可用。
- 收藏与下载统计抽为共用组件，缺失值与零沿用列表语义。Markdown 代码块可键盘聚焦，便于横向滚动；继续沿用现有内容清理及 URL 规则。
- 预览增加 `?marketplace` 和 `?marketplaceLong` 场景，长文来自开发用 fixture，包含代码块和较长说明，便于复核布局。未增加图片、营销动画、评分或远程文件能力。

## 自动验证

| 检查 | 结果 |
| --- | --- |
| `pnpm test` | 42 个测试文件、399 项测试通过 |
| `pnpm build` | TypeScript 与生产构建通过；Vite 仍提示主包超过 500 kB |
| `git diff --check` | 通过 |
| 市场测试 | 23 项通过，包含新增阅读页、查询与列表位置恢复、跨 Scope 加载隔离、安装结果与返回、预览期间关闭限制、详情失败重试及缺失统计 |

此次未改后端安装协议，也未重新执行真实 SkillHub CLI 安装。浏览器安装流程使用项目共用 Fake backend。

## 浏览器预览

使用 agent-browser 独立会话，在 `http://127.0.0.1:1420/preview.html?marketplaceLong` 验证：

- 1440×900、1024×768、768×1024、390×844、320×844 下，文档与主阅读区域均无横向溢出；见[尺寸测量](browser-layout.json)。
- 长文滚动后，精简操作栏与应用页头相接，返回及主要操作仍可见。
- 关闭安装弹窗保留阅读位置。预览中安装成功后保留结果，点击「返回详情」后主要操作变为「查看已安装技能」且焦点回到该按钮。
- 真实浏览器切换到示例项目后，阅读位置保持在 460px，安装状态与目标随 Scope 更新；安装弹窗内显示明确项目名，页头 inert、Scope 控件禁用。关闭后仍在 460px，焦点回到安装按钮。
- axe-core 4.12.1 的 WCAG 2 A/AA 检查：0 violations、0 incomplete，22 passes；见[检查结果](a11y.json)。

截图：[桌面](desktop.png)、[1024px](medium.png)、[768px](tablet.png)、[390px](narrow.png)、[320px](minimum.png)、[长文滚动](desktop-scroll.png)、[窄窗口安装预览](narrow-install.png)。

## 设计技能检查器的限制

design-taste-frontend 的 gate 已运行三轮，最终为 `GATE: OPEN 11 (rounds used up)`，不是通过。没有添加 waiver，也没有修改检查器。

| 未通过项 | 解释 |
| --- | --- |
| `moment-none` | 要求营销页动画段落；用户确认的范围是既有桌面应用的文档阅读页 |
| `hero-screen`、`section-height`、`section-air`、`one-section`、`one-heading`、`container-edges`、`row-lines`、`split-pair` | 要求营销页的全屏 hero、固定屏高 section 和 kit 容器；与已确认的连续正文阅读、吸顶操作栏及应用原有布局不符 |
| `tap-size` | 指向应用原有页头 65×28px 的作用域按钮；本次市场详情页的新按钮在窄窗口中已使用至少 44px 高度，原有应用页头未重做 |
| `off-grid` | 指向应用原有页头 12px 边距与详情正文边距不同；详情本身左右对称，沿用原应用导航 |

最终 gate 的 `check.mjs` 子检查另报告 `Runtime.evaluate: Inspected target navigated or closed`，该项未完成；上述尺寸与交互结论来自独立 agent-browser 测量、截图及人工检查。检查器的营销结构约束不作为本次已确认应用规格的替代验收条件。
