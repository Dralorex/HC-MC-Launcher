# Changes from Modrinth App

Upstream snapshot: `modrinth/code` commit `e8a4e0223adc19114fe830c38810b64862953dcb` (10 Nov 2026).

This repository keeps the Modrinth App install and repair engine (`apps/app`, `apps/app-frontend`, `packages/app-lib`, and the libraries those crates need). Labrinth and the Modrinth website are not included. Their copyright notices stay in the copied files. The Modrinth name, logo, and Microsoft client id are not reused.

Hollow Crown changes:

- Windows is the only shipped bundle target (`nsis`). Mac and Linux builds are not part of v1.
- The visible name is Hollow Crown Launcher. Colors are darker bronze, dark red, gray, and dark gray.
- The mod browser, skins page, and Modrinth hosting pages are not offered. Packs come from the Hollow Crown catalog. Install and repair still use the upstream Modrinth pack flow.
- Play connects to the Paper address in the catalog. Singleplayer and other servers are not offered.
- A file flagged "host this file ourselves" is downloaded from the catalog URL instead of `cdn.modrinth.com`.
- Before Play, instance archives are checked against the server whitelist and blacklist. A hash on both lists follows the blacklist.
- Each Play asks the server for one account-bound token, writes the server's bytes, and removes them when the instance closes. Closing also calls `POST /v1/token/close`. A newer instance drops the older token. Token ids are one-time. The Ed25519 signing key is not in this repository.
- The messenger in `messenger/fabric` is built here and installed into every instance as `mods/hollow-crown-messenger.jar`. It reads the token file and sends token id, expiry, signature, and the mod list on `hollowcrown:eventgate`. Fabric and Quilt load that jar.
- Microsoft login uses the Hollow Crown Azure app id from `HOLLOW_CROWN_AZURE_CLIENT_ID`. The app does not ask for a password.
- The download and the app say this is not an official Minecraft product. Minecraft itself is not in this repository.

The updater endpoint is `https://launcher.thehollowcrownmc.com/updates.json`. The updater public key in `apps/app/tauri-release.conf.json` is still the upstream public key and has to be replaced before Hollow Crown publishes updates. Windows code signing is not configured.
