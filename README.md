# ntfy2clip

This tiny app connects to [ntfy.sh](https://ntfy.sh) or a [self-hosted](https://docs.ntfy.sh/install/) ntfy server through websocket.  
When it receives a text message of a specified topic, it automatically updates  
the system clipboard with the message content.  
It's easy to modify it to work on different platforms and/or to support more message types.

## Implementations

This project has multiple implementations:

- **`master` branch**: Rust implementation using `tokio` and `tokio-tungstenite`
- **`zig` branch**: Zig implementation
- **`go` branch**: Go implementation using `coder/websocket`

All implementations share the same configuration and behavior.

## Installation

### Go Version (this branch)

```bash
go install github.com/lucifer/ntfy2clip@latest
```

Or build from source:

```bash
git clone https://github.com/lucifer/ntfy2clip.git
cd ntfy2clip
git checkout go

# Using Makefile (recommended)
make help              # Show available targets
make                   # Build for current platform (native)
make linux-amd64       # Build for Linux AMD64 (GOAMD64=v3)
make linux-amd64-v4    # Build for Linux AMD64 (GOAMD64=v4)
make linux-arm64       # Build for Linux ARM64
make darwin-arm64      # Build for macOS ARM64 (Apple Silicon)
make darwin-amd64      # Build for macOS AMD64 (GOAMD64=v3)
make clean             # Remove build artifacts

# Or manually with Go
go build -o n2c ./cmd/n2c
```

**Build features:**
- Static binary (`CGO_ENABLED=0`)
- Stripped symbols (`-ldflags="-s -w"`)
- Optimized for modern CPUs (AMD64 v3/v4 microarchitecture levels)
- Binary size: ~5.6-5.9MB (vs ~8.6MB without optimizations)

## Configuration

Configuration is managed through Environment Variables:

- `SERVER`: your self-hosted ntfy server, or `ntfy.sh` by default
- `SCHEME`: `wss` by default, can be `ws` for servers without TLS
- `TOPIC`: to which you subscribe, multiple topics are **NOT** supported for now
- `TOKEN`: your access token, if needed
- `TIMEOUT`: timeout in seconds (default: 120)

## Platform Support

The Go implementation supports:

- **macOS**: Uses `pbcopy`
- **Linux (X11)**: Uses `xclip`
- **Linux (Wayland)**: Uses `wl-copy`
- **Linux (WSL)**: Uses Windows `clip.exe`
- **Windows**: Uses `clip.exe`

## Usage

```bash
export TOPIC=mytopic
./n2c
```

With custom server and token:

```bash
export SERVER=ntfy.example.com
export TOPIC=mytopic
export TOKEN=tk_mytoken123
./n2c
```

## Running as a Service

### macOS (launchd)

Create `~/Library/LaunchAgents/com.example.n2c.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.example.n2c</string>
    
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/n2c</string>
    </array>
    
    <key>EnvironmentVariables</key>
    <dict>
        <key>TOPIC</key>
        <string>mytopic</string>
    </dict>
    
    <key>StandardOutPath</key>
    <string>/tmp/n2c.stdout</string>
    
    <key>StandardErrorPath</key>
    <string>/tmp/n2c.stderr</string>
    
    <key>RunAtLoad</key>
    <true/>
    
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
```

Load the service:

```bash
launchctl load ~/Library/LaunchAgents/com.example.n2c.plist
```

View logs:

```bash
tail -f /tmp/n2c.stderr
# Or use macOS unified logging
log show --predicate 'process == "n2c"' --info --last 1h
```

## How It Works

The ntfy server will send Ping frames, so we only need to return a Pong normally,  
there is no need to actively send Pings to maintain the connection. And of course  
if there's no activity for over 120 seconds, we will try a reconnect.
This interval is adjustable via the `TIMEOUT` environment variable.
