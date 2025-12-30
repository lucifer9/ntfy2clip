const std = @import("std");
const websocket = @import("websocket");
const log = std.log;

const Config = struct {
    server: []const u8,
    scheme: []const u8,
    topic: []const u8,
    token: ?[]const u8,
    timeout: u64,
};

fn getEnv(key: []const u8, default: ?[]const u8) ?[]const u8 {
    return std.posix.getenv(key) orelse default;
}

fn getConfig() !Config {
    const topic = getEnv("TOPIC", null) orelse {
        log.err("TOPIC environment variable is required", .{});
        return error.MissingTopic;
    };

    const timeout_str = getEnv("TIMEOUT", "120").?;
    const timeout = std.fmt.parseInt(u64, timeout_str, 10) catch 120;

    return Config{
        .server = getEnv("SERVER", "ntfy.sh").?,
        .scheme = getEnv("SCHEME", "wss").?,
        .topic = topic,
        .token = getEnv("TOKEN", null),
        .timeout = if (timeout > 0) timeout else 120,
    };
}

const ClipboardCommand = struct {
    argv: []const []const u8,
    env_name: []const u8,
};

fn getClipboardCommand() !ClipboardCommand {
    const builtin = @import("builtin");

    if (builtin.os.tag == .macos) {
        return ClipboardCommand{
            .argv = &[_][]const u8{"/usr/bin/pbcopy"},
            .env_name = "macOS",
        };
    } else if (builtin.os.tag == .windows) {
        return ClipboardCommand{
            .argv = &[_][]const u8{"clip.exe"},
            .env_name = "Windows",
        };
    } else if (builtin.os.tag == .linux) {
        // Check for WSL
        if (std.posix.getenv("WSL_DISTRO_NAME") != null) {
            return ClipboardCommand{
                .argv = &[_][]const u8{"/mnt/c/Windows/System32/clip.exe"},
                .env_name = "WSL",
            };
        }
        // Check for Wayland
        if (std.posix.getenv("WAYLAND_DISPLAY") != null) {
            return ClipboardCommand{
                .argv = &[_][]const u8{"/usr/bin/wl-copy"},
                .env_name = "Wayland",
            };
        }
        // Check for X11
        if (std.posix.getenv("DISPLAY") != null) {
            return ClipboardCommand{
                .argv = &[_][]const u8{ "/usr/bin/xclip", "-sel", "clip", "-r", "-in" },
                .env_name = "Xorg",
            };
        }
        log.err("Unsupported Linux environment (no WAYLAND_DISPLAY or DISPLAY)", .{});
        return error.UnsupportedEnvironment;
    } else {
        log.err("Unsupported operating system", .{});
        return error.UnsupportedOS;
    }
}

fn setClip(allocator: std.mem.Allocator, content: []const u8) !void {
    log.info("Setting clipboard to: {s}", .{content});

    const clip_cmd = try getClipboardCommand();
    log.debug("Running under {s}, using copy command {s}", .{ clip_cmd.env_name, clip_cmd.argv[0] });

    var child = std.process.Child.init(clip_cmd.argv, allocator);
    child.stdin_behavior = .Pipe;

    try child.spawn();

    if (child.stdin) |stdin| {
        stdin.writeAll(content) catch |err| {
            log.err("Failed to write to clipboard stdin: {}", .{err});
            return err;
        };
        stdin.close();
        child.stdin = null;
    }

    _ = child.wait() catch |err| {
        log.err("Failed to wait for clipboard process: {}", .{err});
        return err;
    };
}

const Handler = struct {
    allocator: std.mem.Allocator,
    client: *websocket.Client,
    config: Config,
    last_traffic: i64,

    pub fn serverMessage(self: *Handler, data: []const u8) !void {
        self.last_traffic = std.time.timestamp();

        const parsed = std.json.parseFromSlice(struct {
            event: []const u8,
            topic: []const u8,
            message: ?[]const u8 = null,
        }, self.allocator, data, .{ .ignore_unknown_fields = true }) catch |err| {
            log.err("Error parsing JSON: {}", .{err});
            return;
        };
        defer parsed.deinit();

        const msg = parsed.value;
        if (std.mem.eql(u8, msg.topic, self.config.topic) and std.mem.eql(u8, msg.event, "message")) {
            log.debug("WS received message: event={s}, topic={s}", .{ msg.event, msg.topic });
            if (msg.message) |message| {
                setClip(self.allocator, message) catch |err| {
                    log.err("Failed to set clipboard: {}", .{err});
                };
            }
        }
    }

    pub fn close(_: *Handler) void {
        log.debug("WS connection closed", .{});
    }
};

fn connectAndRun(allocator: std.mem.Allocator, config: Config) !void {
    const is_tls = std.mem.eql(u8, config.scheme, "wss");
    const port: u16 = if (is_tls) 443 else 80;

    var path_buf: [512]u8 = undefined;
    const path = std.fmt.bufPrint(&path_buf, "/{s}/ws", .{config.topic}) catch "/ws";

    var client = websocket.Client.init(allocator, .{
        .host = config.server,
        .port = port,
        .tls = is_tls,
    }) catch |err| {
        log.err("Failed to init client: {}", .{err});
        return err;
    };
    defer client.deinit();

    var headers_buf: [512]u8 = undefined;
    var headers_len: usize = 0;

    const host_header = std.fmt.bufPrint(headers_buf[headers_len..], "Host: {s}\r\n", .{config.server}) catch "";
    headers_len += host_header.len;

    if (config.token) |token| {
        const auth_header = std.fmt.bufPrint(headers_buf[headers_len..], "Authorization: Bearer {s}\r\n", .{token}) catch "";
        headers_len += auth_header.len;
    }

    client.handshake(path, .{
        .timeout_ms = 10000,
        .headers = if (headers_len > 0) headers_buf[0..headers_len] else null,
    }) catch |err| {
        log.err("Handshake failed: {}", .{err});
        return err;
    };

    log.info("Connected to {s} with topic={s} and timeout={d}", .{ config.server, config.topic, config.timeout });

    var handler = Handler{
        .allocator = allocator,
        .client = &client,
        .config = config,
        .last_traffic = std.time.timestamp(),
    };

    try client.readTimeout(@intCast(config.timeout * std.time.ms_per_s));

    while (true) {
        const now = std.time.timestamp();
        if (now - handler.last_traffic > @as(i64, @intCast(config.timeout))) {
            log.err("No traffic in the last {d} seconds", .{config.timeout});
            return error.Timeout;
        }

        const msg_result = client.read();
        if (msg_result) |maybe_msg| {
            if (maybe_msg) |msg| {
                handler.last_traffic = now;
                switch (msg.type) {
                    .text, .binary => {
                        handler.serverMessage(msg.data) catch {};
                    },
                    .ping => {
                        client.writePong(msg.data) catch {};
                    },
                    .pong => {},
                    .close => {
                        log.debug("WS received close message", .{});
                        return;
                    },
                }
            }
        } else |err| {
            log.err("Read error: {}", .{err});
            return err;
        }
    }
}

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const config = getConfig() catch {
        return;
    };

    while (true) {
        connectAndRun(allocator, config) catch |err| {
            log.err("Connection error: {}. Reconnecting...", .{err});
        };
        std.Thread.sleep(5 * std.time.ns_per_s);
    }
}
