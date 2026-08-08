# Notification ingestion v1

## Production ownership and deployment

wumbOS production sessions route freedesktop desktop notifications through
`org.freedesktop.Notifications` at `/org/freedesktop/Notifications` to
wumbosd, then through shared `AttentionState`, its existing socket
`attention_event`, and the Quickshell Attention UI. This is a proper
notification server, not passive D-Bus monitoring.

The base daemon remains testable with notification ownership disabled. Production
ownership is enabled only by
`systemd/wumbosd.service.d/notifications.conf`, which sets
`WUMBOSD_NOTIFICATION_SERVER=1` and makes the existing user service a D-Bus
service for `org.freedesktop.Notifications`. Install the companion
`dbus-1/services/fr.emersion.mako.service` in
`$XDG_DATA_HOME/dbus-1/services/` (normally `~/.local/share/dbus-1/services/`).
It is a user-local override of Fedora's packaged Mako activation file, preserves
the packaged file, and has D-Bus execute the supported user-systemd start command
for `wumbosd.service`.

Install the drop-in under
`~/.config/systemd/user/wumbosd.service.d/notifications.conf`, run
`systemctl --user daemon-reload`, then reload only the running bus configuration:

```sh
busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig
```

Never restart, stop, replace, or reset the live user D-Bus broker to apply this
activation entry: session integrity for unrelated desktop applications and the
Secret Service is a hard requirement. If `ReloadConfig` is unavailable or fails,
defer activation-file changes to the next fresh user session; do not disrupt the
current bus. Keep `wumbosd.socket` enabled. The D-Bus activation entry starts
`wumbosd.service` even before Quickshell has connected to the socket; socket
activation remains available for the existing local transport. To roll back,
remove both user-local files, reload systemd and the safe bus configuration,
restart only `wumbosd.service` so a socket client cannot retain the old
notification-enabled environment, then start Mako:

```sh
rm -f ~/.config/systemd/user/wumbosd.service.d/notifications.conf
rm -f ~/.local/share/dbus-1/services/fr.emersion.mako.service
systemctl --user daemon-reload
busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig
systemctl --user restart wumbosd.service
mako
```

If either target existed before installation, restore it from the takeover
backup rather than removing it. Mako has no v1 role while this activation
override is installed; it remains packaged and can be restored immediately by
the rollback above.

## Interface

The server implements `GetCapabilities()`,
`Notify(susssasa{sv}i) -> u`, `CloseNotification(u)`,
`GetServerInformation() -> (ssss)`, and `NotificationClosed(uu)`.

It identifies as `wumbOS wumbosd` / `wumbOS`, uses `CARGO_PKG_VERSION`, and
reports notification specification version `1.3`. Its only capability is
`body`. Actions, icons/images, markup, sound, action invocation, and automatic
expiration are accepted or ignored as appropriate but are not implemented.

## IDs and lifecycle

Notification IDs are nonzero daemon-local `u32` values distinct from immutable
Attention event `u64` IDs. A replacement returns its requested notification ID
and publishes a new Attention event; old Attention history is never mutated.
`CloseNotification` removes a known active ID and emits
`NotificationClosed(id, 3)`. Unknown IDs return a D-Bus error. State is only
in-memory active-ID tracking; there is no durable notification history or
expiry timer.

## Attention mapping

Every successful `Notify` publishes once through the shared `AttentionState`:

- source: non-empty `desktop-entry` hint, else non-empty `app_name`, else
  `notifications`;
- kind: `notification`;
- title: summary, else app name, else `Notification`;
- body: notification body as uninterpreted UTF-8 text;
- urgency: hint `0` low, `1` normal, `2` critical; absent or unusable is normal.

Before publication this adapter UTF-8-safely truncates source, title, and body
to the existing Attention byte limits (64, 256, 4096). Direct Attention
`Publish` remains reject-on-oversize. The resulting ordinary Attention event
uses the existing D-Bus `EventAdded`, `Recent`, and socket `attention_event`
paths.
