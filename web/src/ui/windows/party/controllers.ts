// The party windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-39, WP-UI-40): the three applets and the two pages of the party
// window are plain classes (the applet registry builds them synchronously), the dialogs are lazy classes (`lazy.ts`).
import type { WndCtor } from '../../catalog';
import { PartyInfoPage } from './PartyInfoPage';
import { PartySkillPage } from './PartySkillPage';
import { PARTY_LAZY } from './lazy';
import { WndParty } from './WndParty';
import { WndPartyFind } from './WndPartyFind';
import { WndPartyQuick } from './WndPartyQuick';

export const PARTY_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_PARTY: () => Promise.resolve(WndParty),
  APP_PARTY_INFO: () => Promise.resolve(PartyInfoPage),
  APP_PARTY_SKILL: () => Promise.resolve(PartySkillPage),
  APP_PARTY_QUICK: () => Promise.resolve(WndPartyQuick),
  APP_PARTY_CONFIRM: () => PARTY_LAZY.WndPartyConfirm.load(),
  APP_PARTYLEAVE_CONFIRM: () => PARTY_LAZY.WndPartyLeaveConfirm.load(),
  APP_PARTYCHANGENAME: () => PARTY_LAZY.WndPartyChangeName.load(),
  APP_CHANGETROUP: () => PARTY_LAZY.WndPartyChangeTroup.load(),
  APP_PARTY_FIND: () => Promise.resolve(WndPartyFind),
  APP_SUMMON_PARTY: () => PARTY_LAZY.WndSummonParty.load(),
  APP_SUMMON_PARTY_USE: () => PARTY_LAZY.WndSummonPartyUse.load(),
  APP_MESSAGEBOX_SUMMONPARTY: () => PARTY_LAZY.WndSummonPartyMsg.load(),
};
