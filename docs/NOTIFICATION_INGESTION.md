# Notification ingestion v1

## Purpose and ownership

wumbosd can ingest freedesktop desktop notifications as Attention events through
`org.freedesktop.Notifications` at `/org/freedesktop/Notifications`. This is a
proper notification server, not passive D-Bus monitoring.

Ownership is disabled by default. Set `WUMBOSD_NOTIFICATION_SERVER=1` only for
isolated validation; then wumbosd requests the notification name without
queueing or replacing an existing owner. Normal production mode keeps the
existing desktop notification server untouched until wumbOS has visible
notification/Attention UI.

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
