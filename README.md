# Hollow Crown Launcher

Windows launcher for The Hollow Crown. Players sign in with Microsoft, install packs from the Hollow Crown catalog, and play on his Paper network.

This is not an official Minecraft product. It is not approved by or associated with Mojang or Microsoft. It is not made by Modrinth, and Modrinth is not responsible for it.

The app is a fork of the [Modrinth App](https://github.com/modrinth/code) (Theseus), snapshot `e8a4e0223adc19114fe830c38810b64862953dcb`. What changed is listed in [CHANGES.md](CHANGES.md). Source is GPL-3.0-only. See [LICENSE](LICENSE) and [COPYING.md](COPYING.md). Modrinth's name, logo, and Microsoft client id are not used.

v1 is Windows only. The download page is [https://launcher.thehollowcrownmc.com](https://launcher.thehollowcrownmc.com).

## What Play does

- Install and repair stay on the Modrinth pack flow. There is no mod browser.
- The catalog chooses the pack and the server address.
- The launcher hashes the instance and compares it with the whitelist and blacklist. A hash on both lists follows the blacklist.
- The launcher asks the server for a join token tied to the Minecraft account. The token lasts only while that instance is open, and a newer instance replaces the older token. Closing the instance calls `POST /v1/token/close`. The Ed25519 signing key is not in this repository.
- Every instance gets the messenger mod. It reads the token file and sends the token id, expiry, signature, and mod list on `hollowcrown:eventgate`.

The request shapes are in [docs/join-protocol.md](docs/join-protocol.md).

## Build

Copy `packages/app-lib/.env.prod` to `packages/app-lib/.env`, then from the repository root:

```sh
pnpm install
pnpm app:dev
```

Set `HOLLOW_CROWN_AZURE_CLIENT_ID` to the Hollow Crown Launcher Azure application id before Microsoft sign-in. Until Mojang allowlists that app, login fails with `Invalid app registration`. The app never asks for a Microsoft password, and it does not ship Minecraft.

`HOLLOW_CROWN_ALLOW_NON_WINDOWS=1` lets a developer start the game code on a non-Windows machine. The shipped bundle is the Windows installer only.
