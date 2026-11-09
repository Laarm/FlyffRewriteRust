// `WndPetStatus` (`CWndPetStatus`, `src:_Interface/WndPetSys.cpp:169-1050`, ref/22 §17 M, Plan/42 §19.4 M, 45 §18.7; WP-UI-33): `APP_PET_STATUS` (192 x 160 with
// the extension, 112 without), the window the summoned SYSTEM pet opens for itself (an egg or a hatched pet; the eat pet has none).
//   * the picture `PetStatusBg.tga` behind everything, the model of the pet over `WIDC_CUSTOM1` (41's preview, an empty canvas without a renderer),
//   * the energy gauge over `WIDC_CUSTOM2` (red, hidden with `features.no_pet_life`) and the experience gauge over `WIDC_CUSTOM3` (blue), both in the
//     outlined three-slice bars of the status window,
//   * the information block - level grade, summed stat bonus, life - and the numbers over the gauges in the display mode a click cycles (2 -> 0 -> 1 -> 2),
//   * the button `WIDC_BUTTON1` toggles the extension (+48 px): the strip `PetLevelBg.tga` over `WIDC_CUSTOM4` with the 1..9 pick of every level D..current
//     as `Icon_PetLevel<n>_S.dds` (34 px pitch of 35), the picks of levels that are not the chosen one dim (alpha 120/255) while the pointer is over the
//     strip, and a tooltip with the bonus of the level under the pointer (`"STR +5"`, red);
//   * the title is the name of the pet and follows it;
//   * `lockLevels` keeps the picks of two levels as they were while the Chance window rolls (`LockShowLevel`).
// The window is placed at the right of the status window (`APP_STATUS1`) when it is open, else 192 px from the left of the layout. The view is polled by
// `process` (10 Hz); an unchanged view commits nothing.

import {
  APP_PET_STATUS,
  APP_STATUS1,
  WIDC_BUTTON1,
  WIDC_CUSTOM1,
  WIDC_CUSTOM2,
  WIDC_CUSTOM3,
  WIDC_CUSTOM4,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { Gauge } from '../../controls/Gauge';
import { type SpriteEntry, SpriteLayer } from '../../controls/SpriteLayer';
import { TextLayer } from '../../controls/TextLayer';
import type { Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectHeight, rectOffset } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import { FONT, textWidth } from '../../text/fonts';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { PreviewHandleLike } from '../login/ports';
import { GAUGE_TEXTURE_NORMAL } from '../char/WndStatus';
import {
  nextPetDisplay,
  PET_DISPLAY_INITIAL,
  type PetDisplay,
  petStatusTexts,
} from './petStatusText';
import type { PetStatusPick, PetStatusRequest, PetStatusView } from './ports';

/** `rect.bottom += 48` of `SetExtension`. */
export const PET_EXTENSION_HEIGHT = 48;
/** `MakeGaugeVertex` colours: the energy `0x64ff0000`, the experience `0x847070ff`. */
export const PET_GAUGE_COLORS = { energy: 0x64ff0000, exp: 0x847070ff } as const;
/** The strip of the extension: one pick icon every 35 px, each hit-tested in a 34 px cell. */
export const PICK_PITCH = 35;
export const PICK_CELL = 34;
/** The picture files. */
export const PET_STATUS_BG = 'theme/PetStatusBg.tga';
export const PET_LEVEL_BG = 'theme/PetLevelBg.tga';
const pickFile = (pick: number): string => `icon/Icon_PetLevel${String(pick)}_S.dds`;

const ID_GAUGE = 0x7a30;
const ID_TEXTS = 0x7a31;
const ID_SPRITES_BEHIND = 0x7a32;
const ID_SPRITES = 0x7a33;

export interface PetStatusDeps {
  readonly request: PetStatusRequest;
  readonly tooltip: TooltipService;
}

/** The preset of each pet kind (`render/preview/presets.ts`). */
const PET_KIND_PRESETS = [
  'petKind0',
  'petKind1',
  'petKind2',
  'petKind3',
  'petKind4',
  'petKind5',
  'petKind6',
] as const;

export class WndPetStatus extends WndNeuz {
  readonly appId = APP_PET_STATUS;
  readonly #request: PetStatusRequest;
  readonly #tooltip: TooltipService;
  #display: PetDisplay = PET_DISPLAY_INITIAL;
  #extended = false;
  #view: PetStatusView | null = null;
  #applied: PetStatusView | null | undefined = undefined;
  #energy: Gauge | null = null;
  #exp: Gauge | null = null;
  #texts: TextLayer | null = null;
  #behind: SpriteLayer | null = null;
  #front: SpriteLayer | null = null;
  #custom: { face: Rect; hp: Rect; exp: Rect; strip: Rect } | null = null;
  #preview: PreviewHandleLike | null = null;
  /** The camera the open preview was made for (`PetStatusView.camera`); a hatched or changed pet needs another one. */
  #previewCamera: number | null | undefined = undefined;
  /** `m_nLockLevel` / `m_nBackUpLevel`: the levels whose picks the Chance window holds, and the picks they show meanwhile. */
  readonly #lock: [{ level: number; pick: number } | null, { level: number; pick: number } | null] =
    [null, null];
  /** `m_nChoiceLevel`: the level under the pointer (`-1` none). */
  #choice = -1;

  constructor(deps: PetStatusDeps) {
    super();
    this.#request = deps.request;
    this.#tooltip = deps.tooltip;
    this.putRegInfo = false;
  }

  get display(): PetDisplay {
    return this.#display;
  }
  get extended(): boolean {
    return this.#extended;
  }
  get energyGauge(): Gauge | null {
    return this.#energy;
  }
  get expGauge(): Gauge | null {
    return this.#exp;
  }
  get texts(): TextLayer | null {
    return this.#texts;
  }
  get sprites(): SpriteLayer | null {
    return this.#front;
  }
  get background(): SpriteLayer | null {
    return this.#behind;
  }
  get toggleButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  get previewHandle(): PreviewHandleLike | null {
    return this.#preview;
  }
  /** The level under the pointer (`m_nChoiceLevel`), `-1` = none. */
  get choiceLevel(): number {
    return this.#choice;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const rectOf = (id: number): Rect => this.getWndCtrl(id)?.rect ?? { l: 0, t: 0, r: 0, b: 0 };
    const custom = {
      face: rectOf(WIDC_CUSTOM1),
      hp: rectOf(WIDC_CUSTOM2),
      exp: rectOf(WIDC_CUSTOM3),
      strip: rectOf(WIDC_CUSTOM4),
    };
    this.#custom = custom;
    const client = this.getClientRect(false);
    this.#behind = this.addControl(new SpriteLayer(), ID_SPRITES_BEHIND, client);
    this.#energy = this.addControl(
      new Gauge({ fill: GAUGE_TEXTURE_NORMAL, color: PET_GAUGE_COLORS.energy }),
      ID_GAUGE,
      custom.hp,
    );
    this.#exp = this.addControl(
      new Gauge({ fill: GAUGE_TEXTURE_NORMAL, color: PET_GAUGE_COLORS.exp }),
      ID_GAUGE,
      custom.exp,
    );
    this.#front = this.addControl(new SpriteLayer(), ID_SPRITES, client);
    this.#texts = this.addControl(new TextLayer(), ID_TEXTS, client);

    const view = this.#request.view.get();
    if (view) this.#openPreview(view);
    // `SetExtension( FALSE )`: the layout is the extended size
    this.#setExtension(false);
    // beside the status window, else at the left of the layout
    const status = this.env.wm.get(APP_STATUS1);
    const layout = this.env.wm.layoutRect;
    if (status) this.move({ x: status.rectWindow.r, y: status.rectWindow.t });
    else this.move({ x: layout.l + 192, y: layout.t });
    this.#apply(view);
  }

  /** `OnDraw`: the model over `WIDC_CUSTOM1` seen by the camera of the egg or of the kind of the pet. */
  #openPreview(view: PetStatusView): void {
    const canvas = this.customCanvas(WIDC_CUSTOM1);
    if (!canvas || !this.#request.previews) return;
    this.#previewCamera = view.camera;
    this.#preview = this.#request.previews.open({
      canvas,
      preset: view.camera === null ? 'petStatus' : (PET_KIND_PRESETS[view.camera] ?? 'petStatus'),
      subject: view.subject,
    });
  }

  // ---- the view ----------------------------------------------------------------------------------------------------

  override process(now: number): void {
    const v = this.#request.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  #apply(v: PetStatusView | null): void {
    this.#applied = v;
    this.#view = v;
    if (v) {
      this.setTitle(v.title);
      this.#energy?.setVisible(v.life !== null);
      this.#energy?.set(v.energy, v.energyMax);
      this.#exp?.set(v.exp, v.expMax);
      // the egg and every kind of pet have their own camera (`SetPetCamTable`)
      if (this.#preview && v.camera !== this.#previewCamera) {
        this.#preview.close();
        this.#preview = null;
      }
      if (this.#preview) this.#preview.setSubject(v.subject);
      else this.#openPreview(v);
    }
    this.#paint();
    this.#preview?.invalidate();
  }

  /** The pick a level shows: the locked one while the Chance window rolls. */
  #pickOf(p: PetStatusPick): number {
    for (const l of this.#lock) if (l?.level === p.level) return l.pick;
    return p.pick;
  }

  #paint(): void {
    const v = this.#view;
    const c = this.#custom;
    if (!v || !c) {
      this.#texts?.setItems([]);
      this.#front?.setEntries([]);
      this.#behind?.setEntries([]);
      return;
    }
    this.#behind?.setEntries([
      {
        key: 'bg',
        path: PET_STATUS_BG,
        x: c.face.l - 12,
        y: c.face.t - 6,
        w: 192,
        h: 81,
      },
    ]);
    this.#texts?.setItems(
      petStatusTexts({
        view: v,
        labels: this.#request.labels,
        display: this.#display,
        rects: { hp: c.hp, exp: c.exp },
        metrics: {
          abilityLabelWidth: Math.round(textWidth(FONT.text, this.#request.labels.ability)),
        },
      }),
    );
    const entries: SpriteEntry[] = [];
    if (this.#extended) {
      let x = c.strip.l - 2;
      const y = c.strip.t - 2;
      entries.push({ key: 'levelbg', path: PET_LEVEL_BG, x, y, w: 172, h: 38 });
      for (const p of v.picks) {
        const pick = this.#pickOf(p);
        if (pick < 1 || pick > 9) continue;
        entries.push({
          key: `pick${String(p.level)}`,
          path: pickFile(pick),
          x,
          y,
          w: 34,
          h: 36,
          // without the `__PETS_RATE` build the levels but the chosen one are drawn at alpha 120
          alpha: this.#choice === p.level || this.#choice === -1 ? 1 : 120 / 255,
        });
        x += PICK_PITCH;
      }
    }
    this.#front?.setEntries(entries);
  }

  // ---- the extension -----------------------------------------------------------------------------------------------

  #setExtension(on: boolean): void {
    this.#extended = on;
    const r = this.rectWindow;
    const full = rectHeight(r);
    const h = on ? full + PET_EXTENSION_HEIGHT : full - PET_EXTENSION_HEIGHT;
    this.setWndRect({ l: r.l, t: r.t, r: r.r, b: r.t + h }, true);
    this.#paint();
  }

  /** `WIDC_BUTTON1`: the extension toggles. */
  toggleExtension(): void {
    this.#setExtension(!this.#extended);
  }

  // ---- LockShowLevel -----------------------------------------------------------------------------------------------

  /**
   * `LockShowLevel(TRUE, level, pos)`: while the Chance window rolls the strip keeps the pick `pick` for `level` (slot `pos` 0 / 1);
   * `LockShowLevel(FALSE, level, pos)` lets the real pick show again.
   */
  lockLevel(pos: 0 | 1, level: number, pick: number | null): void {
    this.#lock[pos] = pick === null ? null : { level, pick };
    this.#paint();
  }

  // ---- input -------------------------------------------------------------------------------------------------------

  /** `OnLButtonDown`: a click cycles the display of the numbers. */
  override onLButtonDown(): void {
    this.#display = nextPetDisplay(this.#display);
    this.#paint();
  }

  /** `OnMouseWndSurface`: the level under the pointer is chosen (bright) and gets its tooltip. */
  override onMouseMove(pt: Point): void {
    const c = this.#custom;
    const v = this.#view;
    if (!c || !v || !this.#extended || this.env.wm.isDraggingShortcut()) return;
    let cell: Rect = { l: c.strip.l, t: c.strip.t, r: c.strip.l + PICK_CELL, b: c.strip.b };
    let choice = -1;
    for (const p of v.picks) {
      if (this.#pickOf(p) >= 1 && rectContains(cell, pt)) {
        const s = this.getScreenClientRect();
        this.#tooltip.put(this, this.#request.pickTip(p.level), rectOffset(cell, s.l, s.t), 0);
        choice = p.level;
      }
      cell = { ...cell, l: cell.r, r: cell.r + PICK_CELL };
    }
    if (choice !== this.#choice) {
      this.#choice = choice;
      this.#paint();
    }
  }

  override onMouseLeave(): void {
    if (this.#choice !== -1) {
      this.#choice = -1;
      this.#paint();
    }
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      this.toggleExtension();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#preview?.close();
    this.#preview = null;
    super.onDestroy();
  }
}
