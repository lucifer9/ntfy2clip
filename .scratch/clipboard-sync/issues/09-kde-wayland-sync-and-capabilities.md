# 09: KDE Wayland 双向同步与能力提示

**What to build:** 在支持 data-control 的 KDE Wayland 桌面，用户可以后台监听本地复制并与其他 peer 双向同步；GNOME 和能力不足的环境明确显示自动上传不可用，继续保留当前可用的接收路径。

**Blocked by:** 03 — macOS 正常网络下的双向同步。

**Status:** ready-for-agent

**Execution:** 已实现 data-control 快照、受监督写入及 GNOME/缺能力提示；实际 KDE 三 peer 通过，GNOME 真机尚未验证。见[验证记录](../../../docs/clipboard-sync-validation.md)。

首版不开发 GNOME Shell 扩展或来源 ID 过滤，不以 XWayland 代替整个 Wayland 剪切板后声称完整支持。平台后端复用共享同步规则，不复制重试机制。

- [ ] 在目标 KDE/KWin 版本验证并记录 ext/wlr data-control、seat、文本 MIME 类型及实际无焦点访问能力；不仅凭 WAYLAND_DISPLAY 或 wl-copy 可执行便宣称能后台监听。
- [ ] 本地同步文本变化通过完整发布路径到达其他 peer；远端接收可以更新 KDE 的真实系统剪切板，并满足共享 hash/origin 规则。
- [ ] 支持空文本和 Unicode，正确区分无 selection、非文本、读取失败；尾部全部 CR/LF 处理与其他平台一致，其他空白不改写。
- [ ] 通知/快照的新鲜度、selection 生命周期和写入完成能够与共享协调流程配合；同值通知、自身写入及自身回送不再次发布。
- [ ] 若使用辅助命令，其消息边界、大小限制、失败和回收须明确；多行文本不能按换行错误分帧，不能依赖抢焦点弹窗作为稳定后台监听方案。
- [ ] GNOME 首版不启用自动上传；不支持必要协议或权限不足时明确报告请求模式、有效模式及原因。只保留实际可用的接收能力，不保证所有 GNOME 版本的命令写入都可用。
- [ ] 能力缺失、后台任务退出和连接断开可观察、可监督，不留下失效监听却显示正常的状态；不得静默切换到 XWayland 并扩大能力声明。
- [ ] 对协议存在/缺失、非法数据及生命周期写接口测试；在真实 KDE Wayland 上完成双向 smoke test，在 GNOME 或等价受限环境验证能力提示和可用接收路径。
- [ ] 补充验证过的 compositor/工具版本和依赖说明；通过受影响范围的格式、构建/类型检查、lint 和测试，缺少真实环境的验收项保持未完成。
