# Local v3 patch

Source: SeaLoong/drcom4scut commit `ef20ae5c71744eb9e096f5e586713490ba01f4ee` (crate 0.3.2).

Changes from that commit:

- `src/settings.rs`: credentials resolve in CLI, environment, YAML order using `DRCOM_USERNAME` and `DRCOM_PASSWORD`.
- `src/main.rs`: removed the startup statement that logged the resolved password in plaintext.

The GUI uses the environment variables and does not pass credentials as process arguments. `build-core.ps1` at the project root documents and reproduces the Windows x64 build input and command.
