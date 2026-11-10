# Hollow Crown join protocol

The launcher asks the Hollow Crown server for a token. It does not sign the token, and the signing key is not in this repository. The messenger mod reads the token file and sends those bytes plus a mod list. It has no key and no allow list.

Ordinary server join is unchanged. This protocol is what the event gate in HC-Platform can check. This repository does not implement that gate.

## Catalog

`GET https://launcher.thehollowcrownmc.com/api/v1/catalog`

```json
{
  "packs": [
    {
      "name": "Hollow Crown",
      "summary": "",
      "modrinth_project_id": "project id",
      "modrinth_version_id": "version id",
      "server_address": "the Paper address",
      "icon_url": null,
      "hosted_files": [
        {
          "sha512": "hex",
          "sha1": "hex",
          "url": "https://launcher.thehollowcrownmc.com/files/example.jar"
        }
      ]
    }
  ]
}
```

`server_address` is where Play connects. A `hosted_files` entry changes only the download URL for that hash. The Modrinth file stays public.

Override the URL with `HOLLOW_CROWN_CATALOG_URL`.

## Mod lists

`GET https://launcher.thehollowcrownmc.com/api/v1/mod-lists`

```json
{
  "whitelist": [{ "sha512": "hex", "sha1": "hex", "required": true }],
  "blacklist": [{ "sha512": "hex" }]
}
```

Before Play, the launcher hashes jars, zips, and litemods in `mods`, `resourcepacks`, and `shaderpacks`. A hash on the blacklist is refused even when it is also on the whitelist. Any other unknown archive is refused. A required whitelist hash must be present. Match by hash, not by file name.

The messenger jar is installed on every Play, so its hash has to be on the whitelist or Play stops.

Override the URL with `HOLLOW_CROWN_LISTS_URL`.

## Join token

`POST https://launcher.thehollowcrownmc.com/api/v1/join-token`

```json
{
  "account_uuid": "minecraft uuid",
  "username": "player",
  "minecraft_access_token": "short-lived Minecraft access token",
  "instance_id": "local instance id",
  "supersede_previous": true
}
```

The launcher sends the Minecraft access token so the server can check the account with Minecraft Services. It does not send the Microsoft password or the refresh token.

The response body is stored unchanged at `hollow-crown/join-token.json` in the instance directory. It must be JSON with `token_id`, `expiry` (RFC3339 or unix time), and `signature`. The launcher rejects a body that is missing a signature or is already expired. It does not create the signature.

`supersede_previous` means a newer open instance replaces older tokens for that account. Only the newest token stays valid. Closing the instance deletes the local file and tells the server to drop that id.

A token id is one-time. If it expires before the client connects, it is dead. Closing the instance also tells the server to drop that id:

`POST https://launcher.thehollowcrownmc.com/v1/token/close`

```json
{ "token_id": "id", "account_uuid": "minecraft uuid" }
```

Override the mint URL with `HOLLOW_CROWN_TOKEN_URL` and the close URL with `HOLLOW_CROWN_TOKEN_CLOSE_URL`.

`signature` is an Ed25519 signature made by the server. The launcher and the messenger do not hold the signing key and do not check the signature.

## Messenger

Channel: `hollowcrown:eventgate`

The payload is UTF-8 JSON:

```json
{
  "token_id": "id from the token file",
  "expiry": "expiry from the token file",
  "signature": "Ed25519 signature from the token file",
  "mods": [{ "path": "mods/example.jar", "sha1": "hex", "sha512": "hex", "size": 1 }]
}
```

`token_id`, `expiry`, and `signature` are copied from `hollow-crown/join-token.json`. The mod list is the jars, zips, and litemods under `mods`, `resourcepacks`, and `shaderpacks`.

The launcher writes `mods/hollow-crown-messenger.jar` into every instance before Play. The jar is the Fabric build from `messenger/fabric`. Fabric and Quilt load it. Forge, NeoForge, and vanilla do not, so Play stops on those loaders. The mod does not contain a key, the whitelist, the blacklist, or staff checks.
