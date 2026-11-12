# Hollow Crown launcher API files

Static JSON served from the launcher site after deploy.

| Path | File to edit |
| --- | --- |
| `GET /api/v1/catalog` | [`catalog.json`](./catalog.json) |
| `GET /api/v1/mod-lists` | [`mod-lists.json`](./mod-lists.json) |

## Catalog

Edit `catalog.json` and put real Modrinth project and version ids in each pack. See `catalog.example.json` for the exact fields. Prefer unlisted Modrinth packs when the pack should not appear in public search.

Play always uses `play.thehollowcrownmc.com`.

## Mod lists

Edit `mod-lists.json`. Match by hash, never by file name. Blacklist wins when a hash is on both lists.

Add the messenger jar (`mods/hollow-crown-messenger.jar`) hashes to the whitelist later. See `mod-lists.example.json`.
