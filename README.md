# ntfy2clip

`n2c` 使用 ntfy topic 广播剪切板文本。默认仅接收；设置 `SYNC_MODE=bidirectional` 后监听本机并主动发布。只支持文本（包括空字符串），不下载附件、不同步图片或文件。

```sh
cargo build --release --locked
SERVER=ntfy.example.com TOPIC=my-clipboard target/release/n2c
SYNC_MODE=bidirectional SERVER=ntfy.example.com TOPIC=my-clipboard target/release/n2c
```

所有 peer 应一起升级。新版使用带 `ntfy2clip` tag 的 v1 JSON 正文，旧二进制会把封装当成文本；新版仍接收没有该 tag 的普通 ntfy 文本。origin 是每次启动的随机 UUID，不是认证身份。

## 文本与交付行为

只删除末尾全部连续 CR/LF，保留开头空白、中间换行、空格、Tab 和 Unicode。不使用通用 `trim`。只观察本地复制时不改写本机原始内容。该规则不能保证多行文本在终端粘贴时不会执行。

启动首次成功读取仅建立基线。空文本正常同步；系统清空、非文本不发送清空命令，但重置本地观察状态。读取失败保留最后有效状态。重复同值不上传，`X → Y → X` 的最后一个 X 仍上传。

本地状态、写入和完成登记串行协调；HTTP 发布与订阅独立运行。自身 origin 回送直接忽略；远端写入前重新读取当前剪切板，同值不写。成功只登记预期内容，后续外部抢写仍作为本地变化处理。

发送和接收各有内存 FIFO，计入在途任务的数量及字节。满时淘汰最旧的未开始任务；没有可淘汰空间时拒绝新任务。新观察到的本地状态取消较早的待写目标。HTTP 权限错误不重试，429 的 Retry-After 冷却作用于整个发布器；瞬时网络错误和暂时占用在预算内重试。

这是尽力交付：响应丢失可能重复发布，HTTP 成功不代表其他 peer 已写入。重连保留 origin 和队列；退出丢弃队列，不落盘、不恢复、不请求历史消息。订阅建立前或断线期间的消息不会补齐。

## 配置

`SERVER` 只接受主机和可选端口；`TOPIC` 是单个由字母、数字、下划线、连字符组成的 topic。发布使用同一 host/topic，`wss → https`，`ws → http`。`TOKEN` 必须具有所需的读/写权限；不要把凭据写入源码或分享日志。

| 环境变量 | 默认值 | 用途 |
| --- | --- | --- |
| SERVER | ntfy.sh | ntfy 主机及可选端口 |
| SCHEME | wss | wss / ws |
| TOPIC | 必填 | 单个 topic |
| TOKEN | 空 | Bearer token |
| TIMEOUT | 120 | 订阅无流量超时，秒；保留原有非法值回退行为 |
| RUST_LOG | info | 本项目日志级别 |
| DEV | 未设置 | 存在时启用 debug；macOS 同时改用 stderr，便于终端诊断 |
| SYNC_MODE | receive | receive / bidirectional |
| MAX_MESSAGE_BYTES | 4096 | 实际序列化正文的 UTF-8 字节上限，含 JSON 转义 |
| CLIPBOARD_POLL_MS | 250 | 重新检查当前快照的间隔 |
| SEND_QUEUE_MAX_MESSAGES | 128 | 发布队列数量 |
| SEND_QUEUE_MAX_BYTES | 8388608 | 发布正文总字节 |
| SEND_TTL_SECS | 60 | 发布任务总寿命 |
| PUBLISH_TIMEOUT_SECS | 10 | 单次 HTTP 超时 |
| PUBLISH_MAX_ATTEMPTS | 5 | 总尝试次数，含首次 |
| PUBLISH_RETRY_BASE_MS | 1000 | 指数退避起点 |
| PUBLISH_RETRY_MAX_MS | 8000 | 退避上限；Retry-After 可以更长 |
| RECEIVE_QUEUE_MAX_MESSAGES | 128 | 接收写入队列数量 |
| RECEIVE_QUEUE_MAX_BYTES | 8388608 | 待写文本总字节 |
| WRITE_TTL_MS | 5000 | 从接收起的写入总寿命 |
| WRITE_TIMEOUT_MS | 1000 | 单次写入预算 |
| WRITE_MAX_ATTEMPTS | 5 | 总写入尝试次数 |
| WRITE_RETRY_BASE_MS | 50 | 写入退避起点 |
| WRITE_RETRY_MAX_MS | 400 | 写入退避上限 |

新增数值配置必须在 `1..=4294967295`，退避起点不能大于上限；非法值报错退出。部署时将 `MAX_MESSAGE_BYTES` 与实际 ntfy 实例的 `message-size-limit` 匹配，不套用托管服务额度。超限拒绝，不截断或自动转附件。helper IPC 另有 16 MiB 帧上限。

## 平台与会话要求

| 平台 | 后端与依赖 |
| --- | --- |
| macOS | 自带常驻原生 helper，主线程使用 NSPasteboard，changeCount 校验快照。默认日志进入 `ntfyclip` unified log；`DEV=1` 使用 stderr。helper 不可用时保留 `/usr/bin/pbcopy` 接收路径。 |
| Windows / WSL | 不支持。WSL 即使设置了 WSLg 的显示变量，也会明确报错退出，不选择 Linux selection 作为替代。 |
| Xorg | Rust x11rb/XFixes 读取 CLIPBOARD（含有界 INCR 传输）；安装 `xclip` 写入。前台 selection owner 由主进程监督并保留到失去所有权/退出，PRIMARY 不参与同步。 |
| KDE Wayland | Rust wl-clipboard-rs 通过 ext/wlr data-control 和 seat 获取当前 offer；重新获取快照，拒绝传输期间可见的变化。写入需 `wl-copy`，以前台进程保有 selection。需要实际桌面用户的 `XDG_RUNTIME_DIR`、`WAYLAND_DISPLAY`。 |
| GNOME / 能力不足 | GNOME Wayland 首版不自动上传。缺协议、seat、显示连接或 helper 时报告请求/有效模式及限制，保留原有命令接收路径；不保证任意 GNOME 环境都能运行 `wl-copy`。不回退到 XWayland 假装支持整个 Wayland 剪切板。 |

Unix 构建需要工具链和对应 TLS 开发库（Linux 使用 native-tls/OpenSSL）；macOS 使用系统 SDK，WebSocket 仍使用 rustls。`Cargo.lock` 固定了测试过的依赖解析。

### 原生 helper 的生命周期

macOS/Linux 使用同一可执行文件的内部 helper 模式，以长度前缀、请求 id 和版本检查进行 stdin/stdout IPC；诊断不混入数据帧。启动握手最多 10 秒；操作失败后先终止、回收，再允许新请求，不重放旧请求。原生写入有进程内 watchdog。终止清理可能花费额外时间；终止失败会禁用该 helper，不能继续并发旧写入。升级前应停止 n2c，再替换可执行文件。

## 验证与已知边界

```sh
cargo fmt --check
cargo check --locked
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
# 以下测试会改写桌面剪切板，只在明确的测试会话运行：
cargo test --test native -- --ignored
# 受控官方 ntfy，仅使用随机测试 topic 和合成文本：
N2C_TEST_SERVER=127.0.0.1:18281 cargo test --test ntfy -- --ignored
# 隔离 X server；不等于实际 Xorg 桌面验收：
DISPLAY=:97 cargo test --test xorg -- --ignored
```

实际环境、版本、步骤和未验证项见 [验证记录](docs/clipboard-sync-validation.md)。尚未完成全平台发布验收。

轮询/通知后重新读取可能漏掉瞬时状态。hash + origin 不能识别已丢失来源的远程桌面旧值，也不建立全局顺序或最终一致性。Screen Sharing / UU Remote 可能需要焦点切换或粘贴才转发，可能改变中间换行或延迟回流。来源过滤未启用；持续回流时应关闭远程桌面剪切板转发，仅保留一个同步通道。

日志不输出正文、完整 hash、TOKEN 或认证请求；即使启用 debug，也不启用第三方 WebSocket/HTTP 的 payload 日志。请用合成文本复现问题，不提交真实剪切板内容。
