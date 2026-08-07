# Foundation v1 D-Bus API

`wumbosd` owns this session-bus identity while it is running:

- Bus name: `org.wumbos.wumbosd`
- Object path: `/org/wumbos/wumbosd`
- Interface: `org.wumbos.wumbosd1`
- API version: `1`

The interface is read-only and has these members:

| Member | Kind | D-Bus type | Result |
| --- | --- | --- | --- |
| `Ping` | method | `s` | Always `pong`. |
| `ApiVersion` | property | `u` | Always `1`. |
| `PackageVersion` | property | `s` | Cargo package version. |
| `Status` | property | `s` | Always `ready` after successful startup. |
| `UptimeMilliseconds` | property | `t` | Monotonic elapsed process time in milliseconds. |

Only one instance can own the bus name. A second instance exits with an error.

Breaking changes require a new versioned interface. Additive compatible members
may remain on `org.wumbos.wumbosd1`.
