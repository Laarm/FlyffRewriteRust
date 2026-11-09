// The `npc` feature, beauty part (Plan/45 §17; WP-CFEAT-19, WP-UI-35 partial): the hair shop (`APP_BEAUTY_SHOP_EX`) and the face shop
// (`APP_BEAUTY_SHOP_SKIN`) with their confirmation (`APP_BEAUTY_SHOP_EX_CONFIRM`).
//
//   * `MMI_BEAUTYSHOP` (13) / `MMI_BEAUTYSHOP_SKIN` (42) on a hair designer / makeup artist: the busy checks (`busy.ts`), a disguised
//     player is refused (`TID_GAME_ERROR_BEAUTYSHOP_DISGUISE`), then the window opens on a DRAFT of the player's current look;
//   * the hair draft is a style 0..9 and a colour red / green / blue 0..255; the prices are `rules.hairCost` (the style alone, the
//     colour alone, both), 0 in coupon mode; the face draft is a head 0..10 for `CHANGE_FACE_COST` (1 000 000), 0 in coupon mode;
//   * OK opens the confirmation (`TID_GAME_CHANGE_HAIR_QUESTION` / `TID_GAME_CHANGE_FACE_QUESTION`); Yes sends `SetHairReq` /
//     `ChangeFaceReq` (VR-026: the server checks the coupon first, then the gold) and closes the shop; a draft that equals the current
//     look closes without sending;
//   * the answers `SetHairNtf` / `ChangeFaceNtf` / `SexChange` rewrite the visual state of the mover they name (the renderer diffs
//     it): the hair of the player and his paper doll change at once.
// The coupon route (WP-CFEAT-14, rule R14 -> the `appearance` feature): the double click on a hair or make-up coupon asks `askCoupon(kind)`
// ("You have a Free Coupon. Do you want to use it now?", `CWndUseCouponConfirm` flag 0); Yes opens the shop in coupon mode (the inventory
// first, the title says "Using the Free Coupon."), where everything is free and OK asks again (flag 1: the question and the red warning
// that the coupon is gone) before `SetHairReq` / `ChangeFaceReq` goes out with `use_coupon`; an unchanged look just closes the shop.
// The windows hold no price logic. The 3-D previews (doc 41: the own mover as is, the draft turning, the head of each style / face) come from
// the preview service through `previewPortOf`; the subjects are the own `MoverVisualState` with the draft hair or head put on.

import type { ClientContext } from '../../../app/context';
import { EntityKind, type EntityRecord } from '../../../game/entities';
import { rulesRuntime } from '../../../game/rules';
import {
  APP_BEAUTY_SHOP_EX,
  APP_BEAUTY_SHOP_EX_CONFIRM,
  APP_BEAUTY_SHOP_SKIN,
  APP_INVENTORY,
} from '../../../gen/defs/resdata';
import { MI_FEMALE } from '../../../gen/defs/obj';
import { MMI_BEAUTYSHOP, MMI_BEAUTYSHOP_SKIN } from '../../../gen/defs/neuz';
import {
  TID_GAME_ASKUSE_COUPON1,
  TID_GAME_ASKUSE_COUPON2,
  TID_GAME_CHANGE_FACE,
  TID_GAME_CHANGE_FACE_QUESTION,
  TID_GAME_CHANGE_HAIR,
  TID_GAME_CHANGE_HAIR_QUESTION,
  TID_GAME_ERROR_BEAUTYSHOP_DISGUISE,
  TID_GAME_SHOP_CHOICE,
  TID_GAME_SHOP_COST,
  TID_GAME_SHOP_PENYA,
  TID_GAME_USE_CHANGE_COUPON,
  TID_GAME_WARNNING_USE_COUPON,
} from '../../../gen/defs/text';
import type {
  BeautyPreviews,
  FacePreviewSubjects,
  FaceShopActions,
  FaceShopView,
  HairPreviewSubjects,
  HairShopActions,
  HairShopView,
  ViewSource,
} from '../../../ui/windows/npc/ports';
import { previewPortOf } from '../../charselect/previews';
import { type FeatureContext, fullUi } from '../../common/module';
import { fmtNumberComma } from '../../common/numbers';
import { addProbeMember } from '../../common/probe';
import { sysChat } from '../../common/text';
import { busyText } from '../busy';
import type { MenuFeature } from '../menuModule';
import { createShopSenders, type ShopSenders } from '../shop/senders';
import {
  CARDS,
  CHANGE_FACE_COST,
  type FaceDraft,
  type HairDraft,
  HAIR_COST,
  HAIR_MESHES,
  HAIRCOLOR_COST,
  HEAD_MESHES,
  clampChannel,
  faceRow,
  hairCards,
  hairChanged,
  initialFaceDraft,
  initialHairDraft,
  newFaceRow,
  numberToRgb,
  rgbToNumber,
  stepFresh,
  stepFriendly,
  stepPage,
  wrap,
} from './state';

export interface BeautyFeature {
  /** Opens the hair shop on the player's current look (`false` when a busy check or the disguise refused). */
  openHair(opts?: { coupon?: boolean }): boolean;
  /** Opens the face shop on the player's current face. */
  openFace(opts?: { coupon?: boolean }): boolean;
  /**
   * The coupon item (rule R14): "You have a Free Coupon. Do you want to use it now?" with Yes / No; Yes opens the hair or face shop in
   * coupon mode (`CWndUseCouponConfirm`, flag 0).
   */
  askCoupon(kind: 'hair' | 'face'): void;
  readonly hairView: ViewSource<HairShopView>;
  readonly faceView: ViewSource<FaceShopView>;
  readonly senders: ShopSenders;
  closeAll(): void;
}

const features = new WeakMap<ClientContext, BeautyFeature>();

/** The beauty feature of a context; `undefined` before the module registered. */
export function beautyOf(ctx: ClientContext): BeautyFeature | undefined {
  return features.get(ctx);
}

/** A tiny observable value (the draft views). */
function localView<T>(initial: T): ViewSource<T> & { set(v: T): void } {
  let cur = initial;
  const fns = new Set<() => void>();
  return {
    get: () => cur,
    subscribe(fn) {
      fns.add(fn);
      return () => {
        fns.delete(fn);
      };
    },
    set(v) {
      cur = v;
      for (const fn of [...fns]) fn();
    },
  };
}

export function registerBeauty(fc: FeatureContext, menu: MenuFeature): BeautyFeature {
  const ctx = fc.app;
  const senders = createShopSenders(ctx);
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const say = (tid: number): void => {
    sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid);
  };
  const stats = { hairOpened: 0, faceOpened: 0, hairSent: 0, faceSent: 0, refused: 0 };

  // ---- the draft state ---------------------------------------------------------------------------------------------------------

  let version = 0;
  let hair: { draft: HairDraft; coupon: boolean; open: boolean } = {
    draft: initialHairDraft(0, 0),
    coupon: false,
    open: false,
  };
  let face: { draft: FaceDraft; coupon: boolean; open: boolean } = {
    draft: initialFaceDraft(0),
    coupon: false,
    open: false,
  };

  const selfVisual = () => ctx.entities.self()?.visual;
  const current = () => {
    const v = selfVisual();
    return {
      mesh: wrap(v?.hairMesh ?? 0, HAIR_MESHES),
      color: (v?.hairColor ?? 0) & 0xffffff,
      face: wrap(v?.headMesh ?? 0, HEAD_MESHES),
    };
  };

  /** `rules.hairCost` of a draft; the constants when the rules are not loaded (the server charges the same). */
  const hairPrice = (mesh: number, rgb: readonly [number, number, number]): number => {
    const cur = current();
    const rules = rulesRuntime(ctx).rules();
    if (rules) {
      return rules.hairCost({
        curMesh: cur.mesh,
        curColor: (0xff000000 | cur.color) >>> 0,
        rgb,
        hair: mesh,
      });
    }
    const colorChanged = rgbToNumber(rgb) !== cur.color;
    return (mesh !== cur.mesh ? HAIR_COST : 0) + (colorChanged ? HAIRCOLOR_COST : 0);
  };

  const buildHair = (): HairShopView => {
    const cur = current();
    const curRgb = numberToRgb(cur.color);
    const d = hair.draft;
    return {
      version,
      mesh: d.mesh,
      meshCount: HAIR_MESHES,
      rgb: d.rgb,
      cards: hairCards(d.page),
      currentMesh: cur.mesh,
      currentRgb: curRgb,
      stylePrice: hair.coupon ? 0 : hairPrice(d.mesh, curRgb),
      colorPrice: hair.coupon ? 0 : hairPrice(cur.mesh, d.rgb),
      total: hair.coupon ? 0 : hairPrice(d.mesh, d.rgb),
      coupon: hair.coupon,
    };
  };
  const buildFace = (): FaceShopView => {
    const d = face.draft;
    return {
      version,
      face: d.face,
      currentFace: current().face,
      faceCount: HEAD_MESHES,
      friendly: faceRow(d.friendlyStart),
      fresh: newFaceRow(d.freshStart),
      cost: face.coupon || d.face === current().face ? 0 : CHANGE_FACE_COST,
      coupon: face.coupon,
    };
  };

  const hairView = localView<HairShopView>(buildHair());
  const faceView = localView<FaceShopView>(buildFace());
  const publish = (): void => {
    version += 1;
    if (hair.open) hairView.set(buildHair());
    if (face.open) faceView.set(buildFace());
  };

  // ---- closing and opening -----------------------------------------------------------------------------------------------------

  const closeAll = (): void => {
    const ui = fullUi(ctx);
    for (const app of [APP_BEAUTY_SHOP_EX, APP_BEAUTY_SHOP_SKIN, APP_BEAUTY_SHOP_EX_CONFIRM]) {
      ui?.wm.get(app)?.destroy();
    }
    hair = { ...hair, open: false };
    face = { ...face, open: false };
  };

  const refused = (): boolean => {
    const ui = fullUi(ctx);
    if (!ui) return true;
    const busy = busyText((app) => ui.wm.isOpen(app));
    if (busy !== null) {
      say(busy);
      stats.refused += 1;
      return true;
    }
    if (selfVisual()?.disguiseMover != null) {
      say(TID_GAME_ERROR_BEAUTYSHOP_DISGUISE);
      stats.refused += 1;
      return true;
    }
    return false;
  };

  /** `CWndBeautyShopConfirm`: "Selected <Change your Hair> Price is <cost> Penya. <Do you want to change your hair?>". */
  const openPaidConfirm = (kind: 'hair' | 'face', cost: number, yes: () => void): void => {
    const hairText = kind === 'hair';
    fullUi(ctx)?.npc.openBeautyConfirm({
      text: [
        text(TID_GAME_SHOP_CHOICE),
        text(hairText ? TID_GAME_CHANGE_HAIR : TID_GAME_CHANGE_FACE),
        text(TID_GAME_SHOP_COST),
        fmtNumberComma(cost),
        text(TID_GAME_SHOP_PENYA),
        text(hairText ? TID_GAME_CHANGE_HAIR_QUESTION : TID_GAME_CHANGE_FACE_QUESTION),
      ].join(' '),
      yes,
      no: () => undefined,
    });
  };

  /** `CWndUseCouponConfirm` flag 1: the question and the red warning; Yes sends the change. */
  const openCouponChange = (yes: () => void): void => {
    fullUi(ctx)?.npc.openBeautyConfirm({
      title: text(TID_GAME_USE_CHANGE_COUPON),
      text: text(TID_GAME_ASKUSE_COUPON2),
      warning: text(TID_GAME_WARNNING_USE_COUPON),
      yes,
      no: () => undefined,
    });
  };

  const hairActions: HairShopActions = {
    step(delta) {
      hair = { ...hair, draft: stepPage(hair.draft, delta) };
      publish();
    },
    pick(mesh) {
      hair = { ...hair, draft: { ...hair.draft, mesh: wrap(mesh, HAIR_MESHES) } };
      publish();
    },
    setChannel(channel, value) {
      const rgb = [...hair.draft.rgb] as [number, number, number];
      rgb[channel] = clampChannel(value);
      hair = { ...hair, draft: { ...hair.draft, rgb } };
      publish();
    },
    reset() {
      const cur = current();
      hair = { ...hair, draft: initialHairDraft(cur.mesh, cur.color) };
      publish();
    },
    ok() {
      const cur = current();
      const shop = (): void => {
        fullUi(ctx)?.wm.get(APP_BEAUTY_SHOP_EX)?.destroy();
      };
      const send = (): void => {
        const d = hair.draft;
        senders.setHair(d.mesh, d.rgb[0], d.rgb[1], d.rgb[2], hair.coupon);
        stats.hairSent += 1;
        shop();
      };
      // `WIDC_OK`: nothing to buy (or an unchanged look with a coupon) just closes the window; the coupon asks again with the warning
      if (!hairChanged(hair.draft, cur.mesh, cur.color)) {
        shop();
        return;
      }
      if (hair.coupon) {
        openCouponChange(send);
        return;
      }
      const cost = hairView.get().total;
      if (cost <= 0) {
        shop();
        return;
      }
      openPaidConfirm('hair', cost, send);
    },
    closed() {
      hair = { ...hair, open: false };
    },
  };

  const faceActions: FaceShopActions = {
    stepFriendly(delta) {
      face = { ...face, draft: stepFriendly(face.draft, delta) };
      publish();
    },
    stepNew(delta) {
      face = { ...face, draft: stepFresh(face.draft, delta) };
      publish();
    },
    pick(f) {
      face = { ...face, draft: { ...face.draft, face: wrap(f, HEAD_MESHES) } };
      publish();
    },
    reset() {
      face = { ...face, draft: initialFaceDraft(current().face) };
      publish();
    },
    ok() {
      const shop = (): void => {
        fullUi(ctx)?.wm.get(APP_BEAUTY_SHOP_SKIN)?.destroy();
      };
      const send = (): void => {
        senders.changeFace(face.draft.face, face.coupon ? 0 : CHANGE_FACE_COST, face.coupon);
        stats.faceSent += 1;
        shop();
      };
      if (face.draft.face === current().face) {
        shop();
        return;
      }
      if (face.coupon) {
        openCouponChange(send);
        return;
      }
      openPaidConfirm('face', CHANGE_FACE_COST, send);
    },
    closed() {
      face = { ...face, open: false };
    },
  };

  // ---- the 3-D previews ------------------------------------------------------------------------------------------------------------

  /** The own mover with the draft put on: a copy of the visual state (the equipment map is shared, the renderer reads it). */
  const looks = (over: { hairMesh?: number; hairColor?: number; headMesh?: number }) => {
    const v = selfVisual();
    return v ? { ...v, ...over } : null;
  };
  const hairSubjects: HairPreviewSubjects = {
    before: () => looks({}),
    after: (view) => looks({ hairMesh: view.mesh, hairColor: rgbToNumber(view.rgb) }),
    card: (mesh) => looks({ hairMesh: mesh }),
  };
  const faceSubjects: FacePreviewSubjects = {
    before: () => looks({}),
    after: (view) => looks({ headMesh: view.face }),
    card: (headMesh) => looks({ headMesh }),
  };
  /** `null` without WebGL2 or without an own mover: the windows keep the placeholder cards. */
  const previewsOf = <S>(subjects: S): BeautyPreviews<S> | null => {
    if (!selfVisual()) return null;
    const port = previewPortOf(ctx);
    return port ? { port, subjects } : null;
  };

  const openHair = (opts: { coupon?: boolean } = {}): boolean => {
    if (refused()) return false;
    const ui = fullUi(ctx);
    if (!ui) return false;
    const cur = current();
    hair = {
      draft: initialHairDraft(cur.mesh, cur.color),
      coupon: opts.coupon === true,
      open: true,
    };
    version += 1;
    hairView.set(buildHair());
    ui.wm.openApplet(APP_INVENTORY);
    ui.npc.openHairShop({
      view: hairView,
      actions: hairActions,
      previews: previewsOf(hairSubjects),
    });
    stats.hairOpened += 1;
    return true;
  };

  const openFace = (opts: { coupon?: boolean } = {}): boolean => {
    if (refused()) return false;
    const ui = fullUi(ctx);
    if (!ui) return false;
    face = {
      draft: initialFaceDraft(current().face),
      coupon: opts.coupon === true,
      open: true,
    };
    version += 1;
    faceView.set(buildFace());
    ui.wm.openApplet(APP_INVENTORY);
    ui.npc.openFaceShop({
      view: faceView,
      actions: faceActions,
      previews: previewsOf(faceSubjects),
    });
    stats.faceOpened += 1;
    return true;
  };

  const askCoupon = (kind: 'hair' | 'face'): void => {
    fullUi(ctx)?.npc.openBeautyConfirm({
      title: text(TID_GAME_USE_CHANGE_COUPON),
      text: text(TID_GAME_ASKUSE_COUPON1),
      yes: () => {
        if (kind === 'hair') openHair({ coupon: true });
        else openFace({ coupon: true });
      },
      no: () => undefined,
    });
  };

  menu.registerAction(MMI_BEAUTYSHOP, ({ npcKey }) => {
    if (npcKey !== null) openHair();
  });
  menu.registerAction(MMI_BEAUTYSHOP_SKIN, ({ npcKey }) => {
    if (npcKey !== null) openFace();
  });
  fc.transactional.register({ id: 'npc.beauty', reasons: 'all', close: closeAll });

  // ---- the reducers --------------------------------------------------------------------------------------------------------------

  /** The mover record a notification names: the header object id, else (face) the character id of the payload. */
  const moverOf = (objId: number, idPlayer?: number): EntityRecord | undefined => {
    const byObj = ctx.entities.get(objId);
    if (byObj?.visual) return byObj;
    if (idPlayer === undefined) return undefined;
    let found: EntityRecord | undefined;
    ctx.entities.forEach(EntityKind.Player, (r) => {
      if (r.idPlayer === idPlayer) found = r;
    });
    return found;
  };

  fc.net.on('setHairNtf', (m, objId) => {
    const rec = moverOf(objId);
    if (!rec?.visual) return;
    rec.visual.hairMesh = m.hair;
    rec.visual.hairColor = rgbToNumber([clampChannel(m.r), clampChannel(m.g), clampChannel(m.b)]);
    if (rec.isSelf && hair.open) publish();
  });
  fc.net.on('changeFaceNtf', (m, objId) => {
    const rec = moverOf(objId, m.idPlayer);
    if (!rec?.visual) return;
    rec.visual.headMesh = m.face;
    if (rec.isSelf && face.open) publish();
  });
  fc.net.on('sexChange', (m, objId) => {
    const rec = ctx.entities.get(objId);
    if (!rec?.visual) return;
    rec.propId = m.moverIndex;
    rec.visual.moverIndex = m.moverIndex;
    rec.visual.sex = m.moverIndex === MI_FEMALE ? 1 : 0;
    if (rec.isSelf) ctx.stores.selfStore.setState({ sex: rec.visual.sex });
  });

  addProbeMember(ctx, 'beauty', () => ({
    hair: { open: hair.open, coupon: hair.coupon, mesh: hair.draft.mesh, rgb: [...hair.draft.rgb] },
    face: { open: face.open, coupon: face.coupon, face: face.draft.face },
    stats: { ...stats },
    cards: CARDS,
    /** The look the server last confirmed (`SetHairNtf` / `ChangeFaceNtf` / `SexChange`). */
    current: { ...current(), sex: selfVisual()?.sex ?? 0 },
  }));

  const feature: BeautyFeature = {
    openHair,
    openFace,
    askCoupon,
    hairView,
    faceView,
    senders,
    closeAll,
  };
  features.set(ctx, feature);
  return feature;
}
