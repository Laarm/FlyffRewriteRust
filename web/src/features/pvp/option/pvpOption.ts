// The PvP option (Plan/45 §25.2, §27.1 "accept PvP"; `src:_Interface/WndOptionGame.cpp:372-382`; WP-CFEAT-36): the game tab of the options
// window (`APP_OPTEX_GAME12`, M8) has the radio pair "accept PvP" (`WIDC_ACCEPTPVP_ON` / `_OFF`). Its state is the server's: the radio shows
// `PVPCONFIRM_MODE` of the own mode word (`ModifyMode` brings every change, also the one of the click), a click sends `ChangeMode`:
// ON `{PVPCONFIRM_MODE, 3}` (clears the flag, duel requests are accepted), OFF `{PVPCONFIRM_MODE, 2}` (sets it, the server answers a challenge
// with `TID_PK_MODE_REJECT`). The options window finds this through `pvpOptionOf(ctx)`; the state is never stored locally (§27.1).

import type { ClientContext } from '../../../app/context';
import { type FeatureContext, type FeatureModule } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { derivedView, type ViewSource } from '../../common/view';
import { sendChangeMode } from './senders';

/** `FREEPK_MODE` of `authorization.h`: attack players without Ctrl (a client convenience, the PK radio of the pre-V9 option window). */
export const FREEPK_MODE = 0x1000;
/** `PVPCONFIRM_MODE`: the player refuses duel requests. */
export const PVPCONFIRM_MODE = 0x2000;

/** `ChangeMode.flag` values (`DPSrvr.cpp:6911`). */
export const MODE_FLAG = {
  SET_FREEPK: 0,
  CLEAR_FREEPK: 1,
  SET_PVPCONFIRM: 2,
  CLEAR_PVPCONFIRM: 3,
} as const;

/** The player accepts duels: `PVPCONFIRM_MODE` is not set (`g_pPlayer->IsMode( PVPCONFIRM_MODE )` selects the OFF radio). */
export function acceptsPvp(modeFlags: number): boolean {
  return (modeFlags & PVPCONFIRM_MODE) === 0;
}

/** The `flag` of `ChangeMode` for the radio the player clicked. */
export function pvpOptionFlag(accept: boolean): number {
  return accept ? MODE_FLAG.CLEAR_PVPCONFIRM : MODE_FLAG.SET_PVPCONFIRM;
}

export interface PvpOptionView {
  /** The radio "accept PvP" is on. */
  readonly accept: boolean;
}

/** The public face of the PvP option of a context. */
export interface PvpOptionFeature {
  /** The server-held state (the own mode word). */
  accepts(): boolean;
  /** A click on a radio: `ChangeMode`. */
  set(accept: boolean): void;
  readonly view: ViewSource<PvpOptionView>;
}

const features = new WeakMap<ClientContext, PvpOptionFeature>();
export function pvpOptionOf(ctx: ClientContext): PvpOptionFeature | undefined {
  return features.get(ctx);
}

export const pvpOptionModule: FeatureModule = {
  id: 'pvp',
  milestone: 5,
  register(fc) {
    registerPvpOption(fc);
  },
};

function registerPvpOption(fc: FeatureContext): PvpOptionFeature {
  const ctx = fc.app;
  const { selfStore } = ctx.stores;
  const accepts = (): boolean => acceptsPvp(selfStore.getState().modeFlags);
  const view = derivedView<PvpOptionView>(() => ({ accept: accepts() }), [selfStore]);
  const feature: PvpOptionFeature = {
    accepts,
    set(accept) {
      sendChangeMode(ctx, PVPCONFIRM_MODE, pvpOptionFlag(accept));
    },
    view,
  };
  features.set(ctx, feature);
  addProbeMember(ctx, 'pvpOption', () => ({ accept: accepts() }));
  return feature;
}
