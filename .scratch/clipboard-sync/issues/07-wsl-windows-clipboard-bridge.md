# 07: WSL 与 Windows 剪切板桥接（已取消）

**What to build:** 在 WSL 中运行的 n2c 能监听和更新用户实际使用的 Windows 剪切板，与其他 peer 双向同步；无需依靠频繁启动 PowerShell，也不会误同步 WSLg 的 Linux selection。

**Blocked by:** 06 — Windows 原生双向同步。

**Status:** wontfix

**Execution:** 用户后续明确取消 Windows/WSL 支持。桥接、Windows helper 和 clip.exe 路径已移除；WSL 启动明确拒绝。以下保留原始票据作为历史，下列验收不再适用。

复用 Windows 剪切板后端及共享同步规则；本票负责常驻 helper 的完整部署和 IPC 生命周期，不另造一套发布器或去重规则。

- [ ] WSL 检测仍优先于 Wayland/Xorg；在有 WSLg 环境变量时依然操作 Windows 剪切板，不默默切换目标。
- [ ] 提供可实际使用的 Windows helper 构建/部署和查找方式；缺失、不兼容或启动失败明确报告，并保留当前可用的仅接收路径。
- [ ] helper 常驻并复用 Windows 观察和读写能力；仅作为剪切板桥接，不自行连接 ntfy 或另行发布内容。
- [ ] 主进程与 helper 使用有长度边界的 stdin/stdout 消息，正确承载多行、空字符串和 Unicode；请求与响应可关联，通知具有可用于核对的时序信息。
- [ ] IPC 数据大小有界，畸形帧、超限帧、断流和 helper 异常退出显式处理；诊断不混入数据通道，不打印剪切板正文或凭据。
- [ ] helper 失效时监督和恢复行为明确；旧请求的完成、终止及回收能与主状态流程协调，不能在恢复后重放过期写入或静默重复注册监听。
- [ ] 首次基线不上传，正常复制可发布；远端写入、自身回送及同值远程桌面转发不会再上传。本地原始文本不因观察被重写。
- [ ] 完成 Windows 实际剪切板→WSL n2c→ntfy→另一 peer 及反方向验证，覆盖 helper 故障、帧边界和文本语义；注明测试的 WSL/Windows 版本。
- [ ] 通过相关 Linux/Windows 目标的构建、格式、lint 和测试，以及真实 WSL smoke test；未提供实际环境不能标记该平台运行验证通过。
