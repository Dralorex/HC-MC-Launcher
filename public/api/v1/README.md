# Hollow Crown launcher API files

Static JSON served from the launcher site after deploy.

| Path | File to edit |
| --- | --- |
| `GET /api/v1/catalog` | [`catalog.json`](./catalog.json) |
| `GET /api/v1/mod-lists` | [`mod-lists.json`](./mod-lists.json) |

## Catalog

Edit `catalog.json`. A pack installs from `mrpack_url` or from Modrinth ids.

Leave `mrpack_url` null to keep using `modrinth_project_id` and `modrinth_version_id`, as Iron Legacy does until an `.mrpack` is uploaded. Prefer an unlisted Modrinth pack when the pack should not appear in public search.

Set `mrpack_url` to install and repair from a file on the launcher host. That path does not call Modrinth versions. Use a full URL or a `/files/...` path:

```json
{
  "name": "Example self-hosted pack",
  "summary": "Installed from the Hollow Crown host.",
  "mrpack_url": "/files/example.mrpack",
  "server_address": "play.thehollowcrownmc.com",
  "icon_url": null,
  "hosted_files": []
}
```

`https://launcher.thehollowcrownmc.com/files/example.mrpack` is the same file. `hosted_files` still only remaps one hashed file after a Modrinth version exists. It does not replace `mrpack_url`.

Play always uses `play.thehollowcrownmc.com`.

## Mod lists

Edit `mod-lists.json`. Match by hash, never by file name. Blacklist wins when a hash is on both lists.

Add the messenger jar (`mods/hollow-crown-messenger.jar`) hashes to the whitelist later. See `mod-lists.example.json`.
