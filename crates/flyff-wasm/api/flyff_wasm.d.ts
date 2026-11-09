/* tslint:disable */
/* eslint-disable */

/**
 * The rules of the browser (Plan/16 §18.4).
 *
 * **Contract of the replica cache** (a deviation from D7's "pure functions taking plain data"). Besides the data
 * decoded once from the bundles (rule tables, item and skill props) the context holds one DST replica per mover the
 * client knows. The replicas are a *cache of server-sent state*, not game state: TypeScript owns the lifecycle of
 * the entities. It passes a mover's spawn snapshot (`MoverCommon.dst_params`) when the mover spawns, the
 * `SetDestParam` / `ResetDestParam` / `ResetDestParamSync` messages as ops, and the id in `forgets` when the mover
 * despawns; it calls `dst_reset_all` on a session re-attach and on a world change, before the new spawns arrive.
 * A snapshot is *loaded* (assigned), never applied as a delta, so a root counter above 1 survives; ops are the legacy
 * deltas. No rule reads anything TypeScript did not feed, and every export is a pure function of (replica,
 * arguments); an object without a replica is evaluated on the default (all zero) table. Per network drain exactly
 * one `drain_sync` call; the per-object exports are for tools and the fallback, `dst_view` is debug-only.
 * Arrays that do not parse throw and change nothing.
 */
export class RulesContext {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * The exact chance of each angel colour in percent, `[red, blue, green, white]`.
     */
    angel_odds(pieces: number): Float32Array;
    /**
     * The colour shares `[red, blue, green, white]` in percent for `pieces` angel materials.
     */
    angel_shares(pieces: number): Float32Array;
    /**
     * The armour +N set level of 31 attribute options.
     */
    armor_set_level(equip_options: Int32Array): number;
    /**
     * `GetAttackSpeed()` for the given job, level, dexterity and weapon item.
     */
    attack_speed(obj: number, job: number, level: number, dex: number, weapon_item: number): number;
    /**
     * `[unit, tax]` of one piece in a penya shop; `input` is `{ cost | itemId + abilityOption, shopRate,
     * eventFactor, perin, taxRate }`.
     */
    buy_unit_price(input: any): Float64Array;
    /**
     * `true` when `from` can be swapped for `to` with a transy scroll.
     */
    can_transy(from: number, to: number): boolean;
    /**
     * `GetCastingAniSpeed()`.
     */
    casting_ani_speed(obj: number): number;
    /**
     * The Red Chip price of an item, `-1` for none.
     */
    chip_cost(item_id: number): number;
    /**
     * M5 (WP-MOVER-23): may the own character attack the clicked object? `selfView`, `targetView` and `ctx` are
     * the camelCase objects of the module notes (every field optional); the answer is `0` no, `1` yes, `2` a PK
     * target. A malformed view throws and answers nothing.
     */
    client_attackable(self_view: any, target_view: any, ctx: any): number;
    /**
     * Every number of the character window: `input` is `{ isPlayer, level, job, base: [str, sta, dex, int], equip:
     * [{ part, itemId, abilityOption, expired }], weaponPart }`, the result `DerivedStats` in camelCase.
     */
    derived_stats(obj: number, input: any): any;
    /**
     * **The** boundary call of a network drain (Plan/40 §7.10): `snapshots` `[obj, n, (dst, adj, chg) x n]*`
     * applied first, `ops` `[obj, kind, dst, a, b]*` second, `forgets` `[obj]*` third, then the `param_rows`
     * `[obj, job (-1 NPC), level, base_dex, weapon_item (0 = unarmed / NPC)]*` are evaluated; 7 `f32` per row
     * `[speed_adj_pct, speed_chg_set, chr_state, hawkeye_rate, jumping, attack_speed, casting_ani_speed]`.
     */
    drain_sync(snapshots: Int32Array, ops: Int32Array, forgets: Uint32Array, param_rows: Int32Array): Float32Array;
    /**
     * Replica cache: `[obj, kind (0 set | 1 reset | 2 sync), dst, a, b]*`; returns the objects whose replica
     * changed, ascending.
     */
    dst_apply_ops(ops: Int32Array): Uint32Array;
    /**
     * Replica cache: the movers despawned.
     */
    dst_forget(objs: Uint32Array): void;
    /**
     * Replica cache: session re-attach and world change.
     */
    dst_reset_all(): void;
    /**
     * Replica cache: `objs` and, per object, `[n, (dst, adj, chg) x n]` (`MoverCommon.dst_params`).
     */
    dst_snapshot(objs: Uint32Array, triples: Int32Array): void;
    /**
     * Debug only, never on the drain path: `adj[95]` then `chg[95]` of the replica, `undefined` without one.
     */
    dst_view(obj: number): Int32Array | undefined;
    /**
     * The card item that upgrades an element (1 fire, 2 water, 3 electricity, 4 wind, 5 earth); 0 for none.
     */
    element_card(element: number): number;
    /**
     * `0` when the item may be worn, else the `TID_*` (`0xFFFFFFFF`: refused without a text).
     */
    equip_check(item_id: number, random_opt_hi: number, sex: number, job: number, level: number, rweapon_item: number): number;
    /**
     * `(int)(exp * 10000 / next_exp(level))`; `exp` is below 2^53.
     */
    exp_percent_x100(level: number, exp: number): number;
    /**
     * M5 (WP-MOVER-19): the jobs `UpdateJob` may name for a character of `level` and `job` (the strict table of
     * Plan/16 §11.4; empty when no fast job change is possible).
     */
    fast_job_targets(level: number, job: number): Uint32Array;
    /**
     * M5 (WP-MOVER-19): the client rule that opens the fast job change window (level 15 vagrant, level 60 expert,
     * level 120 pro or master with `exp_percent_x100 == 9999`).
     */
    fast_job_window(level: number, job: number, exp_percent_x100: number): boolean;
    /**
     * The price of a beauty-shop hair change; `input` is `{ curMesh, curColor (ARGB), rgb: [r, g, b], hair }`.
     */
    hair_cost(input: any): number;
    /**
     * The ability line `[min, max]` of an item at an upgrade level (`[0, 0]` for an unknown item).
     */
    item_min_max(item_id: number, ability_option: number, expired: boolean): Int32Array;
    /**
     * `GetItemMultiplier`.
     */
    item_multiplier(item_id: number, ability_option: number, expired: boolean): number;
    /**
     * Every number of one item tooltip in one call (Plan/42 §9.6): `input` is `{ itemId, abilityOption,
     * expired, element, elementLevel, randomOptionHi, randomOptionLo, eggLevelC, piercing: number[],
     * ultimateGems: number[], owner: { sex, job, level, rweaponItem }, worn: [{ part, itemId, expired }],
     * shop: { shopRate, buyFactor, sellFactor, purchaseTaxRate, salesTaxRate } }` (everything optional but
     * `itemId`), the result `ItemTooltipNumbers` in camelCase with `null` for "none".
     */
    item_tooltip_numbers(input: any): any;
    /**
     * The fee for taking an item out of a mail (Plan/19 §11.4).
     */
    mail_storage_fee(cost: number, elapsed_s: number): number;
    /**
     * `GetSpeed` with the replica of `obj`.
     */
    move_speed(obj: number, base: number, speed_factor: number): number;
    /**
     * The sim inputs of many movers in one call: `inputs` is `[job (-1 NPC), level, base_dex, weapon item (0 =
     * unarmed / NPC)]` per object, the result 8 words per object `[speed_adj_pct, speed_chg_set (0/1), jumping,
     * chr_state, hawkeye_rate, attack_speed (f32 bits), casting_ani_speed (f32 bits), 0]`; read slots 5-6 through
     * a `Float32Array` view of the same buffer.
     */
    mover_params(objs: Uint32Array, inputs: Int32Array): Int32Array;
    /**
     * `new RulesContext(tables, items, itemExt, skills, cfg)`: once after the bundles loaded (the four JSON files
     * as bytes, each the envelope `{bundle, schema, data}`). `cfg` is an object with the optional `RulesConfig`
     * switches in camelCase (`bowdmgPropjob`, `wandAutoattacks`, `betterArmorBonus`, `autoSkillMax`,
     * `fixGetHpMpFpPoint`, `removeMaxRecoverRate`, `healOverTime`, `setMaxSpeed`, `setMaxCast`, `setMaxCrit`,
     * `maxBlockValue`, `maxBlockValueGw`); `undefined` means the repack defaults.
     * `onlyArmorAwakes` (default `true`) decides which items carry a random-option kind in the tooltip numbers.
     */
    constructor(tables: Uint8Array, items: Uint8Array, item_ext: Uint8Array, skills: Uint8Array, cfg: any);
    /**
     * `IsPierceAble(card_kind3, size)` of an item (`cardKind3 = 0xFFFFFFFF` for "any", `noPiercing` is
     * `features.no_piercing`).
     */
    pierce_able(item_id: number, pierced_size: number, card_kind3: number, size: boolean, vis_transformed: boolean, no_piercing: boolean): boolean;
    /**
     * `[unit, tax]` of one piece of a quick sale; `input` is `{ cost | itemId + abilityOption, taxRate,
     * binds }`.
     */
    quick_sell_unit_price(input: any): Float64Array;
    /**
     * `[unit, tax]` of one piece sold to an NPC; `input` is `{ cost | itemId + abilityOption, eventFactor,
     * taxRate }`.
     */
    sell_unit_price(input: any): Float64Array;
    /**
     * M4 (WP-MOVER-24, VR-086): `cur` and `plan` are 45 slots as 90 words `[skill_id, level]`; `>= 0` the cost of
     * the plan, `< 0` the negated error code plus one (see `RulesCore::skill_plan_check`); throws for a wrong length.
     */
    skill_plan_check(cur: Uint32Array, plan: Uint32Array, level: number, job: number, sp: number): number;
    /**
     * M4 (WP-MOVER-24): the points one level of the skill costs; throws for an unknown skill.
     */
    skill_point_cost(skill_id: number): number;
    /**
     * `derived_stats` with `add = [str, sta, dex, int]` pending stat points.
     */
    stat_preview(obj: number, input: any, add: Int32Array): any;
    /**
     * `__STATISTICS` lines for the replica of `obj`; `input` is the `derived_stats` input (`undefined` = a
     * level-1 vagrant), the result `{ speedRun, criticalRate, spellCastSpeed, hitRate, blockMelee,
     * blockRange, pvmDmg, pvpDmg }`.
     */
    statistics(obj: number, input: any): any;
    /**
     * `0` when the item may be put into a trade or a stall, else the `TID_*` of the refusal; `input` is
     * `{ itemId, reserved, flags, keepUntil, equipped, inUse, guildId, bindsByOption, now }`.
     */
    tradeable(input: any): number;
    /**
     * The transy scroll an armour needs, `0` when no scroll can swap it.
     */
    transy_scroll(item_id: number): number;
    /**
     * The items an armour can be swapped for with a transy scroll, ascending by id.
     */
    transy_twins(item_id: number): Uint32Array;
    /**
     * The `[dst, value, dst, value, ...]` the gems in `sockets` give an ultimate weapon.
     */
    ultimate_gem_dsts(weapon_id: number, sockets: Uint32Array): Int32Array;
    /**
     * The gem sockets of a `+ao` ultimate weapon.
     */
    ultimate_sockets(ao: number): number;
    /**
     * The costs, caps, bonus levels of the upgrade windows (see `ConstantsJs`).
     */
    upgrade_constants(): any;
    /**
     * The odds of one upgrade path at a level: `input` is `{ kind: 'general' | 'element' | 'accessory' |
     * 'accessorySafe' | 'collector', level }`, the result `{ p, space, chance, maxLevel }` (`chance` in parts per
     * `space`) or `null` at the cap of the path or for another kind.
     */
    upgrade_odds(input: any): any;
    /**
     * The branch the server's `Enchant` / `SmeltSafetyReq` takes for a pair of items: `input` is `{
     * targetItemId, materialItemId, safe, targetIsAccessory, targetPiercingSize, targetIsVisPet,
     * betterSafeUpgrades (default true), noPiercing (default true) }`, the result `{ branch: 'general' |
     * 'element' | 'accessory' | 'collector' | 'piercingSize' | 'ultimate' | 'none', protection, smeltScroll,
     * maxLevel }` (the protection item and the smelting scroll the safe path asks for, 0 = none).
     */
    upgrade_route(input: any): any;
}

/**
 * The browser simulation world: the shared deterministic sim plus the client host (command buffer, local
 * control, remote extrapolation, render view).
 */
export class SimWorld {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Adds one `FMO1` clip pack under `clip_id` (a `MotionTable` clip id).
     */
    add_clip(clip_id: number, fmo1: Uint8Array): void;
    /**
     * Adds one `FSK1` skeleton pack under `skel_id`.
     */
    add_skeleton(skel_id: number, fsk1: Uint8Array): void;
    /**
     * Applies a command buffer (`{op u16, len u16, payload}` records, Plan/31 §17.4); returns the number of
     * records accepted. A malformed buffer is an error and changes nothing.
     */
    apply_commands(cmds: Uint8Array): number;
    /**
     * `blink_point([from x,y,z, to x,y,z, range])` -> `[x, y, z, ok]`: the Blinkpool ground point from the
     * caster toward the cursor point, clamped to `skill_range` and walked back over blocked cells (Plan/43
     * §7.10); `ok == 0` answers the caster's own position.
     */
    blink_point(v: Float32Array): Float32Array;
    /**
     * `camera_check_bound([cam x,y,z, look_at x,y,z])` -> `[crash (0/1), out x,y,z, len]` (Plan/43 §8.4).
     */
    camera_check_bound(v: Float32Array): Float32Array;
    /**
     * Height of the highest floor at or below `y` (terrain and objects).
     */
    full_height(x: number, y: number, z: number): number;
    /**
     * `height_attr(x, z)`: the attribute class of the cell, `-1` outside the world.
     */
    height_attr(x: number, z: number): number;
    /**
     * Terrain height at `(x, z)`.
     */
    land_height(x: number, z: number): number;
    /**
     * Loads one `FLYC` v1 collision chunk (`Pending -> Loaded`; validated again, O(n)).
     */
    load_chunk(lx: number, lz: number, bytes: Uint8Array): void;
    /**
     * Loads one `FLYK` v1 ctrl collision-model file (the `/gen/ctrl-models` bytes unchanged): the dynamic
     * shapes of every ctrl model, once per session. Ctrl colliders already registered are re-posed.
     */
    load_ctrl_models(bytes: Uint8Array): void;
    /**
     * Loads one `FLMB` v1 model-bounds file (the `/gen/model-bounds` bytes unchanged).
     */
    load_model_bounds(bytes: Uint8Array): void;
    /**
     * Loads one `FLMT` v1 motion table (the `/gen/motion-table` bytes unchanged).
     */
    load_motion_table(bytes: Uint8Array): void;
    /**
     * `SendActMsg` on the active mover with its synchronous result.
     */
    local_act(msg: number, p1: number, p2: number, p3: number, p4: number, p5: number): number;
    /**
     * The local input record `[buttons, mouse_dx, mouse_dy, level, req_flags]`; applies to the next stepped
     * frame(s).
     */
    local_input(input: Int32Array): void;
    /**
     * `[chunks, movers, clips, bytes_static, bytes_clips, decor, ctrls_without_model]`.
     */
    memory_report(): Uint32Array;
    /**
     * `new SimWorld([flags bits, hit_attr_rule, seed, remote_extrap_cap_frames])`.
     */
    constructor(config: Uint32Array);
    /**
     * `range_point(id, x, y, z, r)`: `IsRangeObj(mover, point, r)`; `false` for an unknown mover.
     */
    range_point(id: number, x: number, y: number, z: number, r: number): boolean;
    /**
     * `ray_cast([ox,oy,oz, dx,dy,dz, max_t, mask])` -> `[hit (0/1), x, y, z, t, kind (0 none, 1 terrain, 2
     * static, 3 ctrl), id (u32 bits), nx, ny, nz]` (LH; mask `RAY_TERRAIN 1 | RAY_STATIC 2 | RAY_CTRL 4`).
     */
    ray_cast(ray: Float32Array): Float32Array;
    /**
     * Pointer to the render view (header first, `f32` words, three.js space); rebuilt by every call.
     */
    render_view(alpha: number): number;
    /**
     * Restarts the render smoothing of every mover at its simulated state.
     */
    reset_render_smoothing(): void;
    /**
     * `sample_force_path(id, grip, hit_slow, [f1 x,y,z, f2 x,y,z])` (Plan/31 §13.8; WP-SIM-29): the
     * sword-trail edge points of the mover's whole swing, `[n, e1 x,y,z, e2 x,y,z, ...]` in model space,
     * three.js convention. `grip` is 0 right hand, 1 left hand, 2 shield, 3 knuckle; `hit_slow != 0` slows
     * the clock to 0.1 per step after the hit frame. The live motion clock is not touched; a mover without a
     * skeleton or clip answers `[0]`.
     */
    sample_force_path(id: number, grip: number, hit_slow: number, force: Float32Array): Float32Array;
    /**
     * `set_world([world_id, mpu, lands_w, lands_h, fly_allowed, housing])`: drops every chunk, world mover,
     * ctrl collider, decor entry and projectile and starts all lands pending; `world_id == 0` leaves the
     * world (the other words are ignored, preview movers stay).
     */
    set_world(desc: Float32Array): void;
    /**
     * Steps up to 60 frames; returns how many were stepped. The local report accumulates over the call.
     */
    step(frames: number): number;
    /**
     * Pointer to the event buffer `[count, dropped, records...]` (8 words per record); the buffer is
     * emptied by this call.
     */
    take_events(): number;
    /**
     * `target_check(target, range, cone_cos)`: bits `IN_RANGE 1`, `IN_CONE 2`, `AS_BACK_OK 4` for the active
     * mover and `target` (Plan/43 §10.2); `0` for an unknown target.
     */
    target_check(target: number, range: number, cone_cos: number): number;
    /**
     * Drops the chunk of land `(lx, lz)` (back to pending).
     */
    unload_chunk(lx: number, lz: number): void;
}

/**
 * The angel EXP an `AngelBuff` request needs: `exp1 / 100 * percent` in integers (`percent` outside `1..=100` reads
 * as 100). `exp1` is an exact integer below 2^53.
 */
export function angel_goal_exp(exp1: number, percent: number): number;

/**
 * Decodes an `.ani` motion.
 *
 * Returns `{ meta: AniMeta, blob }`; the whole blob is one `FMO1` (`meta.motion = { off: 0, len:
 * blob.byteLength, .. }`), passed unchanged to `SimWorld.add_clip` (Plan/30 §13.4).
 */
export function decode_ani(bytes: Uint8Array): any;

/**
 * Decodes a `.chr` skeleton. `opts`: `{ space?: 'three' | 'lh' }` (the space of the grips and events).
 *
 * Returns `{ meta: ChrMeta, blob }`; the whole blob is one `FSK1` (`meta.skeleton = { off: 0, len:
 * blob.byteLength, .. }`), passed unchanged to `SimWorld.add_skeleton` (Plan/30 §13.4).
 */
export function decode_chr(bytes: Uint8Array, opts: any): any;

/**
 * Worker export: validates one `FLYC` chunk as fetched (the browser has already removed any
 * `Content-Encoding`) without touching a `SimWorld`; returns a fresh copy of the same bytes (a transferable)
 * or an error naming the violated rule.
 */
export function decode_chunk_check(bytes: Uint8Array): Uint8Array;

/**
 * Decodes a DDS texture for a GPU with `WEBGL_compressed_texture_s3tc`: BC blocks pass through, the
 * uncompressed formats are converted to their WebGL2 layouts. `opts`: `{ all_faces?: boolean }`.
 *
 * Returns `{ meta: DdsMeta, blob }`.
 */
export function decode_dds(bytes: Uint8Array, opts: any): any;

/**
 * Decodes a DDS texture to RGBA8 (the fallback without S3TC). `opts`: `{ all_faces?: boolean }`.
 *
 * Returns `{ meta: DdsMeta, blob }`.
 */
export function decode_dds_bc(bytes: Uint8Array, opts: any): any;

/**
 * Decodes a `.lnd` landscape. `opts`: `{ land_x?, land_z?, mpu?, normals? }` (the land indices of the file
 * name, world units per cell, per-vertex normals; Plan/30 §13.5).
 *
 * Returns `{ meta: LndMeta, blob }` (heights, attributes, water, layer alpha, light RGB, optional normals
 * and the object/sfx placements as typed sections).
 */
export function decode_lnd(bytes: Uint8Array, opts: any): any;

/**
 * Decodes an `.o3d` mesh. `opts`: `{ space?: 'three' | 'lh', lods?: 'all' | 'lod0', collision?: boolean,
 * reject_name_mismatch?: boolean, requested_name?: string | null }` (Plan/30 §13.2).
 *
 * Returns `{ meta: O3dMeta, blob }` (per object position/normal/uv/joints/weights/index/anim sections plus
 * the embedded `FSK1`/`FMO1` and the node-clip `FMO1` sections).
 */
export function decode_o3d(bytes: Uint8Array, opts: any): any;

/**
 * Decodes a `.sfx` effect prototype. Returns `{ meta: SfxMeta, blob: null }`.
 */
export function decode_sfx(bytes: Uint8Array): any;

/**
 * `default_hair_color(sex, hair_mesh) -> number | undefined`: `0xFF000000 | r << 16 | g << 8 | b` of the table of 12
 * §9.2; `undefined` for a sex above 1 or a hair mesh above 9.
 */
export function default_hair_color(sex: number, hair_mesh: number): number | undefined;

/**
 * `[code, slot, percent, max_tiers, band_low, band_high]` of the use of a banded EXP scroll (see the module
 * documentation).
 */
export function exp_scroll_plan(scroll: number, level: number, taken_mask: number, buff_count: number, special_count: number): Int32Array;

/**
 * `{ parsers, pack, bcDecode, bcEncode, mips, limits: { maxInputBytes, maxTexels, maxDim } }`: the version
 * constants of the format layer (folded into asset cache keys) and the decoder caps.
 */
export function formats_info(): any;

/**
 * Greeting used by the placeholder page and the web tests to prove that the module is callable.
 */
export function hello(name: string): string;

/**
 * The order `SortInventory` moves the usable bag items in: indices into `kinds`, stable inside one kind.
 */
export function inventory_sort_order(kinds: Uint32Array): Uint32Array;

/**
 * The stat bonus of a pet: the sum of the rolled values (`values` = the nine values of its kind, `avail_levels`
 * = the roll per grade D..S) of the grades D up to `level`.
 */
export function pet_avail_bonus(values: Int32Array, avail_levels: Uint8Array, level: number): number;

/**
 * `CPet::GetEnergyPercent`.
 */
export function pet_energy_percent(energy: number, max_energy: number): number;

/**
 * `CPet::GetExpPercent`.
 */
export function pet_exp_percent(level: number, exp: number): number;

/**
 * The pieces of feed one `UsePetFeed` uses: the whole stack, capped by what the pet needs.
 */
export function pet_feed_units(level: number, exp: number, energy: number, max_energy: number, stack: number): number;

/**
 * `AddPetExpOnKill`: `[0, new exp]` or `[1, 0]` (levels up).
 */
export function pet_kill_exp(level: number, exp: number, base_exp: number, super_or_boss: boolean, player_level: number, monster_level: number): Uint32Array;

/**
 * `MAKELONG( mover, level )`.
 */
export function pet_levelup_word(mover: number, level: number): number;

/**
 * `CPet::GetMaxExp`: 50 000 for the egg, 100 000 for the grades.
 */
export function pet_max_exp(level: number): number;

/**
 * `CPet::GetIndex`: the model of the pet for `level` (-1 when the kind has no mover row).
 */
export function pet_mover(egg_mover: number, movers: Uint32Array, level: number): number;

/**
 * Whether pocket `index` can be used (`CPocketController::IsAvailable`).
 */
export function pocket_available(perm_extrabags: boolean, index: number, exists: boolean, expired: boolean): boolean;

/**
 * The slots of pocket `index`: `18 / 42 / 42` with `bigger_pocket`, else `6 / 24 / 24`; 0 for no such pocket.
 */
export function pocket_capacity(index: number, bigger_pocket: boolean): number;

/**
 * Hash of a fixed `libm`/`f32` computation sequence derived from `seed`; identical natively, under
 * Node and in the browser (Plan/02 §9.5). Returned to JavaScript as a `BigInt`.
 */
export function selftest_det(seed: number): bigint;

/**
 * `validate_bank_pin(pin) -> number`: 0 when the PIN is four digits and not `"0000"`, else the `TID_2ND_PASSWORD_*`
 * text id of 12 §9.4 (4296 format, 4297 all zero).
 */
export function validate_bank_pin(pin: string): number;

/**
 * `validate_character_name(raw, names, min, max) -> { name?: string, tid?: number }` (12 §9.3 rules 1-6).
 *
 * `names` is the JSON of the `names` bundle's name rules (`invalid_substrings`, `allowed_letters`).
 */
export function validate_character_name(raw: string, names: Uint8Array, min: number, max: number): any;

/**
 * The crate version (workspace version), shown by the placeholder web page.
 */
export function version(): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_rulescontext_free: (a: number, b: number) => void;
    readonly __wbg_simworld_free: (a: number, b: number) => void;
    readonly angel_goal_exp: (a: number, b: number) => number;
    readonly decode_ani: (a: any) => [number, number, number];
    readonly decode_chr: (a: any, b: any) => [number, number, number];
    readonly decode_chunk_check: (a: number, b: number) => [number, number, number, number];
    readonly decode_dds: (a: any, b: any) => [number, number, number];
    readonly decode_dds_bc: (a: any, b: any) => [number, number, number];
    readonly decode_lnd: (a: any, b: any) => [number, number, number];
    readonly decode_o3d: (a: any, b: any) => [number, number, number];
    readonly decode_sfx: (a: any) => [number, number, number];
    readonly default_hair_color: (a: number, b: number) => number;
    readonly exp_scroll_plan: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly formats_info: () => any;
    readonly hello: (a: number, b: number) => [number, number];
    readonly inventory_sort_order: (a: number, b: number) => [number, number];
    readonly pet_avail_bonus: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly pet_energy_percent: (a: number, b: number) => number;
    readonly pet_exp_percent: (a: number, b: number) => number;
    readonly pet_feed_units: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly pet_kill_exp: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
    readonly pet_levelup_word: (a: number, b: number) => number;
    readonly pet_max_exp: (a: number) => number;
    readonly pet_mover: (a: number, b: number, c: number, d: number) => number;
    readonly pocket_available: (a: number, b: number, c: number, d: number) => number;
    readonly pocket_capacity: (a: number, b: number) => number;
    readonly rulescontext_angel_odds: (a: number, b: number) => [number, number];
    readonly rulescontext_angel_shares: (a: number, b: number) => [number, number];
    readonly rulescontext_armor_set_level: (a: number, b: number, c: number) => number;
    readonly rulescontext_attack_speed: (a: number, b: number, c: number, d: number, e: number, f: number) => number;
    readonly rulescontext_buy_unit_price: (a: number, b: any) => [number, number, number, number];
    readonly rulescontext_can_transy: (a: number, b: number, c: number) => number;
    readonly rulescontext_casting_ani_speed: (a: number, b: number) => number;
    readonly rulescontext_chip_cost: (a: number, b: number) => number;
    readonly rulescontext_client_attackable: (a: number, b: any, c: any, d: any) => [number, number, number];
    readonly rulescontext_derived_stats: (a: number, b: number, c: any) => [number, number, number];
    readonly rulescontext_drain_sync: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number) => [number, number, number, number];
    readonly rulescontext_dst_apply_ops: (a: number, b: number, c: number) => [number, number, number, number];
    readonly rulescontext_dst_forget: (a: number, b: number, c: number) => void;
    readonly rulescontext_dst_reset_all: (a: number) => void;
    readonly rulescontext_dst_snapshot: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly rulescontext_dst_view: (a: number, b: number) => [number, number];
    readonly rulescontext_element_card: (a: number, b: number) => number;
    readonly rulescontext_equip_check: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => number;
    readonly rulescontext_exp_percent_x100: (a: number, b: number, c: number) => number;
    readonly rulescontext_fast_job_targets: (a: number, b: number, c: number) => [number, number, number, number];
    readonly rulescontext_fast_job_window: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly rulescontext_hair_cost: (a: number, b: any) => [number, number, number];
    readonly rulescontext_item_min_max: (a: number, b: number, c: number, d: number) => [number, number];
    readonly rulescontext_item_multiplier: (a: number, b: number, c: number, d: number) => number;
    readonly rulescontext_item_tooltip_numbers: (a: number, b: any) => [number, number, number];
    readonly rulescontext_mail_storage_fee: (a: number, b: number, c: number) => number;
    readonly rulescontext_move_speed: (a: number, b: number, c: number, d: number) => number;
    readonly rulescontext_mover_params: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly rulescontext_new: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: any) => [number, number, number];
    readonly rulescontext_pierce_able: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => number;
    readonly rulescontext_quick_sell_unit_price: (a: number, b: any) => [number, number, number, number];
    readonly rulescontext_sell_unit_price: (a: number, b: any) => [number, number, number, number];
    readonly rulescontext_skill_plan_check: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number, number];
    readonly rulescontext_skill_point_cost: (a: number, b: number) => [number, number, number];
    readonly rulescontext_stat_preview: (a: number, b: number, c: any, d: number, e: number) => [number, number, number];
    readonly rulescontext_statistics: (a: number, b: number, c: any) => [number, number, number];
    readonly rulescontext_tradeable: (a: number, b: any) => [number, number, number];
    readonly rulescontext_transy_scroll: (a: number, b: number) => number;
    readonly rulescontext_transy_twins: (a: number, b: number) => [number, number];
    readonly rulescontext_ultimate_gem_dsts: (a: number, b: number, c: number, d: number) => [number, number];
    readonly rulescontext_ultimate_sockets: (a: number, b: number) => number;
    readonly rulescontext_upgrade_constants: (a: number) => [number, number, number];
    readonly rulescontext_upgrade_odds: (a: number, b: any) => [number, number, number];
    readonly rulescontext_upgrade_route: (a: number, b: any) => [number, number, number];
    readonly selftest_det: (a: number) => bigint;
    readonly simworld_add_clip: (a: number, b: number, c: number, d: number) => [number, number];
    readonly simworld_add_skeleton: (a: number, b: number, c: number, d: number) => [number, number];
    readonly simworld_apply_commands: (a: number, b: number, c: number) => [number, number, number];
    readonly simworld_blink_point: (a: number, b: number, c: number) => [number, number];
    readonly simworld_camera_check_bound: (a: number, b: number, c: number) => [number, number];
    readonly simworld_full_height: (a: number, b: number, c: number, d: number) => number;
    readonly simworld_height_attr: (a: number, b: number, c: number) => number;
    readonly simworld_land_height: (a: number, b: number, c: number) => number;
    readonly simworld_load_chunk: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly simworld_load_ctrl_models: (a: number, b: number, c: number) => [number, number];
    readonly simworld_load_model_bounds: (a: number, b: number, c: number) => [number, number];
    readonly simworld_load_motion_table: (a: number, b: number, c: number) => [number, number];
    readonly simworld_local_act: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => number;
    readonly simworld_local_input: (a: number, b: number, c: number) => void;
    readonly simworld_memory_report: (a: number) => [number, number];
    readonly simworld_new: (a: number, b: number) => [number, number, number];
    readonly simworld_range_point: (a: number, b: number, c: number, d: number, e: number, f: number) => number;
    readonly simworld_ray_cast: (a: number, b: number, c: number) => [number, number];
    readonly simworld_render_view: (a: number, b: number) => number;
    readonly simworld_reset_render_smoothing: (a: number) => void;
    readonly simworld_sample_force_path: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number, number, number];
    readonly simworld_set_world: (a: number, b: number, c: number) => [number, number];
    readonly simworld_step: (a: number, b: number) => number;
    readonly simworld_take_events: (a: number) => number;
    readonly simworld_target_check: (a: number, b: number, c: number, d: number) => number;
    readonly simworld_unload_chunk: (a: number, b: number, c: number) => void;
    readonly validate_bank_pin: (a: number, b: number) => number;
    readonly validate_character_name: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number, number];
    readonly version: () => [number, number];
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
