# Local v3 patch

Source: SeaLoong/drcom4scut commit `ef20ae5c71744eb9e096f5e586713490ba01f4ee` (crate 0.3.2).

Changes from that commit:

- `src/settings.rs`: credentials resolve in CLI, environment, YAML order using `DRCOM_USERNAME` and `DRCOM_PASSWORD`.
- `src/main.rs`: removed the startup statement that logged the resolved password in plaintext.
- `src/supervisor.rs`, `src/main.rs`, `src/eap/packet.rs`: when `DRCOM_PARENT_HANDLE` is present, a thread waits on that process and on `DRCOM_SHUTDOWN_EVENT`. Either signal sends one EAPOL-Logoff on a separate capture handle, bounded to one second, then exits. Direct CLI runs do not set the variables and keep the previous lifetime.

The GUI uses the environment variables and does not pass credentials as process arguments. `build-core.ps1` at the project root documents and reproduces the Windows x64 build input and command.
