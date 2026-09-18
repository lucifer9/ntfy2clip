# 剪切板同步验证记录

日期：2026-09-18。全部使用合成文本；不保存或打印用户原有剪切板正文。区分代码实现、自动化验证和真实桌面证据，**尚未完成全部发布验收**。

用户后续明确取消 Windows/WSL 支持：tickets 06/07 已标记 `wontfix`，对应后端、桥接、直接 Win32 依赖及测试移除。此前 Windows/WSL 试验属于历史，不作为当前支持声明。WSL 普通启动明确拒绝，即使存在 WSLg 显示变量也不回退到 Linux selection。

## 环境与服务端

| 角色 | 实际环境 |
| --- | --- |
| A | 本机 macOS 27.0，build 26A428；SDK 27.0；Rust 1.98.1 |
| B | 第二台 macOS 27.0，build 26A428 |
| C | Arch Linux，Linux 7.2.4-arch1-2；KWin / plasma-workspace 6.7.5-1；wl-clipboard 2.3.0；wl_seat v10、ext_data_control_manager_v1 v1 |
| 隔离 X server | Xvfb + xclip，直接测试 LinuxClipboard/XFixes；**不是实际 Xorg 桌面验收** |
| 远程桌面 | A/B 两端 UU Remote 4.41.0；Screen Sharing 7.0；分别连接，用户确认开启转发并执行必要交互 |

Cargo.lock 固定 objc2-app-kit 0.3.2、x11rb 0.13.2、wl-clipboard-rs 0.9.3、tokio-tungstenite 0.30.0、reqwest 0.12.28 等依赖。

使用官方 `binwiederhier/ntfy` v2.28.0 Linux amd64 release，在测试 Linux 主机监听 loopback，经 SSH 转发给其他 peer。无认证/ACL、无 cache-file；除下列参数外均为该版本默认值：

```sh
ntfy serve --listen-http 127.0.0.1:18281 \
  --message-size-limit 4096 --attachment-total-size-limit 0 \
  --visitor-request-limit-burst 10000 --visitor-request-limit-replenish 1ms
```

只使用测试 topic。macOS 官方包不含 `serve`，不能作为该服务端的替代。客户端大小检查包含实际 JSON 转义，不假设服务端会自动告知上限。

`tests/ntfy.rs` 使用真实官方服务和项目假剪切板：三个独立订阅者轮换源端，合计三次发布，各 peer 各写入两次；源端保留原始尾部换行，另两端得到同步文本，多轮观察后计数不增长。这个测试不替代桌面证据。

## 真实桌面证据

macOS 与 KDE 已验证合成文本的发布、接收、Unicode、空文本和同值不回传。A/B/C 的广播路径在下面两个远程桌面组合中实际执行：A 放入文本，B/C 都断言相等，各写入一次，A 只发布一次。断言失败不输出剪切板内容。

`tests/native.rs --ignored` 验证 macOS 原生 helper 的 Unicode、空文本及退出回收。`tests/xorg.rs --ignored` 在隔离 Xvfb 上验证 Unicode、空文本、selection 持续可读、PRIMARY 不上传、远端规范化写入不回传、owner 回收。用户确认未提供实际 Xorg 桌面，接受先做隔离 X server 验证；实际桌面及 clipboard manager 接管仍未验收。

### 取消 Windows/WSL 后的三端复测

最终代码在 A/B 两台 macOS 与 C 的 KDE 桌面上轮流复制合成文本：A 的 Unicode 和尾部 CR/LF、B 的新值、C 的新值均到达另外两端。服务端 topic `n2c-final-abc` 合计 3 次发布，各 peer 各发布 1 次、写入 2 次；后续观察计数不增长。首次立即断言 A 的最终值时早于轮询传播，随后判等通过；fixture 已改为最多等待 5 秒的判等，失败仍不输出内容。

在 KDE 上设置 `XDG_CURRENT_DESKTOP=GNOME` 做策略测试：明确提示不支持自动上传并降为 receive；外部消息写入成功，随后本地复制没有发布。该测试只验证分支策略，不能作为真实 GNOME compositor 验收。

### UU Remote

先不在 B 运行 n2c，只在 A 放入 `n2c-UU-only-probe`。最初 B 判等失败；用户确认设置并执行交互后，B 判等成功。再次后台改写 A 时，B 未立即变化，不能假设该会话会立即转发每次 API 写入。

之后 A/B/C 都运行 n2c：A 放入 `n2c-UU-with-ntfy`，B/C 判等成功。服务端 1 次发布，A 发布一次，B/C 各写入一次。用户保持文本不变，触发 UU 焦点/粘贴操作，再检查计数仍为 1。

这验证了 **ntfy 先到、随后用户触发 UU 同值共享**。B 此时的类型为 `public.utf8-plain-text` 和 `NSStringPboardType`，未获得可靠来源过滤证据；不能据此推断 UU 在所有情况下都没有其他元数据。

### Screen Sharing

用户断开 UU、建立 A→B Screen Sharing 并确认共享剪切板。后台 API 改写 A 时，B 同样未立即得到测试值。等待 n2c 订阅就绪后，以 `n2c-screen-ready` 验证 A/B 相等，B 写入一次。

当时该 topic 累计 5 条合成消息；用户触发共享剪切板且不主动改变文本后，计数仍为 5，B 写入次数仍为 1。一次在新订阅建立前发送的文本未到达 B，符合“不请求历史”；在连接后发送的新值正常。

两种软件尚未完整执行“远程桌面先到”、延迟旧 X、非文本再复制、占用/超时和中间换行转换矩阵。共享状态测试不能替代这些真实会话验收。

## 自动化验证边界

- `receive.rs` / `protocol.rs`：生产 Receiver 入口、topic/event/tag、普通 JSON 文本、空文本、Unicode、尾部规则、坏封装/附件、序列化字节边界、失败后继续。
- `sync.rs` / `coordination.rs` / `inflight.rs`：基线、当前状态而非历史、读失败、早到/延迟自身回送、外部抢写、两类 FIFO、在途容量与字节限制、期限/次数/退避、全局冷却、新本地状态取消旧任务、写入完成前的并发入队。
- `publish.rs`：小型 TCP 假服务核对 POST topic、tag、正文和 200/401/429/503 分类。
- `ipc.rs` / `process.rs`：帧边界、多行/Unicode、畸形/超长输入、非零退出；通过 PID 和晚到标记证明超时后回收且不继续执行。
- `cli.rs`：非法配置失败、合成凭据不进日志，以及 WSL 不误用 WSLg。
- 真实桌面和官方 ntfy 测试标记 `#[ignore]`，普通测试运行不改用户剪切板。

### 最终检查结果

- macOS：`cargo check --locked`、`cargo clippy --all-targets --locked -- -D warnings` 通过；`cargo test --locked` 共 21 项通过、2 项忽略。另行运行原生 helper 与官方 ntfy 的忽略测试，均通过。
- Linux/KDE：同样的 check、clippy 和完整测试通过，共 22 项通过、2 项忽略。另行执行 Xvfb 测试通过，包含约 400 KB Unicode 的 INCR 传输。
- 真实三端复测前，比对三端 `src/sync.rs` 和 `src/platform/mod.rs` 源码校验和一致。

## 审查与修正

按固定基点 `73f0da3a43c81e898ffa9889992512af2b8c8080` 执行独立 Standards / Spec 审查。

- Standards：发现一处帧大小公式重复；改为协议模块统一提供边界，transport 与解码共用。
- Spec：发现首次基线误推进代次、未尝试旧任务错误改签代次两项 P1。先补生产 Receiver 回归测试，确认分别丢弃 B、覆盖本地新值的失败，再修复。首次基线不推进代次；真实新本地状态使所有旧待写任务失效，不区分是否已经尝试。测试与生产使用同一接收入口。
- 已知验收缺口保持可见，未把取消的平台或未提供的桌面当成通过项。

## 未完成验收

实际 Xorg 桌面、GNOME 真机、clipboard manager 完整接管组合，以及完整远程桌面故障/时序矩阵仍未通过。GNOME 的产品策略是明确不上传；受限环境能力提示不能证明任意 GNOME 版本上的写入可用。

没有承诺来源过滤、图片、历史补发、持久队列、恰好一次或全局最终一致。远程桌面丢失来源后带回旧内容仍可能覆盖新内容；必要时关闭其剪切板转发。
