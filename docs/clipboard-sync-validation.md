# 剪切板同步验证记录

日期：2026-09-18。全部使用合成文本；没有保存或打印用户原有剪切板正文。以下区分代码实现、自动化验证和真实桌面证据；**尚未完成全部发布验收**。

## 环境

| 角色 | 实际环境 |
| --- | --- |
| A | 本机 macOS 27.0，build 26A428；SDK 27.0；Rust 1.98.1 |
| B | Windows 11，10.0.26100.9445；原生 n2c.exe，以及 Arch WSL、Linux 6.18.33.2-microsoft-standard-WSL2 下的 n2c + 同版 Windows helper |
| C | Arch Linux，Linux 7.2.4-arch1-2；KWin / plasma-workspace 6.7.5-1；wl-clipboard 2.3.0；wl_seat v10、ext_data_control_manager_v1 v1 |
| D | 第二台 macOS 27.0，build 26A428；与 A 建立远程桌面会话 |
| 隔离 X server | WSL 中 Xvfb :97 + xclip，直接测试 LinuxClipboard/XFixes；**不是 WSL 产品选路，也不是实际 Xorg 桌面验收** |
| 远程桌面 | A/D 两端 UU Remote 4.41.0；Screen Sharing 7.0，分别连接，用户确认开启剪切板转发并执行必要交互 |

依赖解析固定在 Cargo.lock：objc2-app-kit 0.3.2、windows-sys 0.61.2、x11rb 0.13.2、wl-clipboard-rs 0.9.3、tokio-tungstenite 0.30.0、reqwest 0.12.28。Windows GNU 目标在 WSL 交叉构建，并在实际 Windows 会话运行；不能只凭交叉编译通过得出运行结论。

## 受控 ntfy

从官方 `binwiederhier/ntfy` v2.28.0 release 下载 Linux amd64 二进制，在 WSL 使用下列配置临时运行，仅监听 loopback，通过 SSH 转发给测试 peer：

```sh
ntfy serve --listen-http 127.0.0.1:18280 \
  --message-size-limit 4096 --attachment-total-size-limit 0 \
  --visitor-request-limit-burst 10000 --visitor-request-limit-replenish 1ms
```

未启用认证/ACL，测试 topic 可读写；无 cache-file，其他设置为该版本默认值。客户端检查实际 JSON 正文字节，服务端大小配置显式为 4096。macOS 官方包只有客户端，不能用它执行 `serve`。

`tests/ntfy.rs` 使用随机 topic：三个独立订阅者轮换源端，每次仅一次发布，源端保留本地原始尾部换行，另外两端收到同步文本。三个 peer 最终各写入两次，发布总数三次，经过多个轮询周期未增长。这个测试使用真实官方 ntfy 和项目假剪切板，不等同于桌面测试。

## 已执行的真实路径

### macOS / WSL / KDE 三 peer

A 为 macOS，B 为 WSL→Windows，C 为 KDE Wayland。建立各自基线，等待订阅连接后，以 `clipboard_fixture` 写入固定合成文本，使用另一端 fixture 的只判等模式验证，不输出实际内容。

- macOS 发起含中文、emoji、前后空格的文本，Windows 与 KDE 断言通过。
- Windows 剪切板发起 `n2c-origin-windows`，macOS 与 KDE 断言通过。
- KDE 发起 `n2c-origin-kde`，macOS 与 Windows 断言通过。
- 计入之前一次明确的基线后复制，服务端该 topic 共 4 次发布；随后持续多个观察周期仍为 4。没有无限回流。
- `tests/native.rs` 在 macOS 和 Windows helper 上验证 Unicode、空文本及 helper 回收。

另以 **Windows 原生 n2c.exe** 和 macOS n2c 运行双向测试：Windows 发起与 macOS 发起各一次，两端判等通过；ntfy 共 2 次发布，观察后仍为 2。Windows 的订阅、HTTP 发布和原生 helper 均在 Windows 进程运行，不只是 Linux transport 调用 Windows clipboard。

### Xorg

`tests/xorg.rs --ignored` 在隔离 Xvfb 上通过：Unicode、空文本、selection 持续可读、PRIMARY 不产生上传、远端文本规范化写入后不再发布、owner 进程回收。

用户确认没有提供实际 Xorg 桌面，接受先做隔离服务器验证。clipboard manager 接管与实际桌面路径仍未验收；ticket 08 的这部分保持未完成。

### UU Remote

先不在 D 运行 n2c，只在 A 放入 `n2c-UU-only-probe`。最初 D 判等失败；用户确认设置并执行必要交互后，D 判等成功。再次通过后台 API 放入新文本时，D 未立即变化，说明该会话不能按“任意后台改写都会立即转发”假设测试。

之后 A/D/C 都运行 n2c：A 放入 `n2c-UU-with-ntfy`，D/C 判等成功。服务端 1 次发布，A 发布一次，D/C 各写入一次。用户保持文本不变，触发 UU 的焦点/粘贴操作后，再检查计数仍为 1，没有额外发布或写入增长。

该观察验证 **ntfy 先到、随后用户触发 UU 同值共享** 的组合。D 此时观察到的类型为 `public.utf8-plain-text` 和 `NSStringPboardType`，没有获得可用于可靠来源过滤的证据；这不证明 UU 在所有情况下都不提供其他元数据。

### Screen Sharing

用户断开 UU、建立 A→D 的 Screen Sharing 并确认开启共享剪切板。仅用后台 API 改写 A 时，D 未立即收到测试值。之后等待 n2c 订阅就绪，使用 `n2c-screen-ready`，A/D 判等通过，D 写入一次。

当时同一测试 topic 累计 5 条合成消息；用户触发共享剪切板操作、保持文本不变后，计数仍为 5，D 写入次数仍为 1。没有同值放大。一次在新订阅建立前发送的文本未到达 D，符合“不请求历史”的语义，重新在连接后发送的新值正常。

这里只完成 **ntfy 先到、随后触发 Screen Sharing 同值共享**。尚未完整执行两种软件的“远程桌面先到”、延迟旧 X、非文本重新复制、占用/超时和中间换行转换矩阵；这些行为不能因共享核心测试通过就标记为真实远程桌面验收通过。

## 自动化边界

- `receive.rs` / `protocol.rs`：实际 Coordinator 接收入口、topic/event/tag、普通 JSON 文本、空文本、Unicode、尾部规则、坏封装/附件、序列化字节边界、失败后继续。
- `sync.rs` / `coordination.rs`：基线、当前状态而非历史集合、读失败、自身回送早于 HTTP 完成及延迟回送、外部抢写、两类 FIFO、在途容量、字节拒绝、次数/期限/退避、429 全局冷却、新本地状态取消旧重试。时钟可控。
- `publish.rs`：项目 HTTP 边界，小型 TCP 假服务核对 POST topic、tag、正文和 200/401/429/503 分类。
- `ipc.rs` / `process.rs`：帧边界、多行/Unicode、畸形/超长输入、进程非零退出；用进程 PID 和晚到标记证明超时后已回收且没有继续执行。
- `cli.rs`：非法配置失败及合成凭据不进入日志。
- 真实桌面和官方 ntfy 测试使用 `#[ignore]`，普通测试运行不改用户剪切板。

## 发现并处理的问题

- SSH 进入 WSL 时可能没有 `WSL_DISTRO_NAME`；新增 Microsoft 内核识别，继续优先 Windows 剪切板。
- Windows 后续出现 `OpenClipboard code=5` 且没有占用窗口；用户解锁后恢复。不能把读取失败当作清空。
- Windows 仍持有旧 exe image 时，覆盖 WSL 路径后可继续得到旧 IPC 响应。停止旧实例、使用新路径后，同版协议握手及读写通过。
- 跨机拷贝保留 mtime、又与编译重叠时，Cargo 曾复用旧产物。后续测试构建在同步完成后触发，检查源文件 checksum，避免以旧二进制冒充新实现。
- 使用 macOS unified log 的同时，限制日志来源为本项目，避免 DEV 模式打开依赖库的 WebSocket 正文日志。

## 尚未完成的验收

实际 Xorg 桌面、GNOME 真机、clipboard manager 的完整接管组合，以及上述远程桌面完整故障/时序矩阵尚未通过。GNOME 的产品策略是明确不上传；受限环境的能力提示不能替代任意 GNOME 版本上的写入验证。

没有承诺来源过滤、图片、历史补发、持久队列、恰好一次或全局最终一致。远程桌面丢失来源后重新带回旧内容仍可能覆盖新内容；必要时关闭其剪切板转发。
