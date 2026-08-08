# Attention Event API v1

## Purpose and durability

The Attention API is the daemon's small shared event primitive for future
attention routing. Events live only in wumbosd memory. They are not persisted,
acknowledged, filtered, searched, or replayed automatically after a socket
connection.

Freedesktop notification ingestion is one optional producer; see
[`NOTIFICATION_INGESTION.md`](NOTIFICATION_INGESTION.md). It normalizes
notification input before publication, while direct `Publish` retains the
validation below.

## Event fields and validation

Every event has `id`, `created_at_ms`, `source`, `kind`, `title`, `body`, and
`urgency`.

- `id` is an unsigned daemon-local monotonically increasing integer, beginning
  at `1` for a fresh daemon.
- `created_at_ms` is Unix epoch milliseconds assigned by the daemon at
  publication time.
- `source`, `kind`, and `title` must be non-empty UTF-8 strings.
- `body` may be empty.
- Maximum UTF-8 byte lengths are: `source` 64, `kind` 64, `title` 256, and
  `body` 4096. Oversized input is rejected without truncation.
- `urgency` is an unsigned byte: `0` = `low`, `1` = `normal`, `2` = `critical`.
  Other values are rejected.

## D-Bus

The same session service owns:

- Object: `/org/wumbos/wumbosd/Attention`
- Interface: `org.wumbos.wumbosd.Attention1`

The event structure signature is `(ttssssy)`, in field order `id`,
`created_at_ms`, `source`, `kind`, `title`, `body`, `urgency`.

| Member | D-Bus signature | Behavior |
| --- | --- | --- |
| `Publish(source, kind, title, body, urgency)` | `ssssy → t` | Validates, stores, emits `EventAdded`, and returns the new id. Invalid input returns `org.freedesktop.DBus.Error.InvalidArgs`. |
| `Recent(limit)` | `u → a(ttssssy)` | Returns at most 128 events, newest first. `0` returns an empty array; larger limits are capped at 128. |
| `Dismiss(id)` | `t → b` | Removes the matching event, emits `EventRemoved`, and returns whether it existed. |
| `Clear()` | `→ u` | Removes all events, emits `EventCleared` when non-empty, and returns the number removed. |
| `EventAdded(event)` | `(ttssssy)` | Emitted once after successful insertion. |
| `EventRemoved(id)` | `t` | Emitted once after successful dismissal. |
| `EventCleared` | `()` | Emitted once after a non-empty clear. |

## Socket live delivery

Socket protocol version remains `1`. After a successful publication, each
currently connected compatible client receives one newline-framed JSON object:

```json
{"protocol":1,"type":"attention_event","event":{"id":1,"created_at_ms":1700000000000,"source":"validation","kind":"message","title":"Attention event test","body":"Synthetic validation event","urgency":1}}
```

The nested event field values match the D-Bus event. This is live-only fanout;
no event history is sent on connection. A slow or lagging client is disconnected
rather than delaying publication or other clients.
