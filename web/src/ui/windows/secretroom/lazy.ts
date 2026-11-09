// The secret-room windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports these shells dynamically. One class per line (the vitest transform `lazyWndSync` reads this
// file line by line).
import { LazyWnd } from '../../wnd/lazy';

import type * as Offer from './WndSecretRoomOffer';
import type * as OfferState from './WndSecretRoomOfferState';
import type * as LineUp from './WndLineUpSelection';
import type * as Board from './WndSecretRoomBoard';
import type * as InfoBox from './WndSecretRoomInfoBox';
import type * as Quick from './WndSecretRoomQuick';
import type * as CancelConfirm from './WndSecretRoomCancelConfirm';
import type * as ChangeTaxRate from './WndSecretRoomChangeTaxRate';
import type * as CheckTaxRate from './WndSecretRoomCheckTaxRate';
import type * as GuildMemMng from './WndSecretRoomGuildMemMng';
import type * as GuildMember from './WndSecretRoomGuildMember';

export const SECRETROOM_LAZY = {
  WndSecretRoomOffer: new LazyWnd<typeof Offer.WndSecretRoomOffer>(
    'WndSecretRoomOffer',
    async () => (await import('./WndSecretRoomOffer')).WndSecretRoomOffer,
  ),
  WndSecretRoomOfferState: new LazyWnd<typeof OfferState.WndSecretRoomOfferState>(
    'WndSecretRoomOfferState',
    async () => (await import('./WndSecretRoomOfferState')).WndSecretRoomOfferState,
  ),
  WndLineUpSelection: new LazyWnd<typeof LineUp.WndLineUpSelection>(
    'WndLineUpSelection',
    async () => (await import('./WndLineUpSelection')).WndLineUpSelection,
  ),
  WndSecretRoomBoard: new LazyWnd<typeof Board.WndSecretRoomBoard>(
    'WndSecretRoomBoard',
    async () => (await import('./WndSecretRoomBoard')).WndSecretRoomBoard,
  ),
  WndSecretRoomInfoBox: new LazyWnd<typeof InfoBox.WndSecretRoomInfoBox>(
    'WndSecretRoomInfoBox',
    async () => (await import('./WndSecretRoomInfoBox')).WndSecretRoomInfoBox,
  ),
  WndSecretRoomQuick: new LazyWnd<typeof Quick.WndSecretRoomQuick>(
    'WndSecretRoomQuick',
    async () => (await import('./WndSecretRoomQuick')).WndSecretRoomQuick,
  ),
  WndSecretRoomCancelConfirm: new LazyWnd<typeof CancelConfirm.WndSecretRoomCancelConfirm>(
    'WndSecretRoomCancelConfirm',
    async () => (await import('./WndSecretRoomCancelConfirm')).WndSecretRoomCancelConfirm,
  ),
  WndSecretRoomChangeTaxRate: new LazyWnd<typeof ChangeTaxRate.WndSecretRoomChangeTaxRate>(
    'WndSecretRoomChangeTaxRate',
    async () => (await import('./WndSecretRoomChangeTaxRate')).WndSecretRoomChangeTaxRate,
  ),
  WndSecretRoomCheckTaxRate: new LazyWnd<typeof CheckTaxRate.WndSecretRoomCheckTaxRate>(
    'WndSecretRoomCheckTaxRate',
    async () => (await import('./WndSecretRoomCheckTaxRate')).WndSecretRoomCheckTaxRate,
  ),
  WndSecretRoomGuildMemMng: new LazyWnd<typeof GuildMemMng.WndSecretRoomGuildMemMng>(
    'WndSecretRoomGuildMemMng',
    async () => (await import('./WndSecretRoomGuildMemMng')).WndSecretRoomGuildMemMng,
  ),
  WndSecretRoomGuildMember: new LazyWnd<typeof GuildMember.WndSecretRoomGuildMember>(
    'WndSecretRoomGuildMember',
    async () => (await import('./WndSecretRoomGuildMember')).WndSecretRoomGuildMember,
  ),
} as const;
