// The targeting state and the focus protocol (Plan/43 §7.1-§7.3, §7.5, §7.11; WP-INPUT-07 subset, WP-INPUT-09): the focus, the
// next target, the render target and the auto-attack flag that the click handlers and the control decision share, and the
// messages that follow the focus:
//
//   setFocus(o, send)   `SetTargetReq{old, 1}` (release) when a live mover loses the focus, `SetTargetReq{new, 2}` for a new mover
//                       focus, `SetTargetReq{NULL_ID, 2}` for an emptied focus (World.cpp:253-347)
//   server-driven       `SetTargetNtf` focuses a live mover in view (a `NULL_ID` or unknown target is a no-op), `ClearTarget` empties
//   MoverDeath          the own death empties the focus; a kill by the own player keeps the focus unless the victim WAS the focus
//                       (`features.targetsave`) and takes over the queued next target (§7.11)
//   flight              a mover that appears again while the own player flies and was the last focus becomes the focus (World.cpp:456)
//
// The environment (`TargetingEnv`) is what the object does not own: which ids are movers, whether they live, the wire and the
// feature switches. `createInput` builds it from the entity table; the tests pass a scripted one.

/** `NEXTSKILL_NONE`. */
export const NEXTSKILL_NONE = -1;
/** `NEXTSKILL_ACTIONSLOT`. */
export const NEXTSKILL_ACTIONSLOT = -2;
/** `m_timerFocusAttack`: a held click starts the attack after this long (500 ms). */
export const FOCUS_ATTACK_MS = 500;
/** `NULL_ID`: the empty target of `SetTargetReq`. */
export const NULL_TARGET = 0xffff_ffff;

/** One focus change, as `CWorld::SetObjFocus` sees it (`send` is false for the ones the client makes without telling). */
export interface FocusChange {
  readonly old: number | null;
  readonly next: number | null;
  readonly send: boolean;
}

/** What `setFocus` needs to know about an object (kept as the first-cut name of the type). */
export interface FocusKinds {
  /** The object is a mover (the focus of a mover becomes `focusOld`). */
  isMover(id: number): boolean;
  /** `CK1_HOUSING` furniture: never focused. */
  isHousing(id: number): boolean;
  // ---- housing / guild house (ui-house-quiz)
  /** `CK1_GUILD_HOUSE` furniture (`IsGuildHousingObj`): `setFocus` selects its row (`sink.guildFurniture`) and leaves the focus alone. */
  isGuildHouse?(id: number): boolean;
  /** A mover that lives (`IsLive()`); default: every mover. */
  isLive?(id: number): boolean;
  /** The object is known to the client (the entity table has it, it is not marked removed). */
  exists?(id: number): boolean;
  /** A peaceful mover (an NPC without a fight). */
  isPeaceful?(id: number): boolean;
  /** A GM in `TRANSPARENT_MODE`: not targetable. */
  isTransparent?(id: number): boolean;
  /** Dead (`IsDie()`). */
  isDead?(id: number): boolean;
  /** The own mover id. */
  selfId?(): number;
  /** The own player flies (a mover that reappears becomes the focus again). */
  flying?(): boolean;
  /** On the guild-war map (`WI_WORLD_GUILDWAR`): a dead next target is skipped. */
  guildWarMap?(): boolean;
  /** `features.targetsave`. */
  targetSave?(): boolean;
}

export interface TargetingSink {
  /** The focus changed (the probe and the target store follow). */
  focusChanged?(c: FocusChange): void;
  /** `SendSetTarget(target, clear)`: `clear` 1 releases the claim, 2 remembers the target (Plan/17 §3.10). */
  sendTarget?(target: number, clear: 0 | 1 | 2): void;
  /** `SendMoverFocus` for a game master that focuses a player (M8, gm.proto): `idPlayer` of the focused player. */
  gmFocus?(id: number): void;
  // ---- housing / guild house (ui-house-quiz)
  /** A `CK1_GUILD_HOUSE` ctrl was picked: the storage window selects its row; `true` = handled (the focus stays), `false`/absent = an ordinary ctrl focus. */
  guildFurniture?(id: number): boolean;
  /** `m_bSetQuestNPCDest = FALSE` (`World.cpp:289-298`): any other focus change ends the quest-NPC destination arrow. */
  clearQuestDestination?(): void;
}

export class Targeting {
  /** `CWorld::m_pObjFocus` (mover, item or ctrl). */
  focus: number | null = null;
  /** `m_idObjFocusOld`: the last focused mover. */
  focusOld: number | null = null;
  /** `m_pNextTargetObj` (VER >= 11). */
  nextTarget: number | null = null;
  /** `m_pRenderTargetObj`: the bracket of the attacked target. */
  renderTarget: number | null = null;
  /** `m_pSelectRenderObj`: the hover bracket while nothing is focused. */
  selectRenderObj: number | null = null;
  /** `m_bSelectTarget`. */
  selectTarget = false;
  /** `m_bAutoAttack`. */
  autoAttack = false;
  /** `m_dwNextSkill`. */
  nextSkill: number = NEXTSKILL_NONE;
  /** `m_timerFocusAttack` reset time (ms). */
  tFocusAttack = Number.NEGATIVE_INFINITY;
  /** `m_idLastTarget`. */
  lastTarget: number | null = null;
  /** `SetTargetReq` messages sent, newest last (the probe and the tests read it). */
  readonly sent: { target: number; clear: number }[] = [];

  constructor(
    private readonly kinds: FocusKinds,
    readonly sink: TargetingSink = {},
  ) {}

  private isLive(id: number): boolean {
    return this.kinds.isLive ? this.kinds.isLive(id) : this.kinds.isMover(id);
  }

  private send(target: number, clear: 0 | 1 | 2): void {
    this.sent.push({ target, clear });
    if (this.sent.length > 50) this.sent.shift();
    this.sink.sendTarget?.(target, clear);
  }

  /** `CWorld::SetObjFocus(o, send)` (§7.2); returns whether the focus changed. */
  setFocus(id: number | null, send = true): boolean {
    if (id !== null && this.kinds.isHousing(id)) return false;
    // ---- housing / guild house (ui-house-quiz): a guild-house furniture selects its row and passes (`World.cpp:271-286`)
    if (
      id !== null &&
      this.kinds.isGuildHouse?.(id) === true &&
      this.sink.guildFurniture?.(id) === true
    ) {
      return false;
    }
    this.sink.clearQuestDestination?.();
    const old = this.focus;
    if (old !== null && this.kinds.isMover(old)) this.focusOld = old;
    if (id === old) return false;
    // the claim on the old target ends (the server clears `id_targeter`) while it lives
    if (old !== null && this.kinds.isMover(old) && this.isLive(old) && send) this.send(old, 1);
    if (id !== null) {
      if (this.kinds.isMover(id) && send) this.send(id, 2);
    } else {
      if (send) this.send(NULL_TARGET, 2);
      this.nextTarget = null;
      this.renderTarget = null;
    }
    this.focus = id;
    this.sink.focusChanged?.({ old, next: id, send });
    return true;
  }

  // ---- server-driven focus (§7.3) ------------------------------------------------------------------------------------

  /** `SetTargetNtf{target}`: a live mover in view is focused; `NULL_ID` and unknown objects change nothing. */
  onServerTarget(id: number): void {
    if (id === NULL_TARGET || id === 0) return;
    if (this.kinds.exists && !this.kinds.exists(id)) return;
    if (!this.kinds.isMover(id) || !this.isLive(id)) return;
    this.setFocus(id);
  }

  /** `ClearTarget`: the server forces the client to drop its focus. */
  onClearTarget(): void {
    this.setFocus(null);
  }

  /** A mover was added: while the own player flies, the object it focused last reappears as the focus (World.cpp:456-466). */
  onMoverSpawned(id: number): void {
    if (this.kinds.flying?.() !== true) return;
    if (this.focusOld === id && this.focus !== id && this.kinds.isMover(id)) this.setFocus(id);
  }

  /** `DelObj` of the focused or the next target (`World.cpp:892`). */
  onObjectRemoved(id: number): void {
    if (this.focus === id) this.setFocus(null);
    if (this.nextTarget === id) this.nextTarget = null;
    if (this.renderTarget === id) this.renderTarget = null;
    if (this.selectRenderObj === id) this.selectRenderObj = null;
    if (this.lastTarget === id) this.lastTarget = null;
    if (this.focusOld === id) this.focusOld = null;
  }

  // ---- next target and death (§7.11) ---------------------------------------------------------------------------------

  /** `SetNextTarget()` (VER >= 11): the queued next target becomes the focus. Auto-attack ends either way. */
  setNextTarget(): void {
    const id = this.nextTarget;
    if (id !== null) {
      const guildWarDead =
        this.kinds.guildWarMap?.() === true && (this.kinds.isDead?.(id) ?? false);
      const hidden = this.kinds.isTransparent?.(id) ?? false;
      if (guildWarDead || hidden) return;
      this.setFocus(id);
      this.nextTarget = null;
      if (!(this.kinds.isPeaceful?.(id) ?? false)) this.renderTarget = this.focus;
    }
    this.autoAttack = false;
  }

  /**
   * `MoverDeath` (the own part of 17 §11.1 and `OnMoverDeath`): the own death empties the focus; a kill by the own player
   * empties it unless `features.targetsave` is on and the victim was not the focus (an area kill keeps the target), and a queued
   * next target takes over; a dead next target is dropped.
   */
  onMoverDeath(victim: number, attacker: number): void {
    const self = this.kinds.selfId?.() ?? 0;
    if (self !== 0 && victim === self) {
      this.setFocus(null);
      this.lastTarget = null;
    } else if (self !== 0 && attacker === self) {
      const saved = this.nextTarget;
      if (this.kinds.targetSave?.() === true) {
        if (victim === this.focus) this.setFocus(null);
      } else {
        this.setFocus(null);
      }
      this.lastTarget = null;
      if (saved !== null) {
        this.nextTarget = saved;
        this.setNextTarget();
      }
    }
    if (victim === this.nextTarget) {
      if (this.nextTarget === this.renderTarget) {
        this.nextTarget = null;
        this.renderTarget = this.focus;
      } else {
        this.nextTarget = null;
      }
    }
  }

  /** `OnKeyDown` of W/A/S/D: a movement key ends auto-attack at once (§7.5). */
  onMovementKeyDown(): void {
    this.autoAttack = false;
  }

  /** The world is left: nothing stays selected. */
  reset(): void {
    this.focus = null;
    this.focusOld = null;
    this.nextTarget = null;
    this.renderTarget = null;
    this.selectRenderObj = null;
    this.selectTarget = false;
    this.autoAttack = false;
    this.nextSkill = NEXTSKILL_NONE;
    this.tFocusAttack = Number.NEGATIVE_INFINITY;
    this.lastTarget = null;
  }

  snapshot(): Record<string, unknown> {
    return {
      focus: this.focus,
      focusOld: this.focusOld,
      nextTarget: this.nextTarget,
      renderTarget: this.renderTarget,
      selectRenderObj: this.selectRenderObj,
      selectTarget: this.selectTarget,
      autoAttack: this.autoAttack,
      nextSkill: this.nextSkill,
      lastTarget: this.lastTarget,
      sent: this.sent.slice(-10),
    };
  }
}
