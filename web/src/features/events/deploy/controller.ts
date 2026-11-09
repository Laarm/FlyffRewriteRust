// The placement controller (`CDeployManager`, `CGuildDeployManager`, `src:_Common/Housing.cpp:685-1390`, `Neuz.cpp:1384-1447`, `WndWorld.cpp:7014-7046, 11799-11830`; Plan/45 §28.10, Plan/43 §7.13; WP-CFEAT-46, WP-INPUT-17): ONE
// furniture follows the pointer as a translucent ghost until the click confirms it or Esc ends it. Two managers of the original differ in details that the controller keeps apart (`kind`):
//
//                       mini room                                  guild house
//   start angle         0, the key accumulator resets to 0         0 for a SETUP, the angle of the row for a RESET; the key accumulator is KEPT across placements
//   extra collision     none                                       the height attribute (NOWALK / NOFLY / NOMOVE are red)
//   confirm sends       `HousingSetupFurnitureReq{ setup }`        `GuildHousePacketReq{ SETUP 2 | RESET 4, slot, info }`, then `selectedObjId = null`, `mode = 0`
//
// The controller is a state machine with its dependencies passed in (`DeployDeps`), so it is tested without a renderer: without `render()` it runs with `collide` false. The renderer is read at the moment of use.

import { TID_GAME_HOUSING_INSTALL_FAIL01 } from '../../../gen/defs/text';
import type { DeployRenderApi, DeployScene } from '../../../game/ports';
import type { Vec3LH } from '../../../render/types';
import type { DeployFeature, DeployKind, DeployState } from './api';
import { collides } from './collide';

/** `obj_miniwall01.o3d`: the wall object of the small house and of the mini room. */
export const WALL_FILE = 'obj_miniwall01.o3d';
/** `NULL_ID`. */
export const NULL_OBJ = 0xffff_ffff;
/** The wheel turns the ghost 2.5 degrees per notch. */
export const WHEEL_STEP = 2.5;
/** PgUp / PgDn turn 45 degrees. */
export const KEY_STEP = 45;

/** A row of the guild house as the controller needs it. */
export interface DeployRow {
  readonly itemId: number;
  readonly pos: Vec3LH;
  readonly angle: number;
  readonly ctrlObjId: number;
  readonly expireAt: number;
}

export interface DeployDeps {
  /** One client line in the colour of its text id (`TID_GAME_HOUSING_INSTALL_FAIL01`). */
  chat(tid: number): void;
  /** The ghost renderer, read at the moment of use. */
  render(): DeployRenderApi | undefined;
  /** `GetHeightAttribute( x, z )`: `undefined` = no query slot this iteration. */
  heightAttr(x: number, z: number): number | undefined;
  /** `prop.link_kind` (the ctrl model) of an item, `undefined` for an unknown item. */
  linkKind(itemId: number): number | undefined;
  /** The mini room: `HousingSetupFurnitureReq{ setup: true }`. */
  placeHousing(itemId: number, pos: Vec3LH, angle: number): void;
  /** The guild house. */
  guild: {
    row(slot: number): DeployRow | null;
    /** `GuildHouse->m_dwSelectedObjID`. */
    selectedObjId(): number | null;
    /** `GuildHousePacketReq{ SETUP | RESET, slot, info }`. */
    send(op: 2 | 4, slot: number, info: DeployRow & { setup: true }): void;
    select(objId: number | null): void;
    setMode(mode: 0 | 2 | 4): void;
  };
  log?(message: string): void;
}

/** The record of a request the controller sent (the probe and the tests read them). */
export type DeploySent =
  | {
      readonly kind: 'housing';
      readonly itemId: number;
      readonly pos: Vec3LH;
      readonly angle: number;
    }
  | {
      readonly kind: 'guild';
      readonly op: 2 | 4;
      readonly slot: number;
      readonly itemId: number;
      readonly pos: Vec3LH;
      readonly angle: number;
      readonly ctrlObjId: number;
    };

export interface DeployController extends DeployFeature {
  /** The requests sent so far, newest last (at most 20). */
  sent(): readonly DeploySent[];
  /** Test hook: the ghost stands at `p` until the pointer picks another point. */
  moveTo(p: Vec3LH): void;
  /** Test hook: the confirming click. */
  click(): void;
  /** The key accumulator of the guild manager (kept across placements). */
  keyAngle(): number;
  /** The collision scene of the renderer is known (the ghost model, the room wall): until then `collide` is false whatever the pose. */
  sceneReady(): boolean;
}

const same3 = (a: Vec3LH, b: Vec3LH): boolean => a[0] === b[0] && a[1] === b[1] && a[2] === b[2];

export function createDeployController(deps: DeployDeps): DeployController {
  let st: DeployState | null = null;
  /** The guild manager keeps its key accumulator across placements (`Init()` does not reset `m_fAngle`). */
  let guildKeyAngle = 0;
  let scene: DeployScene | null = null;
  let heightAttr: number | undefined;
  /** The pose the last collision was computed for. */
  let computed: { pos: Vec3LH; angle: number } | null = null;
  /** `moveTo`: the ghost stands at `pos` while the pointer's pick stays at `base` (the first pick after it, `null` until there is one). */
  let override: { base: Vec3LH | null } | null = null;
  const sent: DeploySent[] = [];

  const note = (s: DeploySent): void => {
    sent.push(s);
    if (sent.length > 20) sent.shift();
  };

  const update = (patch: Partial<DeployState>): void => {
    if (st) st = { ...st, ...patch };
  };

  /** The collision of the current pose; recomputed only when the pose or the scene changed. */
  const recompute = (force = false): void => {
    if (!st) return;
    const render = deps.render();
    const pose = { pos: st.pos, angle: st.angle };
    if (
      !force &&
      computed &&
      same3(computed.pos, pose.pos) &&
      computed.angle === pose.angle &&
      scene !== null
    ) {
      return;
    }
    computed = pose;
    // the scene is read while the model loads (`null`), then once per pose (the other ctrls may have changed)
    scene = render ? render.scene(WALL_FILE, st.exceptObjId) : null;
    let attr: number | undefined;
    if (st.kind === 'guild') {
      attr = deps.heightAttr(st.pos[0], st.pos[2]);
      // no query slot this iteration: the last answer stays
      if (attr !== undefined) heightAttr = attr;
    }
    const collide = collides({
      scene,
      pos: st.pos,
      angle: st.angle,
      exceptObjId: st.exceptObjId,
      guild: st.kind === 'guild',
      heightAttr: st.kind === 'guild' ? heightAttr : undefined,
    });
    update({ collide });
    render?.pose(st.pos, st.angle, collide);
  };

  const finish = (): void => {
    deps.render()?.hide();
    st = null;
    scene = null;
    computed = null;
    override = null;
    heightAttr = undefined;
  };

  const begin = (
    kind: DeployKind,
    mode: 0 | 2 | 4,
    itemId: number,
    ctrlId: number,
    slot: number,
    angle: number,
    exceptObjId: number,
    pos: Vec3LH,
  ): boolean => {
    // a placement that runs ends first, nothing is sent
    if (st) cancel();
    const render = deps.render();
    render?.show(ctrlId);
    st = {
      kind,
      mode,
      itemId,
      ctrlId,
      slot,
      exceptObjId,
      pos,
      angle,
      keyAngle: kind === 'guild' ? guildKeyAngle : 0,
      collide: false,
    };
    scene = null;
    computed = null;
    override = null;
    heightAttr = undefined;
    if (kind === 'guild') deps.guild.setMode(mode);
    return true;
  };

  const cancel = (): boolean => {
    if (!st) return false;
    const wasGuild = st.kind === 'guild';
    finish();
    if (wasGuild) deps.guild.setMode(0);
    return true;
  };

  const ctrlOf = (itemId: number): number | undefined => {
    const link = deps.linkKind(itemId);
    return link === undefined || link === NULL_OBJ ? undefined : link;
  };

  const controller: DeployController = {
    active: () => st !== null,
    state: () => st,
    sent: () => sent,
    keyAngle: () => st?.keyAngle ?? guildKeyAngle,
    sceneReady: () => scene !== null,

    beginHousing(itemId) {
      const ctrlId = ctrlOf(itemId);
      if (ctrlId === undefined) {
        deps.log?.(`deploy: item ${String(itemId)} has no ctrl model`);
        return false;
      }
      return begin('housing', 0, itemId, ctrlId, -1, 0, NULL_OBJ, [0, 0, 0]);
    },

    beginGuild(slot, mode) {
      const row = deps.guild.row(slot);
      if (!row) return false;
      const ctrlId = ctrlOf(row.itemId);
      if (ctrlId === undefined) {
        deps.log?.(`deploy: item ${String(row.itemId)} has no ctrl model`);
        return false;
      }
      const render = deps.render();
      // `AfxMessageBox( "cannot find house object" )`: no wall object, no placement
      if (render && !render.wallPresent(WALL_FILE)) {
        deps.log?.('deploy: cannot find house object');
        return false;
      }
      const selected = deps.guild.selectedObjId();
      return begin(
        'guild',
        mode,
        row.itemId,
        ctrlId,
        slot,
        mode === 4 ? row.angle : 0,
        selected ?? NULL_OBJ,
        mode === 4 ? row.pos : [0, 0, 0],
      );
    },

    cursor(p) {
      if (!st) return;
      if (p !== null) {
        if (override !== null) {
          // the pick of an unmoved pointer does not undo `moveTo`: the first pick after it is the base, a different one means the pointer moved
          if (override.base === null) {
            override.base = p;
            return;
          }
          if (same3(override.base, p)) return;
          override = null;
        }
        if (!same3(st.pos, p)) update({ pos: p });
      }
      recompute();
    },

    wheel(notches) {
      if (!st || notches === 0) return;
      update({ angle: st.angle + WHEEL_STEP * notches });
      recompute();
    },

    pageKey(code) {
      if (!st) return;
      let k = st.keyAngle;
      if (code === 'PageDown') {
        k += KEY_STEP;
        if (k > 359) k = 0;
      } else {
        k -= KEY_STEP;
        if (k < -359) k = 0;
      }
      if (st.kind === 'guild') guildKeyAngle = k;
      // the keys overwrite a rotation done with the wheel (`SetAngle( m_fAngle )`)
      update({ keyAngle: k, angle: k });
      recompute();
    },

    confirm() {
      if (!st) return;
      // the pose may have changed since the last tick (a click right after a wheel notch)
      recompute();
      if (st.collide) {
        deps.chat(TID_GAME_HOUSING_INSTALL_FAIL01);
        return;
      }
      const s = st;
      finish();
      if (s.kind === 'housing') {
        deps.placeHousing(s.itemId, s.pos, s.angle);
        note({ kind: 'housing', itemId: s.itemId, pos: s.pos, angle: s.angle });
        return;
      }
      const row = deps.guild.row(s.slot);
      const op: 2 | 4 = s.mode === 4 ? 4 : 2;
      if (row) {
        const info = {
          itemId: s.itemId,
          pos: s.pos,
          angle: s.angle,
          // a SETUP carries the object id and the time of the row, a RESET the object id of the piece that moves
          ctrlObjId: op === 4 ? s.exceptObjId : row.ctrlObjId,
          expireAt: row.expireAt,
          setup: true as const,
        };
        deps.guild.send(op, s.slot, info);
        note({
          kind: 'guild',
          op,
          slot: s.slot,
          itemId: s.itemId,
          pos: s.pos,
          angle: s.angle,
          ctrlObjId: info.ctrlObjId,
        });
      }
      deps.guild.select(null);
      deps.guild.setMode(0);
    },

    cancel,

    moveTo(p) {
      if (!st) return;
      override = { base: null };
      update({ pos: p });
      recompute();
    },

    click() {
      controller.confirm();
    },
  };
  return controller;
}
