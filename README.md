# wumbosd

`wumbosd` is the native Rust user-session service for wumbOS.

Foundation v1 provides one event-driven session D-Bus service with health,
status, version, and uptime reporting. Attention Event v1 adds a bounded,
in-memory publication and recent-event API on that same service; the Unix socket
also forwards live Attention events to Quickshell. The socket is only the local
transport.

## Development

Run it from a graphical user session with a session D-Bus:

```sh
cargo run
```

Without systemd socket activation, wumbosd creates
`$XDG_RUNTIME_DIR/wumbos` with mode `0700`, binds
`$XDG_RUNTIME_DIR/wumbos/wumbosd.sock` with mode `0600`, and removes only that
socket on a clean exit.

## Normal user-session mode

Install `systemd/wumbosd.socket` and `systemd/wumbosd.service` as user units.
The socket unit owns the persistent endpoint and activates one foreground
`wumbosd` service on connection. The service validates `LISTEN_PID` and
`LISTEN_FDS`, adopts exactly fd 3 as its nonblocking Unix stream listener, and
never unlinks the systemd-owned socket on exit. Stopping the socket unit removes
the endpoint; stopping only the service leaves it available for a later
activation.

The process owns `org.wumbos.wumbosd` and exits cleanly on Ctrl-C or SIGTERM.
Only one instance can run in a session. The Foundation contract is documented in
[`docs/DBUS_API.md`](docs/DBUS_API.md), Attention Event v1 in
[`docs/ATTENTION_API.md`](docs/ATTENTION_API.md), the optional notification
ingestion adapter in [`docs/NOTIFICATION_INGESTION.md`](docs/NOTIFICATION_INGESTION.md),
and the Quickshell transport in [`docs/SOCKET_API.md`](docs/SOCKET_API.md).
