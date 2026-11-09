// `motions.execute(id)` (Plan/45 §19.4; `CWndMgr::ObjectExecutor` SHORTCUT_MOTION, `src:_Interface/WndManager.cpp:3001-3175`,
// `CMover::DoPickupItemAround` `src:_Common/MoverSkill.cpp:3523-3558`; WP-CFEAT-51): the pure decision of one motion shortcut. The
// module (`motionsModule.ts`) implements `MotionEnv` over the client; the rows ESCAPE, CHEER and COUPLE are registered by the
// features that own them (WP-CFEAT-14 / WP-CFEAT-40) and do nothing before.

import {
  MOT_BASE_AUTOATTACK,
  MOT_BASE_BATTLE,
  MOT_BASE_COLLECT,
  MOT_BASE_COUPLE,
  MOT_BASE_CHEER,
  MOT_BASE_ESCAPE,
  MOT_BASE_PICKUP,
  MOT_BASE_SIT,
  MTI_POINTWAYEAST,
  MTI_POINTWAYNORTH,
  MTI_POINTWAYSOUTH,
  MTI_POINTWAYWEST,
} from '../../gen/defs/neuz';
import { OBJMSG, STA } from '../../game/combat/constants';
import type { MotionProp } from '../../gen/bundles';
import { TID_GAME_NOTTARGET } from '../../gen/defs/text';

/** `ANILOOP_1PLAY`, `ANILOOP_CONT`, `ANILOOP_LOOP`. */
export const ANILOOP = { PLAY1: 1, CONT: 2, LOOP: 4 } as const;

/** The pickup radius: items within 4 m (`fDistSq < 4.0f * 4.0f`). */
export const PICKUP_RANGE = 4;

/** `prop.play`: 0 plays once, 1 holds the last frame, 2 loops; anything else plays once. */
export function animLoopOf(play: number): number {
  switch (play) {
    case 1:
      return ANILOOP.CONT;
    case 2:
      return ANILOOP.LOOP;
    default:
      return ANILOOP.PLAY1;
  }
}

/** The facing a point-way motion sets after it started: north 180, south 0, east 90, west 270 degrees. */
export function pointWayAngle(motion: number): number | null {
  switch (motion) {
    case MTI_POINTWAYNORTH:
      return 180;
    case MTI_POINTWAYSOUTH:
      return 0;
    case MTI_POINTWAYEAST:
      return 90;
    case MTI_POINTWAYWEST:
      return 270;
    default:
      return null;
  }
}

/** The row of a motion as the shortcut needs it. */
export type MotionRow = Pick<MotionProp, 'id' | 'motion' | 'play' | 'name' | 'icon' | 'desc'>;

/** What the executor reads of the own mover. */
export interface MotionSelf {
  readonly dead: boolean;
  /** `OBJSTA_*` state word. */
  readonly state: number;
  readonly sitting: boolean;
  /** `OBJSTAF_COMBAT`. */
  readonly combat: boolean;
}

/** An object near the player: the nearest ground item within the pickup radius is picked. */
export interface NearbyItem {
  readonly id: number;
  /** The squared 3-D distance to the player (`D3DXVec3LengthSq`). */
  readonly distSq: number;
}

/** Everything the executor needs from the client. */
export interface MotionEnv {
  /** `GetWndVendorBase()`: a vendor window (own shop or a browsed shop) is open. */
  vendorOpen(): boolean;
  row(id: number): MotionRow | undefined;
  /** Stores the motion name as the pending chat text and (re)starts its 1000 ms timer. */
  queueChat(text: string): void;
  self(): MotionSelf | null;
  /** `m_dwReqFlag & REQ_USESKILL`. */
  skillPending(): boolean;
  /** `SendMotion(OBJMSG_SITDOWN | OBJMSG_STANDUP)`. */
  sendMotion(msg: number): void;
  /**
   * `SendActMsg` on the own mover; `then(result)` runs when the sim answered (a busy query slot defers it to the next
   * iteration). `1` = the message started.
   */
  act(
    msg: number,
    p: readonly [number, number, number, number, number],
    then: (result: number) => void,
  ): void;
  /** The ground items within the pickup radius, with their squared distances. */
  itemsAround(): readonly NearbyItem[];
  /** `CMD_SetUseItem(id)`: walk to the item; the server picks it up on arrival. */
  useItem(id: number): void;
  clearDest(): void;
  /** `SetAngle(deg)`. */
  face(deg: number): void;
  /** `SendPlayerBehavior()`. */
  report(): void;
  focusId(): number | null;
  /** `targeting.attackable(id)` with its side effects. */
  attackable(id: number): boolean;
  /** The focus object is a mover (not a ground item or a ctrl). */
  isMover(id: number): boolean;
  setAutoAttack(on: boolean): void;
  /** The active hand holds a wand. */
  wandInHand(): boolean;
  /** `features.wand_autoattacks`. */
  wandAutoattacks(): boolean;
  magicAttack(id: number, power: number): void;
  /** `PutString(GETTEXT(tid))`. */
  say(tid: number): void;
}

/** A row registered by a later feature (`motions.register`). */
export type MotionHandler = (env: MotionEnv) => void;

export class MotionExecutor {
  readonly #handlers = new Map<number, MotionHandler>();
  /** The last executed id (the probe reads it). */
  last: { id: number; action: string } | null = null;

  constructor(private readonly env: MotionEnv) {}

  /** Registers the row of `MOT_BASE_ESCAPE` (14), `CHEER` and `COUPLE` (40); a later registration replaces an earlier one. */
  register(id: number, handler: MotionHandler): void {
    this.#handlers.set(id, handler);
  }

  /** `motions.execute(id)`: steps 0-3 of 45 §19.4. */
  execute(id: number): void {
    const e = this.env;
    // 0. a vendor window is open: nothing, not even the chat
    if (e.vendorOpen()) {
      this.last = { id, action: 'vendor' };
      return;
    }
    const row = e.row(id);
    if (!row) {
      this.last = { id, action: 'unknown' };
      return;
    }
    // 1. the name goes out as a chat line one second later, whatever step 2 does
    if (row.name !== '') e.queueChat(row.name);
    // 2.
    switch (id) {
      case MOT_BASE_PICKUP:
        this.#pickup();
        return;
      case MOT_BASE_BATTLE:
        this.#battle();
        return;
      case MOT_BASE_SIT:
        this.#sit();
        return;
      case MOT_BASE_COLLECT:
        this.last = { id, action: 'collect' }; // the body is commented out in the original
        return;
      case MOT_BASE_AUTOATTACK:
        this.#autoAttack();
        return;
      case MOT_BASE_ESCAPE:
      case MOT_BASE_CHEER:
      case MOT_BASE_COUPLE: {
        const h = this.#handlers.get(id);
        this.last = { id, action: h ? 'registered' : 'unregistered' };
        h?.(e);
        return;
      }
      default:
        this.#emote(row);
    }
  }

  /** `DoPickupItemAround`: the nearest ground item within 4 m, not while attacking or dead. */
  #pickup(): void {
    const e = this.env;
    const me = e.self();
    this.last = { id: MOT_BASE_PICKUP, action: 'none' };
    if (!me || (me.state & STA.ATK_ALL) !== 0 || me.dead) return;
    let best: NearbyItem | null = null;
    for (const it of e.itemsAround()) {
      if (it.distSq < PICKUP_RANGE * PICKUP_RANGE && (best === null || it.distSq < best.distSq))
        best = it;
    }
    if (best) {
      e.useItem(best.id);
      this.last = { id: MOT_BASE_PICKUP, action: 'pickup' };
    }
  }

  /** The combat mode toggles; a started change is a movement report. */
  #battle(): void {
    const e = this.env;
    const me = e.self();
    if (!me) return;
    const msg = me.combat ? OBJMSG.MODE_PEACE : OBJMSG.MODE_COMBAT;
    this.last = { id: MOT_BASE_BATTLE, action: me.combat ? 'peace' : 'combat' };
    e.act(msg, [0, 0, 0, 0, 0], (result) => {
      if (result === 1) this.#behavior();
    });
  }

  /** Sit down or stand up: not jumping, attacking, damaged, in an action or with a skill request pending. */
  #sit(): void {
    const e = this.env;
    const me = e.self();
    this.last = { id: MOT_BASE_SIT, action: 'refused' };
    if (!me) return;
    const busy =
      (me.state & (STA.JUMP_ALL | STA.ATK_ALL | STA.DMG_ALL | STA.ACTION_ALL)) !== 0 ||
      e.skillPending();
    if (busy) return;
    e.sendMotion(me.sitting ? OBJMSG.STANDUP : OBJMSG.SITDOWN);
    this.last = { id: MOT_BASE_SIT, action: me.sitting ? 'standup' : 'sitdown' };
  }

  /** The auto-attack toggle of the emoticon: a wand without `wand_autoattacks` fires a single bolt instead. */
  #autoAttack(): void {
    const e = this.env;
    this.last = { id: MOT_BASE_AUTOATTACK, action: 'none' };
    const focus = e.focusId();
    if (focus === null || !e.attackable(focus)) return;
    if (e.isMover(focus)) {
      if (e.wandInHand() && !e.wandAutoattacks()) {
        e.magicAttack(focus, 0);
        this.last = { id: MOT_BASE_AUTOATTACK, action: 'wand' };
      } else {
        e.setAutoAttack(true);
        this.last = { id: MOT_BASE_AUTOATTACK, action: 'auto' };
      }
    } else {
      e.say(TID_GAME_NOTTARGET);
      this.last = { id: MOT_BASE_AUTOATTACK, action: 'nottarget' };
    }
  }

  /** The emotes: `local_act(OBJMSG_MOTION, motion, loop)`; a started one turns point-way motions and is reported. */
  #emote(row: MotionRow): void {
    const e = this.env;
    this.last = { id: row.id, action: 'emote' };
    e.act(OBJMSG.MOTION, [row.motion, animLoopOf(row.play), 0, 0, 0], (result) => {
      if (result !== 1) return;
      const angle = pointWayAngle(row.motion);
      if (angle !== null) e.face(angle);
      this.#behavior();
    });
  }

  /** `ClearDest()` and `SendPlayerBehavior()`. */
  #behavior(): void {
    this.env.clearDest();
    this.env.report();
  }
}
