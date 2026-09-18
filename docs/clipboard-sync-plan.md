# 剪切板同步调研记录

> 范围变更：用户已取消 Windows/WSL 支持。下文相关内容仅保留为历史调研，不构成当前支持承诺；当前范围以正式规格为准。

本文件保留调查依据和能力边界。已确认行为、默认值与实施顺序以 [正式规格](../.scratch/clipboard-sync/spec.md) 为准；术语见 [CONTEXT.md](../CONTEXT.md)，协议取舍见 [ADR-0001](adr/0001-versioned-text-sync.md)。早期的初步方案已由正式规格取代。

调查基于本仓库、官方文档和上游源码。没有向用户的自建 ntfy 或 ntfy.sh 发布测试消息，没有读取用户当前剪切板，也没有完成远程桌面或多平台运行测试。上游 main/master 和本机 SDK 不能代表目标部署版本，实施前仍需核对。

## 1. 仓库现状

`Cargo.toml`：Rust 2024，Tokio 1，tokio-tungstenite 使用 `*`，仓库没有 Cargo.lock；尚无 HTTP 发布客户端或剪切板监听依赖。

`src/client.rs` 当前只接收文本：订阅 `/{topic}/ws`，收到 `event=message` 后调用外部程序写入剪切板。

| 环境 | 当前写入方式 |
| --- | --- |
| macOS | `/usr/bin/pbcopy` |
| Windows | `clip.exe` |
| WSL | `/mnt/c/Windows/System32/clip.exe` |
| Wayland | `/usr/bin/wl-copy` |
| Xorg | `/usr/bin/xclip -sel clip -r -in` |

WSL 检测优先于 Wayland/Xorg，目标是 Windows 剪切板，不能无意中换成 WSLg selection。

实施直接相关的现状：

- 每条接收消息独立 spawn，剪切板写入没有顺序保证。
- child.wait() 未检查退出码，不能以此确认写入成功。
- 日志输出正文，Debug 还输出带 Authorization 的请求；相关路径需停止记录正文与凭据。
- 当前订阅没有 since 游标，不自动补齐断线期间消息。

本次只编写规划文档，未修改这些代码。

## 2. ntfy 的多订阅者语义

**相同 topic 的多个在线订阅者都会收到发布消息，不是竞争消费。** 官方订阅 API 支持 WebSocket；上游 `server/topic.go` 为每个订阅注册回调，Publish 复制并遍历所有订阅者，分别分发消息，没有“其中一个收到便消费掉”的逻辑。[1][2]

A、B、C 同时订阅时，A 发布一次，B/C 均可收到，A 自己也会收到回送。前提是连接正常、权限允许且未触发服务限制。这不等于恰好一次交付、离线自动补齐或各 peer 的全局总顺序。

HTTP POST/PUT 发布与 WebSocket 订阅可以共用 topic 和认证。自建实例的 TOKEN 需具备相应读写权限；不直接套用 ntfy.sh 的每日额度，但自建也可能配置消息大小、请求速率、连接数和缓存限制。[1][3]

上述是文档/源码证据，不是服务实例实测。实施验证需三个独立订阅连接，以合成文本检查 fan-out、自身回送及发布成功和 peer 写入成功的区别。

## 3. 文本与附件限制

ntfy 文档列出的默认普通消息上限为 4,096 字节，自建以实际配置为准。超过上限的直接上传文本可能转成附件；当前程序只提取 message，不下载附件，不能把附件说明当成原文。[3]

上游 handleBodyAsTextMessage 对普通正文调用 strings.TrimSpace；JSON 发布入口也将 message 转交普通正文处理，所以仅改用 ntfy 根路径的 JSON 发布 API 不能保住首尾空白。[4]

正式规格采用应用级 JSON 封装，让需要保留的空白处于 JSON 字符串内部，并通过 ntfy 支持的 tags 明确识别协议。不能假设任意自定义 HTTP 请求头会被 ntfy 转发到订阅者。[1][3]

ntfy 可以通过附件传输图片，但用户已排除首版图片同步；本项目也不为超长文本增加附件或分片通道。超限处理和空文本语义见正式规格。

### 尾部换行是有意的行为

xclip 上游手册明确：`-r` / `-rmlastnl` 删除最后一个换行，以便将命令输出粘贴到命令提示符时不因末尾换行立即执行。[20]

用户确认需要保留这一意图，并将规则统一为所有平台移除末尾全部连续 CR/LF；它不是待修复的无意损坏。同步文本的比较、发布和写入必须使用同一规则，否则写入前后 hash 不同会干扰同值抑制。其他空白仍需保留；规则的具体例子以正式规格为准。

## 4. 平台监听能力

| 环境 | 调查结论 | 待验证边界 |
| --- | --- | --- |
| macOS | NSPasteboard changeCount 可检测版本变化，多类型 pasteboard item 有公开接口。[5][13] | 轮询可能漏掉短暂状态；原生线程及新版本访问权限需实测。changeCount 不证明写入来源。 |
| Windows | AddClipboardFormatListener / WM_CLIPBOARDUPDATE 提供通知；sequence number 可校验变化，OpenClipboard 参与读写协调。[6][7][8] | 处理占用、Unicode、有界操作与消息循环；sequence number 不是来源身份。 |
| WSL | 官方支持从 WSL 启动 Windows 程序，因而可用常驻 helper 接入 Windows 剪切板。[9] | helper 部署、IPC 帧边界与故障恢复尚未验证；Linux selection 不等于 Windows 剪切板。 |
| Xorg | XFixes 通知 CLIPBOARD selection owner 变化，事件含 owner、timestamp 等。[10] | owner 是窗口，不是可靠应用身份；clipboard manager 接管及数据读取仍需验证。 |
| KDE Wayland | 当前 KWin 上游实现了 ext-data-control；wl-paste --watch 文档要求相应 data-control 能力。[11][12][16] | 以目标 compositor、协议和工具版本为准；wl-copy 可用不代表后台监听可用。 |
| GNOME Wayland | 所查 Mutter main 初始化与构建清单没有 data-control 实现；Shell 内扩展可通过 selection owner-changed 监听。[17][18] | 不能据此承诺所有版本相同。用户决定首版不做 GNOME 自动上传，不开发配套扩展。 |

系统剪切板保存当前状态，不是复制历史。即使有通知，读取时中间内容也可能已被覆盖；必须区分“尽力发送已观察到的变化”与“捕获每次系统写入”。

## 5. 剪切板是否提供写入者 ID

### macOS

核对 Apple NSPasteboard 文档和本机 macOS 26.5/27.0 AppKit 头文件，没有发现可查询当前写入者 PID/bundle ID 的公共接口。`declareTypes:owner:` 的 owner 是延迟数据提供对象，不是获取其他应用身份的接口。[13]

`org.nspasteboard.source` 是应用自愿填写的来源约定，可包含 bundle ID；约定允许保留原始来源，也允许空字符串。这不是系统保证的最后写入者身份，不应当作认证依据，也不能在缺失时用前台应用推断。[14]

约定文档还记录了 Apple Handoff 的 `com.apple.is-remote-clipboard` 类型，但这不证明 Screen Sharing 或 UU Remote 会写入它。两款软件提供的实际类型/来源值，需要使用合成文本分别验证。[14]

### KDE/GNOME Wayland

ext-data-control 和 wlr-data-control 的 selection/data offer 提供对象、MIME 类型及数据传输，不提供通用的来源 PID/app_id。offer ID 不能用于“忽略某应用”。KWin 上游标准实现也没有在该接口增加来源身份字段。[12][15][16]

Clipboard portal 扩展 RemoteDesktop/InputCapture 等会话，需要授予剪切板权限，并不独立提供无条件后台监听会话。其 SelectionOwnerChanged 中的 session_is_owner 只说明本会话是否为 owner，不返回其他应用身份。[19]

**结论：没有证据支持通用、跨这些平台的可靠“忽略应用 ID”功能。** 用户选择先验证真实元数据，未证实前不加入首版过滤配置。来源未知的内容仍按内容 hash + origin 处理，不能假装已经过滤。

## 6. 防同值回环与剩余边界

内容比较和来源身份解决不同问题。远程桌面可能只传文本，系统私有格式不能作为防回流前提。

当前内容 hash 能抑制相同同步文本的重复发布和写入；网络 origin 能过滤本实例的延迟回送。历史 hash 会误过滤 X→Y→X，因此用户明确不采用。

它们不能识别所有旧内容：A 复制 X 后又复制 Y，B 经远程桌面获得的旧 X 此时才发布，A 会看到不同 origin、不同当前 hash。没有因果信息时，这与 B 主动复制 X 无法可靠区分。重试导致的非连续重复消息也有相似边界。

用户接受有界重试和偶发重复通知，不增加 event_id 或全局排序；本地新变化优先于尚未完成的旧写入重试。具体预算、取消与报告规则见正式规格。若实际远程桌面持续回流不同状态，关闭其剪切板转发、只保留本项目通道，是可操作的消环方式。

## 来源

[1] ntfy 订阅 API：https://docs.ntfy.sh/subscribe/api/

[2] ntfy topic 发布源码：https://github.com/binwiederhier/ntfy/blob/main/server/topic.go

[3] ntfy 发布 API、tags、认证与限制：https://docs.ntfy.sh/publish/ ，https://docs.ntfy.sh/publish/#limitations

[4] ntfy 正文及 JSON 发布处理：https://github.com/binwiederhier/ntfy/blob/main/server/server.go （handleBodyAsTextMessage、JSON 发布入口）

[5] Apple Pasteboard Concepts：https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/PasteboardGuide106/Articles/pbConcepts.html

[6] Microsoft Using the Clipboard：https://learn.microsoft.com/en-us/windows/win32/dataxchg/using-the-clipboard

[7] Microsoft Clipboard Formats：https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats

[8] Microsoft OpenClipboard：https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-openclipboard

[9] Microsoft WSL interoperability：https://learn.microsoft.com/en-us/windows/wsl/filesystems#run-windows-tools-from-linux

[10] X.Org XFixes protocol：https://www.x.org/releases/current/doc/fixesproto/fixesproto.txt

[11] wl-clipboard 上游手册：https://github.com/bugaevc/wl-clipboard/blob/master/data/wl-clipboard.1

[12] Wayland ext-data-control 协议：https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/staging/ext-data-control/ext-data-control-v1.xml （读取镜像：https://raw.githubusercontent.com/wayland-mirror/wayland-protocols/main/staging/ext-data-control/ext-data-control-v1.xml）

[13] Apple NSPasteboard API：https://developer.apple.com/documentation/appkit/nspasteboard （结构化文档：https://developer.apple.com/tutorials/data/documentation/appkit/nspasteboard.json）；另核对本机 macOS 26.5/27.0 SDK 的 AppKit NSPasteboard.h。

[14] NSPasteboard 来源与特殊类型约定：https://nspasteboard.org/ （约定维护者的一手说明，不是 Apple 系统保证）

[15] wlr-data-control 协议：https://github.com/swaywm/wlr-protocols/blob/master/unstable/wlr-data-control-unstable-v1.xml

[16] KWin data-control 实现：https://github.com/KDE/kwin/blob/master/src/wayland/datacontroldevice_v1.cpp ，https://github.com/KDE/kwin/blob/master/src/wayland/datacontroloffer_v1.cpp

[17] Mutter 初始化与构建清单：https://github.com/GNOME/mutter/blob/main/src/wayland/meta-wayland.c ，https://github.com/GNOME/mutter/blob/main/src/meson.build

[18] GNOME Clipboard Indicator 实现：https://github.com/Tudmotu/gnome-shell-extension-clipboard-indicator/blob/master/extension.js

[19] Clipboard portal 接口：https://github.com/flatpak/xdg-desktop-portal/blob/main/data/org.freedesktop.portal.Clipboard.xml

[20] xclip 上游手册：https://github.com/astrand/xclip/blob/master/xclip.1
