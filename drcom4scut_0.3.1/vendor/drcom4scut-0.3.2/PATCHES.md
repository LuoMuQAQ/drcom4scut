# Local v3 patch

Source: SeaLoong/drcom4scut commit `ef20ae5c71744eb9e096f5e586713490ba01f4ee` (crate 0.3.2).

Changes from that commit:

- `src/settings.rs`: credentials resolve in CLI, environment, YAML order using `DRCOM_USERNAME` and `DRCOM_PASSWORD`.
- `src/main.rs`: removed the startup statement that logged the resolved password in plaintext.
- `src/supervisor.rs`, `src/main.rs`, `src/eap/packet.rs`: when `DRCOM_PARENT_HANDLE` is present, a thread waits on that process and on `DRCOM_SHUTDOWN_EVENT`. Either signal sends one EAPOL-Logoff on a separate capture handle, bounded to one second, then exits. Direct CLI runs do not set the variables and keep the previous lifetime.

The GUI uses the environment variables and does not pass credentials as process arguments. `build-core.ps1` at the project root documents and reproduces the Windows x64 build input and command.

## 2026-09-25: UDP generation ownership and authentication invalidation

- Each UDP generation receives a dedicated EAP inbox. Subscribe/replay and publication share one mutex so a retired receiver cannot steal SUCCESS from a replacement.
- Cache the latest EAP state, including STOP/SLEEP/QUIT. Ordinary EAP failure and worker exit/panic invalidate cached SUCCESS.
- The UDP owner consumes EAP events directly. Login clears old checksum state, checks replay immediately, observes cancellation with bounded waits, and uses a monotonic 90-second deadline.
- Drop cancels and wakes parked workers; an idle sender checks cancellation every 200 ms. A blocked socket read retains its existing 30-second timeout.
- Offline regressions cover repeated generation replacement, concurrent subscribe/publication/invalidation, cached authentication, STOP/SLEEP/QUIT, checksum reuse, and idle-worker shutdown. Packet formats are unchanged.
- Windows error 10022 remains an unproven network/socket cause. This addresses the demonstrated recovery race, not every source of disconnects.

The root `build-core.ps1` now defaults to the native GUI resource, checks the built binary before replacing it, and retains the SDK/build cache. Pass `-Destination` for another target. Historical .NET embedded binaries are not replaced by default.
