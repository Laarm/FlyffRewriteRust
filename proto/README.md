# proto/

The wire protocol of Flyff V15 (Plan/11). `flyff/options.proto` (package `flyff`) defines the custom options,
`flyff/v1/*.proto` (package `flyff.v1`) the messages. `legacy/` holds the coverage-gate inputs: `legacy_index.csv`
(1081 rows, generated from annex 11a by `proto-coverage --import-11a`) and `ui_send_sites.csv` (308 client send
functions, generated from ref/99 appendix B by `--import-ui-sites`); `cargo xtask coverage proto` checks the schema
against them (tools/proto-coverage, ADR-024).

* Generate with `cargo xtask gen-proto` (needs `buf`, and `pnpm --dir web install` for `protoc-gen-es`); check with
  `cargo xtask gen-proto --check`. Outputs: `crates/flyff-proto/src/gen/*` and `web/src/gen/proto/*`, both committed.
  Never edit them, never run `buf` or `protoc` by hand, there is no build script.
* Rules (Plan/11 §5): no `map<>`, fields in ascending number order, legacy fields numbered 1..N in legacy order,
  added fields from 100, `(flyff.js_safe_number) = true` on every 64-bit field (except the five bit-packed ones, the
  allowlist `BIT_PACKED` of the coverage gate: the three of Plan/11 section 4.3, `ItemElem.random_opt_item_id`,
  `ItemSelectAwakeningValueNtf.new_random_option` and `CallTheRoll.attendance_bits`, plus `QueryEquipEntry.random_opt_item_id`
  and `UpdateItemEx.value`, which carry the same packed random-option word with bits 62 and 63),
  `(flyff.max_len)` on client-to-server strings/bytes/repeated, no `jstype`.
* **The second package** `flyff.admin.v1` (`proto/flyff/admin/v1/admin.proto`, Plan/25 section 3.16, WP-ADMIN-05) is the admin console
  protocol of `/admin/ws`: no envelope, no legacy ids, no `(flyff.dir)`; one `AdminClientMsg` or `AdminServerMsg` per WebSocket
  frame. `gen-proto` generates `crates/flyff-proto/src/gen/flyff.admin.v1.rs` (`flyff_proto::admin_pb`, codec in
  `flyff_proto::admin`) and `web/src/gen/proto/flyff/admin/v1/admin_pb.ts` (short path `web/src/gen/proto/admin/index.ts`). It is
  part of `PROTO_HASH` and of the descriptor set, but none of the wire tables covers it: `coverage proto` and exit-check read
  `flyff.v1` / `proto/flyff/v1` only, and `legacy_ids`, `dispatch`, `limits` and the typed registry list wire messages only. Its
  64-bit fields carry `(flyff.js_safe_number)` like the wire package's. Its enum values have no enum-name prefix (the plan fixes
  `PLAYER`, `FORBIDDEN`, `LOGOUT`, ...), so `buf.yaml` ignores `ENUM_VALUE_PREFIX` for that one file.
* Any byte change of a `.proto` file changes `PROTO_HASH` (client and server always ship together).
* Transcribed so far: `common`, `item_types`, `mover_types`, `social_types` (Plan/11 section 7), `session`, `lobby`,
  `world`, `movement`, `chat`, `notice` and, for the fight-and-level slice (task proto-m3, WP-PROTO-11 rest, 12, 13,
  14), the whole of `character` (35 messages), `taskbar` (6), `combat` (25, with `ProjectileKind`), `skill` (20),
  `pvp` (18, the wanted board stays dropped and its tags reserved) and `inventory` (34); for the economy, NPC, pet and
  quest slice (task proto-m4, WP-PROTO-15 .. 19) `upgrade` (28), `shop` (14), `trade` (38), `bank` (18), `mail` (10),
  `pet` (28), `npc` (8, with the `DialogOp` helper messages) and `quest` (10); for the social and event slice (task
  proto-rest, WP-PROTO-21 .. 27, 29, 30) `party` (34), `guild` (50), `guild_war` (13), `guild_siege` (25),
  `messenger` (40), `relation` (23), `event` (33), `lord` (20), `secret_room` (14), `housing` (19) and `gm` (6). That
  completes the envelope: 289 C2S + 347 S2C messages, no stub. `envelope.proto` holds the BEGIN/END fence of every
  feature file (Plan/11 section 4.2) and its entries (a tag is never reused). Every envelope message that is not a
  stub needs a golden vector (`crates/flyff-proto/tests/golden/README.md`).
* `int_` / `class_` (target-language keywords) keep a trailing underscore per Plan/11 section 5.2 and carry
  `// buf:lint:ignore FIELD_LOWER_SNAKE_CASE`.
* A message that is not an envelope entry but only a part of one (`SmModeEntry`, `TaskBarApplet`, `QueryEquipEntry`,
  ...) is a helper message without `(flyff.dir)`/`legacy_id` options; the multiplexed legacy messages (`PkRelation`,
  `DpsUpdate`) are one message with an inner `oneof` numbered legacy sub value + 1.
