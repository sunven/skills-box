# 项目批量更新的技能 ID 回归

Date: 2026-11-11
Status: resolved

项目列表选中技能后打开「批量更新」，预览失败并显示
`无效的 skill 标识或路径。skill id 不属于当前项目 scope：bmad`。

根因是 `commands::preview_batch` 将完整项目技能 ID 解析成目录名后，
又将目录名传给要求完整 ID 的 `ProjectUpdateChecks::check`。
修复保留传入项目检查器的完整 ID，原有 scope 校验继续拒绝裸名称和其他项目的 ID。
全局检查仍使用目录名。

将命令实现提取到同文件的私有函数，使测试覆盖实际命令到项目检查器、
批量计划和命令预览的链路。最小复现和两个已安装技能的完整场景均先运行失败，
错误详情与截图一致，修复后通过。

回归覆盖项目锁 v1/v2、从真实库存选择两个技能、保留目标 ID 和选择顺序、
项目 CLI 目录与参数、预览保留技能文件和锁、无前缀及跨项目 ID 拒绝，
以及已经消失的技能作为跳过目标返回。测试使用临时项目、固定来源归档和记录型执行器。

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml --quiet`：766 项通过，13 项按原配置跳过。
- `pnpm test src/App.batch.test.tsx src/hooks/use-batch-operation.test.tsx src/App.project-check-lifecycle.test.tsx`：3 个文件、39 项通过。
- 新测试文件的 `rustfmt --check` 和 `git diff --check` 通过。

验证针对源码与自动化测试，未重新打包或替换正在运行的桌面应用。
