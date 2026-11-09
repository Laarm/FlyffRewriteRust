// The world click handlers (Plan/43 §4.4-§4.8, §6.6, §6.7, §7.14; WP-INPUT-07): `CWndWorld::OnLButtonDown/Up/DblClk`,
// `OnRButtonDown/Up/DblClk`, `OnMButtonDown/Up` and `OnMouseMove`, with the M2 subset of the selection logic: local focus (no
// `SetTargetReq` yet), the L+R chord rules (KQ-31: the 200 ms selection rule here, the jump latched through
// `WorldTick.noteChord` at each press, owner #002), click-to-move through `groundControl`, Z + click follow, the mover menu
// with the 20 m approach rule and the camera drag. The charged release of a bow or wand shot (§4.5) and the auto-attack rules of
// the clicks are WP-INPUT-09..11; the vendor, buff-icon and placement clicks are later work packages (`InputHooks` seams).

import {
  APP_MINIGAME_DICE,
  APP_MINIGAME_KAWIBAWIBO,
  APP_RR_MINIGAME_ARITHMETIC,
  APP_RR_MINIGAME_CARD,
  APP_RR_MINIGAME_DICE,
  APP_RR_MINIGAME_KAWIBAWIBO,
  APP_RR_MINIGAME_LADDER,
  APP_RR_MINIGAME_STOPWATCH,
  APP_RR_MINIGAME_TYPING,
  APP_SMELT_JEWEL,
} from '../gen/defs/resdata';
import { create } from '@bufbuild/protobuf';
import { QueryPVendorItemSchema } from '../gen/proto/flyff/v1/trade_pb';
import { buffIconClick, buffIconDoubleClick } from './buffIcons';
import { LR_SUPPRESS_MS, type ButtonEvent, type Point, type WorldHandlers } from './pointer';
import type { InputCore, WorldTick } from './worldTick';
import { MODE_DONMOVE } from './worldTick';
import type { Picker } from './picker';
import type { ClientMsg } from '../net/codec';
import type { ObjectInfo, SelfInfo } from './ports';
import { IK3_BOW, IK3_WAND } from '../gen/defs/itemkind';
import { modifierGateOpen } from './keymap';
import { FOCUS_ATTACK_MS, NEXTSKILL_NONE } from './targeting';

/** `BLOCKING_MINIGAMES_AND_SMELT` (§4.4): windows that stop the world clicks and show `CUR_NO`. */
export const BLOCKING_MINIGAMES_AND_SMELT: readonly number[] = [
  APP_MINIGAME_KAWIBAWIBO,
  APP_MINIGAME_DICE,
  APP_RR_MINIGAME_KAWIBAWIBO,
  APP_RR_MINIGAME_DICE,
  APP_RR_MINIGAME_ARITHMETIC,
  APP_RR_MINIGAME_STOPWATCH,
  APP_RR_MINIGAME_TYPING,
  APP_RR_MINIGAME_CARD,
  APP_RR_MINIGAME_LADDER,
  APP_SMELT_JEWEL,
];

/**
 * The personal-shop click reaches this far from the camera (`m_fDistCamera`): with the zoom limit on 30 m (owner #019: the original's
 * 20 m x 1.5, tied to the zoom limit `WndWorld.cpp:7218-7224`, which went from 18 m to 27 m), else 40 m.
 */
export const VENDOR_CLICK_RANGE_LIMITED = 30;
export const VENDOR_CLICK_RANGE = 40;
/** The mover menu opens within this many metres of the target (`__IMPROVE_SYSTEM_VER15`, `ShowMoverMenu`). */
export const MOVER_MENU_RANGE = 20;
/** `AUTH_GAMEMASTER` ('L'). */
const AUTH_GAMEMASTER = 76;

const round = (n: number): number => Math.round(n);

export class WorldClicks implements WorldHandlers {
  constructor(
    private readonly c: InputCore,
    private readonly tick: WorldTick,
    private readonly picker: Picker,
  ) {}

  /** An own vendor, a vendor window or a blocking mini-game window is open (`CUR_NO`, no menus). */
  worldBlocked(): boolean {
    const w = this.c.deps.windows;
    const s = this.c.deps.self();
    return (
      (s?.ownVendorOpen ?? false) ||
      w.isVendorWindowOpen() ||
      w.anyOpen(BLOCKING_MINIGAMES_AND_SMELT)
    );
  }

  private canAct(): boolean {
    const s = this.c.deps.self();
    if (s === null || s.dead || s.ownVendorOpen) return false;
    const w = this.c.deps.windows;
    if (w.isVendorWindowOpen()) return false;
    if ((s.modeFlags & MODE_DONMOVE) !== 0 || s.settingStone) return false;
    return !w.anyOpen(BLOCKING_MINIGAMES_AND_SMELT);
  }

  /** `menuAllowed()` (`WndWorld.cpp:11716-11760`). */
  private menuAllowed(): boolean {
    const hooks = this.c.deps.hooks;
    if (hooks?.worldMap?.shown() === true) return false;
    return this.canAct();
  }

  // ---- WorldHandlers -------------------------------------------------------------------------------------------------

  down(e: ButtonEvent, dbl: boolean): void {
    this.c.mouse.ptOld = e.p; // a press happens where the pointer is: the next drag delta starts here
    if (e.button === 0) {
      if (dbl) this.lButtonDblClk(e);
      else this.lButtonDown(e);
    } else if (e.button === 2) {
      if (dbl) this.rButtonDblClk();
      else this.rButtonDown(e);
    }
  }

  up(e: ButtonEvent): void {
    if (e.button === 0) this.lButtonUp(e);
    else if (e.button === 2) this.rButtonUp(e);
    else this.mButtonUp();
  }

  move(p: Point, _mods?: unknown, movement?: Point): void {
    const c = this.c;
    const m = c.mouse;
    const self = c.deps.self();
    if (self === null) return;
    // under Pointer Lock the pointer position is frozen and `movementX/Y` carries the motion
    const dx = movement ? movement.x : p.x - m.ptOld.x;
    const dy = movement ? movement.y : p.y - m.ptOld.y;
    // flight steering (§4.8): mode 0 with R held, or mode 1
    if (self.flying && c.flight.steering()) {
      m.steerDX += dx;
      m.steerDY += dy;
    }
    // camera drag: the right button on the ground, the middle button while flying (§8.5, Q18)
    if (self.flying ? m.M.down : m.R.down) {
      m.dragDX += dx;
      m.dragDY += dy;
    }
    m.ptOld = p;
    m.pos = p;
  }

  wheel(notches: number): void {
    // ---- housing / guild house (ui-house-quiz): the wheel turns the furniture being placed instead of zooming (`Neuz.cpp:1470-1485`)
    if (this.tick.deploy.wheel(notches)) return;
    this.c.mouse.wheelNotches += notches;
  }

  lostCapture(): void {
    // alt-tab, devtools: the buttons are up, the up handlers' game logic does NOT run (§4.1)
    this.c.mouse.releaseAll();
  }

  // ---- left button ---------------------------------------------------------------------------------------------------

  /** `OnLButtonDown` (§4.4). */
  private lButtonDown(e: ButtonEvent): void {
    const c = this.c;
    const m = c.mouse;
    const t = c.targeting;
    const hooks = c.deps.hooks;
    if (hooks?.worldMap?.shown() === true) {
      hooks.worldMap.click?.(e.p.x, e.p.y);
      return;
    }
    c.followId = null;
    const self = c.deps.self();
    if (self === null || !this.canAct()) return;
    // a HUD buff icon (§4.4, §7.13): the double-click icons wait, the angel icons send `AngelBuff`; either way no selection
    const send = (msg: ClientMsg): void => c.deps.send?.(msg);
    if (buffIconClick(hooks?.buffIconAt?.(e.p.x, e.p.y) ?? null, send)) return;
    m.tLDown = e.t;
    this.tick.noteChord(e.t); // R already down: the chord jumps (owner #002), also when the 200 ms rule below drops the click
    if (e.t - m.tRDown < LR_SUPPRESS_MS) return; // L+R chord: no selection (KQ-31)
    // ---- housing / guild house (ui-house-quiz): a furniture follows the pointer - the click confirms it (`WndWorld.cpp:7014-7046`): no selection, no destination
    if (this.tick.deploy.press()) return;
    const old = t.focus;
    const picked = this.picker.pickAt(e.p.x, e.p.y);
    let sel = picked;
    if (t.nextTarget === null && old === null) t.autoAttack = false;
    c.commands.clearCmd();
    if (sel !== null && sel.kind === 'mover') {
      if (m.R.down || m.mouseMode === 1) {
        t.autoAttack = false;
        sel = null;
      } else if (self.flying) {
        // flight (owner #011, deviation from `WndWorld.cpp:7073-7092`, where only Tab changed the focus): a click focuses any
        // non-player mover like on foot (a ground or flying monster, an NPC) so that it can be targeted; players stay unselectable
        // (original). Attacking stays limited to flying monsters: `attackable.ts` (the client) and `hit_type.rs` (the server).
        t.autoAttack = false;
        t.nextTarget = null;
        if (c.deps.entityInfo(picked?.id ?? -1)?.isPlayer === true) {
          sel = null;
        } else {
          sel = this.picker.selectPicked(picked);
          const si = sel === null ? null : c.deps.entityInfo(sel.id);
          if (sel !== null && si !== null && !si.peaceful) t.renderTarget = sel.id;
        }
      } else {
        sel = this.picker.selectPicked(picked);
        const si = sel === null ? null : c.deps.entityInfo(sel.id);
        if (sel !== null && si !== null && (si.isPlayer || !si.peaceful)) t.renderTarget = sel.id;
        const id = sel?.id ?? null;
        if (old !== null && old === id) {
          // the second click on the current target
          t.autoAttack = false;
          t.nextTarget = null;
          if (c.deps.options().autoAttack) t.autoAttack = true; // wands too (`__WAND_AUTOATTACKS`)
        } else if (old !== null && t.autoAttack && old !== id) {
          t.nextTarget = id; // queue the next target
        } else {
          t.autoAttack = false;
        }
      }
    } else {
      sel = this.picker.selectPicked(picked);
      t.autoAttack = false;
      t.nextTarget = null;
      t.renderTarget = null;
    }
    t.nextSkill = NEXTSKILL_NONE; // a left click cancels a reserved skill
    c.skill.onLeftClick(); // and ends a running action slot (`EndSkillQueueReq`, §7.12)
    c.commands.clearSkillFlag();
    const selInfo = sel === null ? null : c.deps.entityInfo(sel.id);
    if (sel === null) {
      m.freeMove = true; // the next control step picks the ground
      if (self.flying) {
        m.flyMove = true;
        c.flight.accEdgeReset(); // `s_bAccKeyed = FALSE` (WndWorld.cpp:7202)
      }
    } else if (sel.kind === 'mover') {
      if (this.vendorClick(e, self, sel.id, selInfo)) {
        // the open shop of another player (§7.13): `QueryPVendorItem` (or nothing: Ctrl outside a safe region), no follow, no menu
      } else if (
        selInfo?.isPlayer === true &&
        sel.id !== self.id &&
        c.keys.isHeld('KeyZ') &&
        modifierGateOpen(e.mods)
      ) {
        // Z + click on another player: follow
        if (!self.flying) {
          c.followId = sel.id;
          c.commands.setDestObj(sel.id, 0);
        }
      } else {
        c.deps.hooks?.playIdleSound?.(sel.id);
        if (selInfo?.peaceful === true && !self.flying && (!selInfo.isPlayer || e.mods.alt)) {
          const gc = hooks?.gc1to1;
          if (gc?.possible(self.id) === true && gc.possible(sel.id)) {
            if (!selInfo.isPlayer) this.openMoverMenu(sel.id); // 1:1 arena: no player menus
          } else {
            t.autoAttack = false;
            this.openMoverMenu(sel.id);
          }
        }
      }
      t.selectTarget = true;
      if (sel.id !== old) t.tFocusAttack = e.t;
      c.charge.restart(e.t); // the gauge starts with the click
    } else if (sel.kind === 'item') {
      t.selectTarget = true;
    } else {
      t.selectTarget = true;
      hooks?.showCtrlMenu?.(sel.id);
    }
    if (self.flying) c.flight.onClick(self); // `ControlPlayer(WM_LBUTTONDOWN)`: the flying attack of the click (§9.5)
    this.tick.runImmediate(e.t);
  }

  /** `OnLButtonUp` (§4.5). */
  private lButtonUp(e: ButtonEvent): void {
    const c = this.c;
    const self = c.deps.self();
    if (self === null || self.dead) return;
    c.mouse.flyMove = false;
    // the charged release of a bow or wand shot: the held click on a mover that was selected at least 500 ms ago (§4.5)
    const t = c.targeting;
    const focus = t.focus;
    if (
      focus !== null &&
      c.deps.entityInfo(focus)?.kind === 'mover' &&
      t.selectTarget &&
      e.t - t.tFocusAttack >= FOCUS_ATTACK_MS &&
      c.attack.attackable(focus)
    ) {
      const power = c.charge.power(e.t);
      if (power >= 0) {
        c.charge.restart(e.t);
        const kind3 = c.deps.hand?.()?.kind3;
        if (kind3 === IK3_WAND) c.commands.magicAttack(focus, power);
        else if (kind3 === IK3_BOW) c.commands.rangeAttack(focus, power);
      }
    }
    // ---- housing / guild house (ui-house-quiz): no ground control while a furniture is placed (`WndWorld.cpp:6709-6713`)
    if (!this.tick.deploy.active()) this.tick.runImmediate(e.t);
    c.targeting.selectTarget = false;
  }

  /** `OnLButtonDblClk` (§4.5). */
  private lButtonDblClk(e: ButtonEvent): void {
    const c = this.c;
    c.followId = null;
    this.tick.noteChord(e.t); // R held: a second L press that Windows reports as a double click is still a chord press (owner #002)
    // ---- housing / guild house (ui-house-quiz): no ground control while a furniture is placed (`WndWorld.cpp:7449-7453`)
    if (!this.tick.deploy.active()) this.tick.runImmediate(e.t);
    const t = c.targeting;
    const hl = this.picker.highlight;
    // regardless of the option (KEEP); a wand does not take the double click (it charges). Not while the own mover flies (owner #011,
    // small deviation from `WndWorld.cpp:7447-7467`, which has no IsFly guard): a click in flight focuses a ground monster that the
    // flying attack refuses, and the auto-attack would start right after landing
    if (
      t.focus !== null &&
      hl?.id === t.focus &&
      c.deps.hand?.()?.kind3 !== IK3_WAND &&
      c.deps.self()?.flying !== true
    ) {
      t.autoAttack = true;
    }
    // the HUD buff icons: return scroll, party summon, pet feed pocket (§7.13)
    const hooks = c.deps.hooks;
    buffIconDoubleClick(
      hooks?.buffIconAt?.(e.p.x, e.p.y) ?? null,
      (msg) => c.deps.send?.(msg),
      () => hooks?.partySummon?.(),
    );
  }

  /**
   * The personal-shop click of §4.4 / §7.13 (`WndWorld.cpp:7225-7255`): another player with an open shop, within the camera distance limit
   * (30 m with the zoom limit, 40 m without; owner #019), while the own player does not look into a shop, without Alt. In a safe region the click always
   * asks; elsewhere only without Ctrl (Ctrl is the "attack" modifier there). `true` = the click belongs to the shop branch, also when Ctrl
   * suppressed the message (the original's `if` chain stops there).
   */
  private vendorClick(
    e: ButtonEvent,
    self: SelfInfo,
    id: number,
    info: ObjectInfo | null,
  ): boolean {
    const c = this.c;
    if (info?.isPlayer !== true || id === self.id || info.vendorOpen !== true) return false;
    if (self.visitingVendor === true || e.mods.alt) return false;
    const cam = c.displayed;
    const pos = info.pos;
    if (cam === null || pos === null) return false;
    const limit = c.deps.options().zoomLimit ? VENDOR_CLICK_RANGE_LIMITED : VENDOR_CLICK_RANGE;
    const dCam = Math.hypot(cam.pos[0] - pos[0], cam.pos[1] - pos[1], cam.pos[2] - pos[2]);
    if (dCam >= limit) return false;
    if (c.deps.hooks?.regionSafe?.(id) === true || !e.mods.ctrl) {
      // legacy-send: SendQueryPVendorItem
      c.deps.send?.({
        case: 'queryPVendorItem',
        value: create(QueryPVendorItemSchema, { vendor: id }),
      });
    }
    return true;
  }

  // ---- right button --------------------------------------------------------------------------------------------------

  /** `OnRButtonDown` (§4.6). */
  private rButtonDown(e: ButtonEvent): void {
    const c = this.c;
    const self = c.deps.self();
    if (self === null || self.ownVendorOpen) return;
    c.mouse.ptSpot = e.p;
    c.mouse.tRDown = e.t;
    this.tick.noteChord(e.t); // L already down: the chord jumps (owner #002)
    if (self.flying) c.flight.onRButtonDown(); // mode 0: the steering lock is requested in the gesture (§9.3)
  }

  /** `OnRButtonUp`: the player menu when the pointer did not move while the button was held. */
  private rButtonUp(e: ButtonEvent): void {
    const c = this.c;
    const m = c.mouse;
    const self = c.deps.self();
    this.tick.runImmediate(e.t);
    if (
      self !== null &&
      round(e.p.x) === round(m.ptSpot.x) &&
      round(e.p.y) === round(m.ptSpot.y) &&
      this.menuAllowed() &&
      // ---- housing / guild house (ui-house-quiz): a furniture follows the pointer - the release confirms it and opens no player menu (`MenuException`, `WndWorld.cpp:11799-11830`)
      !this.tick.deploy.rightRelease()
    ) {
      const hl = this.picker.pickAt(e.p.x, e.p.y);
      const info = hl === null ? null : c.deps.entityInfo(hl.id);
      if (
        hl !== null &&
        hl.kind === 'mover' &&
        info?.isPlayer === true &&
        hl.id !== self.id &&
        !info.transparent
      ) {
        c.targeting.autoAttack = false;
        c.targeting.selectTarget = true;
        c.targeting.setFocus(hl.id);
        this.openMoverMenu(hl.id);
      }
    }
    if (self?.flying === true) {
      c.camera.requestUnlock(); // snap back behind the broom
      c.flight.onRButtonUp(); // leaves the mode-0 lock (mode 1 keeps it)
    }
  }

  /** `OnRButtonDblClk`: snap behind on the ground; while flying the free steering mode toggles (§9.3). */
  private rButtonDblClk(): void {
    const self = this.c.deps.self();
    if (self === null) return;
    if (self.flying) this.c.flight.toggleMouseMode();
    else this.c.camera.requestSnapBehind();
  }

  // ---- middle button -------------------------------------------------------------------------------------------------

  private mButtonUp(): void {
    const self = this.c.deps.self();
    if (self?.flying === true) this.c.camera.requestUnlock();
  }

  // ---- the mover menu (§7.14) ----------------------------------------------------------------------------------------

  /**
   * `moverMenu.open(t)` (`CWndWorld::ShowMoverMenu`): inside 20 m the menu of doc 45 opens at the top-right corner of the
   * target's bound rect; farther away the player walks towards the target (no auto-open on arrival).
   */
  openMoverMenu(id: number): void {
    const c = this.c;
    const self = c.deps.self();
    const o = c.deps.entityInfo(id);
    const pos = o?.pos ?? null;
    if (self === null || o === null || pos === null) return;
    const dx = self.pos[0] - pos[0];
    const dy = self.pos[1] - pos[1];
    const dz = self.pos[2] - pos[2];
    const near = dx * dx + dy * dy + dz * dz < MOVER_MENU_RANGE * MOVER_MENU_RANGE;
    if (near && (!c.mouse.R.down || o.isPlayer)) {
      if (self.authority < AUTH_GAMEMASTER && o.transparent) return;
      c.commands.clearDest();
      c.deps.hooks?.showMoverMenu?.(id, c.deps.render()?.projectBounds?.(id) ?? null);
    } else if (!self.flying) {
      c.commands.setDestObj(id, 0);
    }
  }
}
