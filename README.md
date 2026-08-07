# wumbosd

`wumbosd` is the native Rust user-session service for wumbOS.

Foundation v1 provides one event-driven session D-Bus service with health,
status, version, and uptime reporting. It has no desktop-shell integration or
higher-level capabilities.

## Development

Run it from a graphical user session with a session D-Bus:

```sh
cargo run
```

The process owns `org.wumbos.wumbosd` and exits cleanly on Ctrl-C or SIGTERM.
Only one instance can run in a session. The public D-Bus contract is documented
in [`docs/DBUS_API.md`](docs/DBUS_API.md).
