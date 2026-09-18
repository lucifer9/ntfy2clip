# 08: Xorg 双向同步

**What to build:** Xorg 用户在系统 CLIPBOARD 中复制文本后可经 ntfy 同步到其他 peer，远端内容也能写回本机；clipboard manager 接管和重复通知不会引发同值传播。

**Blocked by:** 03 — macOS 正常网络下的双向同步。

**Status:** ready-for-human

**Execution:** 已实现 XFixes/CLIPBOARD/INCR 与前台 selection owner。隔离 Xvfb 测试通过；用户确认实际 Xorg 桌面暂不可用，不标记该验收项完成。见[验证记录](../../../docs/clipboard-sync-validation.md)。

本票接入既有共享同步流程，不另写队列、网络重试或历史去重。仅处理 CLIPBOARD，不扩展到 PRIMARY。

- [ ] 在实际 Xorg 会话验证 XFixes 变更通知和文本读写，记录系统、工具及依赖版本；优先复用满足语义的已有命令，不以重构后端为目的。
- [ ] 本地同步文本变化能经 ntfy 到另一 peer，反方向能写入本机；启动基线、模式开关、hash 和 origin 使用共享规则。
- [ ] 只观察 CLIPBOARD，改变 PRIMARY 不产生上传；selection owner/timestamp 用于变化或新鲜度核对，不当成可靠来源应用身份。
- [ ] 统一移除同步文本末尾全部 CR/LF，其余空白保留；保留原有末尾换行处理意图，不能只靠单次 -r 删除实现跨平台文本规则。
- [ ] 正确区分空文本、无 selection、非文本和读取失败；验证 Unicode、多行内容、数据传输及 clipboard manager 接管。
- [ ] 写入进程退出码、后台 selection 所有者生命周期和关闭清理明确；不能将工具启动成功等同于内容写入成功，也不能破坏供后续粘贴使用的 selection 生命周期。
- [ ] 稳定快照与共享串行登记正确衔接；自己的写入通知、同值 selection 接管、自身回送不再次发布，积压旧快照不回灌。
- [ ] 缺少显示连接、扩展或工具时明确报告能力及可用接收范围，不静默声称双向同步正常；补充平台依赖说明。
- [ ] 通过真实 Xorg 的双向 smoke test、相关通知/失败的接口测试，以及受影响范围的格式、构建/类型检查、lint 和测试；平台未实测不得作为通过记录。
