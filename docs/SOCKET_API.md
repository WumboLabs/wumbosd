# wumbosd Socket API v1

The canonical public API is the session D-Bus service `org.wumbos.wumbosd` at
`/org/wumbos/wumbosd`, interface `org.wumbos.wumbosd1`. This socket is a thin,
local transport for Quickshell; it does not replace or extend that D-Bus API.

## Endpoint and listener ownership

The endpoint is `$XDG_RUNTIME_DIR/wumbos/wumbosd.sock`. `XDG_RUNTIME_DIR` is
required.

For standalone development, wumbosd creates `wumbos` with mode `0700`, binds
the socket with mode `0600`, refuses to replace a non-socket or symlink, and
removes only that exact socket during normal shutdown.

For normal user sessions, `wumbosd.socket` creates and owns the same endpoint
with directory mode `0700` and socket mode `0600`. It persists while
`wumbosd.service` is stopped and is removed only when the socket unit stops.
On activation the service validates `LISTEN_PID` and `LISTEN_FDS == 1`, adopts
fd 3 as a nonblocking Unix stream listener, and never unlinks the endpoint.

## Framing

Protocol version is `1`. Every frame is one UTF-8 JSON object terminated by a
single newline (`\n`). Frames are limited to 8192 bytes before the newline.
Malformed, unsupported, and oversized frames terminate only the affected
client connection.

On connection the daemon sends one frame:

```json
{"protocol":1,"type":"hello","api_version":1,"package_version":"0.1.0","status":"ready","uptime_ms":1234}
```

`package_version`, `status`, and `uptime_ms` are current Foundation values.

A compatible client sends:

```json
{"protocol":1,"type":"ping"}
```

The daemon replies:

```json
{"protocol":1,"type":"pong","uptime_ms":1234}
```

Clients may send this read-only bounded snapshot request after a compatible
hello/ping exchange:

```json
{"protocol":1,"type":"attention_recent_request","limit":32}
```

`limit` is required, accepts `0`, and is capped at the service EventStore
maximum of 128. The daemon returns one frame using the same newest-first event
ordering and JSON representation as Attention D-Bus `Recent`:

```json
{"protocol":1,"type":"attention_recent","events":[{"id":1,"created_at_ms":1700000000000,"source":"validation","kind":"message","title":"Attention event test","body":"Synthetic validation event","urgency":1}]}
```

The request is read-only: it neither persists, acknowledges, nor mutates
Attention events. It is an additive protocol 1 extension, not a general socket
command RPC system.

After a successful Attention API publication, every currently connected client
receives one additive live-delivery frame:

```json
{"protocol":1,"type":"attention_event","event":{"id":1,"created_at_ms":1700000000000,"source":"validation","kind":"message","title":"Attention event test","body":"Synthetic validation event","urgency":1}}
```

`urgency` is `0` for low, `1` for normal, and `2` for critical. The nested
event matches the D-Bus Attention event exactly. The server does not send event
history automatically on connection. A client that cannot keep up is
disconnected; delivery is not durable or retried.

Clients may remove one event or clear all events:

```json
{"protocol":1,"type":"attention_dismiss","id":1}
{"protocol":1,"type":"attention_clear"}
```

The daemon replies with `attention_dismiss_result` (`id`, `removed`) or
`attention_clear_result` (`removed`). Successful mutations are broadcast to
all connected clients as `attention_removed` (`id`) or `attention_cleared`.
Event IDs remain monotonic across dismiss and clear.

## Compatibility

Breaking changes require a new protocol version. Additive fields may remain in
protocol `1`; clients must ignore unknown additive fields. Clients must require
both `protocol == 1` and `api_version == 1` before interpreting Foundation
state.
