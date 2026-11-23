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

## Windows GUI v0.3.9: event-driven idle wait

- The EAP and UDP process loops wait for readiness rather than yielding and sleeping repeatedly. Existing packet and control handling order, retry counters, and log messages are unchanged. A 10 ms fallback still checks unexpected worker exits.
- Included in the embedded Windows v0.3.9 core. The core version remains 0.3.2; see the pinned binary hash in `build-core.ps1` and the GUI build documentation. Real-network reconnection, resume and CPU measurements remain unverified.

## Windows GUI v0.3.9 package refresh: EAP receive filtering

- Check the outer Ethernet type on the borrowed receive buffer; only `0x888e` frames are copied and queued to EAP. This matches the previous accept gate and does not add VLAN support. The destination address and full EAP checks remain downstream.
- A filtered frame is a successful read. Reset the consecutive receive-error counter and return to the worker lifecycle checks after every frame. Driver reads, retry behavior, all UDP logic and the independent EAPOL-Logoff sender are unchanged.
- Includes production tests with a fake data-link receiver for one read per iteration, short and VLAN frames, raw EAPOL preservation and timeout propagation. The public package does not include diagnostic control switches or BPF.
