// The ports of the input layer (Plan/43 §1.3, §17 "Interfaces requested from other client docs"): everything `input/` needs
// from the window manager (42), the chat and HUD (45), the renderer (41), the sim bridge (40) and the stores is declared
// here as a narrow interface, so that every module is testable with a fake and the app wires the real objects in one place
// (`app/inputHost.ts`). The optional members of {@link InputHooks} are the integration points of features that land later
// (world map, revival window, mover menu, ...): absent = the step is skipped.

import type { BlinkResult, CameraBound, RayHit } from '../game/ports';
import type { Vec3 } from '../game/simCmd';
import type { ClientMsg } from '../net/codec';
import type { PickProxy, Ray3, ScreenRect, Vec3LH } from '../render/types';
import type { CameraSubject } from './camera/backCamera';
import type { PointerLockPort } from './flight';
import type { InputMode, RouterWindows } from './router';

export type { InputMode, RouterWindows };

/** The sim bridge operations of the input layer (`SimBridge` satisfies it structurally). */
export interface SimPort {
  /** The command writer; the bytes are applied at the next frame. */
  readonly cmd: {
    localCmd(
      act: number,
      target: number,
      range: number,
      ar: number,
      p3: readonly [number, number, number],
      flags: number,
    ): unknown;
    localDestPos(pos: Vec3, forward: boolean): unknown;
    localClear(what: number): unknown;
    localFace(angle: number): unknown;
    setTracking(target: number): unknown;
  };
  /** Cap Q: `undefined` = no slot this iteration. */
  rayCast(origin: Vec3, dir: Vec3, maxT: number, mask: number): RayHit | null | undefined;
  /**
   * `SendActMsg` on the own mover (Plan/31 17.3 `local_act`): the result code of the sim (`> 0` started; for `ATK1` the
   * attack flags). Cap Q: `undefined` = no slot this iteration, the caller issues it again in the next one.
   */
  localAct?(msg: number, p: readonly [number, number, number, number, number]): number | undefined;
  /** How many more queries the bridge will execute in this iteration (cap Q = 2 in total, Plan/40 7.3). */
  queryBudget?(): number;
  heightAttr?(x: number, z: number): number | undefined;
  blinkPoint?(self: Vec3, to: Vec3, range: number): BlinkResult | undefined;
  /** `target_check(target, range, coneCos)`: the `IN_RANGE | IN_CONE | AS_BACK_OK` bits (Plan/43 section 10.2); cap Q. */
  targetCheck?(target: number, range: number, coneCos: number): number | undefined;
  /** Cap C. */
  cameraCheckBound(eye: Vec3, lookAt: Vec3): CameraBound;
  /** The world the sim holds (0 = none): a change resets the telescope. */
  readonly worldId: number;
}

/** The renderer operations of the input layer (`RenderApi` satisfies it). */
export interface RenderPort {
  visibleObjects?(): readonly PickProxy[];
  raycastPrecise?(id: number, ray: Ray3): number | null;
  projectBounds?(id: number): ScreenRect | null;
  setHighlight?(id: number): void;
  /** `XI_GEN_MOVEMARK01` at the clicked ground point (Plan/43 §6.6). */
  moveMark?(pos: Vec3LH, normal: Vec3LH | null): void;
}

/** The window-manager operations the world logic needs (a structural subset of `WindowManager`). */
export interface WorldWindows extends RouterWindows {
  isOpen(app: number): boolean;
  focusedApp(): number | null;
  anyOpen(apps: readonly number[]): boolean;
  isVendorWindowOpen(): boolean;
  isDraggingShortcut(): boolean;
  cancelDragUnlessFrom(app: number): void;
  topClosableWindow(): object | null;
  /** `GetWndBase()` rule of the Esc chain: the top window is closed (`'closed'`), kept (`'kept'`) or there is none. */
  closeTopWindow(): 'none' | 'kept' | 'closed';
  /** The top window is the mover context menu or window id 10: the Esc chain only focuses the world. */
  topIsMenuOrWorldFocus?(): boolean;
  openStartMenu(aboveTaskbar: true): void;
  /** The world (the root window) takes the focus. */
  focusWorld?(): void;
  clearAllWindows(): void;
  restoreWindows(): void;
  readonly allHidden: boolean;
  objectExecutorApplet(app: number): void;
}

export interface TaskbarPort {
  executeItemSlot(i: number): void;
  setItemPage(n: number): void;
  slotIsEquipItem(i: number): boolean;
}

export interface ChatPort {
  /** Enter: opens the chat when it is closed and gives its edit the keyboard. */
  focusInput(): boolean;
  /** The chat window exists. */
  isOpen(): boolean;
  /** R: the whisper reply (`/whisper <last whisperer> `); `false` without a last whisperer or a chat window. */
  whisperReply(): boolean;
  /** The chat window is the focused window (movement keys are treated as up). */
  isFocused(): boolean;
}

/** What the input layer knows about the own player (the stores and the render view). */
export interface SelfInfo {
  readonly id: number;
  readonly level: number;
  readonly dead: boolean;
  readonly flying: boolean;
  /** Logical LH position and yaw (the render view of the last frame). */
  readonly pos: Vec3LH;
  readonly angle: number;
  /** `m_dwMode` bits (`DONMOVE_MODE`, ...). */
  readonly modeFlags: number;
  /** `m_dwStateFlag`-independent: `OBJSTA_*` state word of the active mover (render view). */
  readonly state: number;
  readonly authority: number;
  /** `GetRadius()` of the own mover (the arrival test of the follow upkeep). */
  readonly radius: number;
  /** The own vendor is open. */
  readonly ownVendorOpen: boolean;
  /** `m_vtInfo.GetOther() != NULL`: the own player looks into another player's shop (a trade partner counts too). */
  readonly visitingVendor?: boolean;
  /** `IsRegionAttr(RA_SAFETY)` at the own position (create-monster placement). */
  readonly regionSafe?: boolean;
  /** `CHS_SETSTONE`. */
  readonly settingStone: boolean;
  /** The own mover sits (`OBJSTAF_SIT`). */
  readonly sitting?: boolean;
  /** The own mover is in combat mode (`OBJSTAF_COMBAT`, the battle motion toggles it). */
  readonly combat?: boolean;
  /** Current MP (the wand charge fires at power 3 when it is below 5). */
  readonly mp?: number;
  /** `pk_propensity > 0`. */
  readonly chaotic?: boolean;
  /** `m_idGuild` (`visual.guild.idGuild`), 0 without a guild: the guild-war Tab list needs the own guild (WP-INPUT-16). */
  readonly guildId?: number;
}

/** The item in the active hand (`GetActiveHandItemProp`): what the attack commands decide on. `null` = bare hands. */
export interface HandItem {
  readonly id: number;
  /** `IK2_*`. */
  readonly kind2: number;
  /** `IK3_*`. */
  readonly kind3: number;
  /** `AR_*` code of `attack_range`. */
  readonly attackRange: number;
  /** `use_motion` (`NULL_ID` or <= 0 when the weapon has none). */
  readonly useMotion: number;
}

/** What the input layer knows about another object. */
export interface ObjectInfo {
  readonly kind: 'mover' | 'item' | 'ctrl';
  readonly isPlayer: boolean;
  readonly peaceful: boolean;
  /** `CK1_HOUSING` furniture. */
  readonly housing: boolean;
  /** Dead mover. */
  readonly dead: boolean;
  readonly transparent: boolean;
  /** LH position of the object (render view), `null` when it is not known. */
  readonly pos: Vec3LH | null;
  /** `GetRadius()` (the render view's radius slot), 0 when unknown. */
  readonly radius: number;
  /** MoverProp `killable != 0` (monsters; default true). */
  readonly killable?: boolean;
  /** MoverProp `class` (`RANK_*`). */
  readonly rank?: number;
  /** MoverProp `nChaotic` (guards: negative = good, positive = chaotic side). */
  readonly chaotic?: number;
  /** MoverProp `flying == 1` (a flying monster). */
  readonly flyingNpc?: boolean;
  /** A player with an open personal shop (`m_vtInfo.IsVendorOpen()`: its title balloon is up). */
  readonly vendorOpen?: boolean;
  /** `m_idGuild` (`visual.guild.idGuild`) of a player, 0 when it has no known guild: the guild-war Tab list (WP-INPUT-16). */
  readonly guildId?: number;
  // ---- housing / guild house (ui-house-quiz)
  /** `CK1_GUILD_HOUSE` furniture (`CtrlProp::IsGuildHousingObj`): a click selects its row of the house and opens the furniture menu, the focus does not change (WP-INPUT-17). */
  readonly guildHouse?: boolean;
}

/**
 * What the input layer needs to know about the job skill in one slot (the prop at the effective level, Plan/43 section 7.10):
 * `GetSkill(0, slot)` and its `ItemProp` / `AddSkillProp`.
 */
export interface SkillInfo {
  /** The index into the 45 job-skill slots. */
  readonly slot: number;
  /** `SI_*` id. */
  readonly skillId: number;
  /** The stored level (`0` = not learnt yet: the skill cannot be used). */
  readonly level: number;
  /** `WUI_*` (`dwUseChance`). */
  readonly useChance: number;
  /** `EXT_*` (`dwExeTarget`). */
  readonly exeTarget: number;
  /** `nEvildoing` (< 0 hostile, > 0 beneficial). */
  readonly evildoing: number;
  /** `AR_*` code of the arrival range (`dwAttackRange`). */
  readonly attackRange: number;
  /** `AS_*` (`dwAtkStyle`; `AS_BACK` needs the back of the target). */
  readonly atkStyle: number;
  /** `AddSkillProp.dwSkillRange` of the effective level (the Blinkpool range). */
  readonly skillRange: number;
  /** `KT_MAGIC` / `KT_SKILL`. */
  readonly skillType: number;
}

/** The job-skill table and the client skill state of Plan/17 section 6.14 (`web/src/game/combat/skills.ts`). */
export interface SkillAccess {
  /** `GetSkill(0, slot)`; `null` for an empty slot or a skill whose prop is unknown. */
  skill(slot: number): SkillInfo | null;
  /** `IsBullet(prop)`: the arrow / bullet the skill needs is equipped (or `no_arrows_poster`). */
  isBullet(slot: number): boolean;
  /** Blinkpool: the ground point chosen by `cmdSetUseSkill` (`m_vTelePos`), read at the arrival and by the hit frame. */
  setTelePos(pos: Vec3LH | null): void;
  telePos(): Vec3LH | null;
}

/** The action slot as the input layer sees it (the taskbar model and the runtime of Plan/17 section 6.13). */
export interface ActionSlotAccess {
  /** The skill slots of the queue, compact (`queue[0]` first); empty cells are dropped. */
  queue(): readonly number[];
  /** `m_nExecute`: 0 idle, 1 waiting for the first cast, 2 running. */
  executing(): 0 | 1 | 2;
  setExecuting(n: 0 | 1 | 2): void;
}

export interface InputOptions {
  readonly mouseSpeed: number;
  readonly cameraLock: boolean;
  readonly autoAttack: boolean;
  readonly zoomLimit: boolean;
  readonly offTarget: boolean;
  /** `render.hidePets` (the `offPets` alias). */
  readonly offPets: boolean;
}

/** Features the input layer reads (`ClientFeatureConfig.features`). */
export interface InputFeatures {
  readonly no_quake?: boolean;
  readonly hide_pets?: boolean;
  readonly pet_filter?: boolean;
  readonly party_find?: boolean;
  readonly teleportation_system?: boolean;
  readonly ftool?: boolean;
  readonly farm_bot?: boolean;
  readonly no_quest_button?: boolean;
  readonly targetsave?: boolean;
  readonly wand_autoattacks?: boolean;
  readonly no_arrows_poster?: boolean;
}

/** The integration points of later features (docs 15/22/45). Every member is optional. */
export interface InputHooks {
  /** Esc step 1: the world-map overlay. */
  readonly worldMap?: { shown(): boolean; hide(): void; click?(x: number, y: number): void };
  /** Esc step 2: the revival window. */
  readonly revival?: { isOpen(): boolean; open(): void };
  /** Esc step 3: the start menu is visible. */
  startMenuVisible?(): boolean;
  /** Esc step 6: the quest-NPC destination arrow. */
  readonly questDestination?: { active(): boolean; clear(): void };
  /**
   * Esc step 7 and the click-destination step: create-monster placement mode (`g_Neuz.m_pCreateMonItem`; the item-use router of 45 starts
   * it). `place` sends `CreateMonster` for the waiting item at the ground point and ends the mode (`false`: there was no item).
   */
  readonly createMonster?: {
    active(): boolean;
    leave(): void;
    place(pos: Vec3LH): boolean;
  };
  /** `pMover->IsRegionAttr(RA_SAFETY)` of another player (the personal-shop click: in a safe region Ctrl does not matter). */
  regionSafe?(id: number): boolean;
  /** The party-summon buff icon was double clicked (45 opens `APP_SUMMON_PARTY_USE` when a summon is pending and it is not the own). */
  partySummon?(): void;
  /** Esc step 5: the hero casting skill (`SI_KNT_HERO_DRAWING`) has a future end frame. */
  cancelHeroCast?(): void;
  /** `moverMenu.open` / `ShowMoverMenu` (doc 45 builds the menu). */
  showMoverMenu?(id: number, anchor: ScreenRect | null): void;
  /** The player branch of `IsAttackAble` (`rules.clientAttackable`: duels, wars, PK): `0` no, `1` yes, `2` yes and remember. Default: no. */
  playerAttackable?(id: number): number;
  /** `ctrlMenu.open` (guild-house furniture). */
  showCtrlMenu?(id: number): void;
  /**
   * The buff icon under the pointer (`GetBuffIconRect`: the `II_SYS_*` item id of an item buff), `null` for none. The buff strip window
   * of 42 claims the pointer over its icons itself (45 `BuffActions.click` / `activate`) and `InputDeps.uiClaimed` keeps that click away
   * from the world (BFIX1 #004), so the world sees a click here only when no window claims it (the running client provides no hook).
   */
  buffIconAt?(x: number, y: number): number | null;
  /** `selfActions.cancelBaseMotion` (`StateModeReq` cancel). */
  cancelBaseMotion?(): void;
  /** `baseMotionMode` is on. */
  baseMotionActive?(): boolean;
  /** `closeOnMove()` of doc 45 and `ctrlCountdown.cancel` of doc 15. */
  closeOnMove?(): void;
  /** A housing / guild-house deploy is armed. */
  deployActive?(): boolean;
  /** 1:1 guild combat: the camera does not collide, player menus are off. */
  readonly gc1to1?: { participant(): boolean; possible(id: number): boolean };
  /** Plays the idle sound of a clicked mover (`audio.play(sndIdle1)`). */
  playIdleSound?(id: number): void;
  /** Toggles the FPS display (`input.showFps`). */
  toggleFps?(): void;
  /** The melee request site (`WndWorldControlPlayer.cpp:814`): the audio engine's combat-music cue (44 T40). */
  localAttackStarted?(): void;
  /** The wand attack started: the action-point display grows by one (45, `actionSlot.actionPoint += 1`). */
  actionPointUp?(): void;
  /**
   * `__S8_PK`: a good skill (`nEvildoing > 0`) on another chaotic player needs the Ctrl key while `events.pk` is on
   * (`ProcessMoveArrival`); `true` = the target is such a player. Absent = never.
   */
  chaoticPlayer?(id: number): boolean;
  /** On the guild-war map (`WI_WORLD_GUILDWAR`): a dead next target is skipped. */
  guildWarMap?(): boolean;
  /** The own character wears a text-disguise buff (`IK3_TEXT_DISGUISE`): nothing is attackable. */
  textDisguised?(): boolean;
  /** The highlighted object is a ground item: doc 45 shows its tooltip anchored at the projected bound rect (§4.10). */
  itemTooltip?(id: number, anchor: ScreenRect | null): void;
  /** Ctrl is the `chaotic` modifier of other windows; reserved. */
  readonly statConfirm?: { yes(): void; cancel(): void };
  // ---- housing / guild house (ui-house-quiz)
  /**
   * The placement of a housing or guild-house furniture (WP-INPUT-17, `input/deploy.ts`; the controller is `features/events/deploy`): `active()` while a ghost follows the pointer, `cursor( p )` with the terrain point
   * under the pointer (`null` keeps the last one), `confirm()` the click, `cancel()` Esc (`true` when a placement ended), `wheel( n )` the rotation (positive = towards the user), `pageKey( code )` PageUp / PageDown.
   */
  readonly deploy?: {
    active(): boolean;
    cursor(p: Vec3LH | null): void;
    confirm(): void;
    cancel(): boolean;
    wheel(notches: number): void;
    pageKey(code: string): void;
  };
  /** `SetObjFocus` of a `CK1_GUILD_HOUSE` ctrl: the row of the house is selected (the open storage window follows); `true` = handled, the focus stays. */
  guildFurniture?(id: number): boolean;
  /** Esc without a placement: `GuildHouse->m_dwSelectedObjID = NULL_ID`. */
  clearFurnitureSelection?(): void;
}

/** The chat line of the telescope and the system notices (`ui.chat.putString`). */
export type NoticeSink = (text: string, color: number, style: number) => void;

/** Everything `createInput` needs. */
export interface InputDeps {
  now(): number;
  mode(): InputMode;
  readonly sim: SimPort;
  render(): RenderPort | null;
  readonly windows: WorldWindows;
  readonly taskbar: TaskbarPort;
  readonly chat: ChatPort;
  /** `CursorManager.setWorldCursor`. */
  setWorldCursor(id: number): void;
  options(): InputOptions;
  features(): InputFeatures | null;
  self(): SelfInfo | null;
  /** The item in the active hand, `null` for bare hands. */
  hand?(): HandItem | null;
  /** The arrow slot is filled (`IsBullet`); with `features.no_arrows_poster` the attack commands do not ask. */
  bulletEquipped?(): boolean;
  /** The job-skill table (WP-INPUT-13); absent = no skill can be used. */
  readonly skills?: SkillAccess;
  /** The action slot (WP-INPUT-14); absent = the C key does nothing. */
  readonly actionSlot?: ActionSlotAccess;
  /** Sends a protocol message (`SetTargetReq`, `MeleeAttackReq`, ...); absent in tests that do not look at the wire. */
  send?(msg: ClientMsg): void;
  /** The active mover of the render view that `camera(due, alpha, view)` received (`null` when it is not in the view). */
  cameraSubject(view: unknown): CameraSubject | null;
  /** What is known about another object (the entity table and the render view of the last frame). */
  entityInfo(id: number): ObjectInfo | null;
  /** The size of the world canvas in CSS pixels. */
  viewport(): { readonly w: number; readonly h: number };
  /**
   * The UI consumed this DOM pointer event (`ui/wm/pointerClaims.ts`: a window, an icon of a pass-through window such as the buff
   * windows, a modal): a claimed press, wheel or move never reaches the world (BFIX1 #004). Absent = nothing is claimed.
   */
  uiClaimed?(e: object): boolean;
  /** The text and the colour of a client string id (the telescope and run/walk notices). */
  text?(tid: number): string;
  textColor?(tid: number): number;
  notice?: NoticeSink;
  /** The tab is hidden (§2.4): the record is neutral and auto-run is suspended. */
  hidden?(): boolean;
  /**
   * The Pointer Lock of the world canvas (flight steering, Plan/43 §9.3, D7). Absent = no lock (an environment without the API): the
   * flight steers with the pointer deltas under pointer capture instead.
   */
  readonly pointerLock?: PointerLockPort;
  readonly hooks?: InputHooks;
  /** Cosmetic RNG for the quake jitter (tests inject one). */
  readonly random?: () => number;
}
