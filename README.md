# wumbosd

`wumbosd` is the native Rust user-session service for wumbOS: a single
event-driven session D-Bus service with health, status, version, and uptime
reporting; an Attention Event API (bounded in-memory publication, recent
history, and dismissal controls) whose events are also forwarded live over a
local Unix socket to Quickshell; and a freedesktop notification server that
ingests desktop notifications into the same Attention pipeline, including
notification actions. The socket is only the local transport.

Alpha software, MPL-2.0 licensed, developed against a Hyprland/Wayland
systemd user session.

## Documentation

- [docs/INSTALL.md](docs/INSTALL.md) — requirements, binary and source
  installs, uninstall.
- [docs/PORTABLE_DISTRIBUTION.md](docs/PORTABLE_DISTRIBUTION.md) — the
  versioned binary release contract: signed-manifest authenticity, pinned
  trust, install/update/rollback with `wumbosdctl`.
- [docs/RELEASE_PROCESS.md](docs/RELEASE_PROCESS.md) — maintainer process
  for signed tags, release artifacts, and publication.
- [docs/SUPPORT.md](docs/SUPPORT.md) — supported boundaries and how to
  report problems.
- API contracts: [docs/DBUS_API.md](docs/DBUS_API.md),
  [docs/ATTENTION_API.md](docs/ATTENTION_API.md),
  [docs/SOCKET_API.md](docs/SOCKET_API.md),
  [docs/NOTIFICATION_INGESTION.md](docs/NOTIFICATION_INGESTION.md).

## Development

Run it from a graphical user session with a session D-Bus:

```sh
cargo run
```

Without systemd socket activation, wumbosd creates
`$XDG_RUNTIME_DIR/wumbos` with mode `0700`, binds
`$XDG_RUNTIME_DIR/wumbos/wumbosd.sock` with mode `0600`, and removes only
that socket on a clean exit.

## Normal user-session mode

Install `systemd/wumbosd.socket` and `systemd/wumbosd.service` as user units
([docs/INSTALL.md](docs/INSTALL.md)). The socket unit owns the persistent
endpoint and activates one foreground `wumbosd` service on connection. The
service validates `LISTEN_PID` and `LISTEN_FDS`, adopts exactly fd 3 as its
nonblocking Unix stream listener, and never unlinks the systemd-owned socket
on exit. Stopping the socket unit removes the endpoint; stopping only the
service leaves it available for a later activation.

The process owns `org.wumbos.wumbosd` and exits cleanly on Ctrl-C or
SIGTERM. Only one instance can run in a session. Production notification
ownership is enabled by the `notifications.conf` drop-in and the documented
D-Bus activation override; see
[docs/NOTIFICATION_INGESTION.md](docs/NOTIFICATION_INGESTION.md).

## License

MPL-2.0. See [LICENSE](LICENSE).
