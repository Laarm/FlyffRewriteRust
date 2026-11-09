// The item-use dialogs in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports these shells dynamically. They are opened with request
// objects (`ui.itemUse.open*`) when an item asks for them. The applet windows (the duration list `APP_COMM_ITEM`, the buff status window and the buff
// strip) are NOT here: the applet registry builds them synchronously.
import { LazyWnd } from '../../wnd/lazy';
import type * as ChangeAttribute from './WndChangeAttribute';
import type * as ChangeName from './WndChangeName';
import type * as EquipBindConfirm from './WndEquipBindConfirm';
import type * as QuestItemInfo from './WndQuestItemInfo';
import type * as RandomOptionRetry from './WndRandomOptionRetry';
import type * as SelectChannel from './WndSelectChannel';
import type * as SelectVillage from './WndSelectVillage';
import type * as SummonAngel from './WndSummonAngel';
import type * as YesNoText from './WndYesNoText';

export const ITEMUSE_LAZY = {
  WndEquipBindConfirm: new LazyWnd<typeof EquipBindConfirm.WndEquipBindConfirm>(
    'WndEquipBindConfirm',
    async () => (await import('./WndEquipBindConfirm')).WndEquipBindConfirm,
  ),
  WndQuestItemInfo: new LazyWnd<typeof QuestItemInfo.WndQuestItemInfo>(
    'WndQuestItemInfo',
    async () => (await import('./WndQuestItemInfo')).WndQuestItemInfo,
  ),
  WndSelectVillage: new LazyWnd<typeof SelectVillage.WndSelectVillage>(
    'WndSelectVillage',
    async () => (await import('./WndSelectVillage')).WndSelectVillage,
  ),
  WndSummonAngel: new LazyWnd<typeof SummonAngel.WndSummonAngel>(
    'WndSummonAngel',
    async () => (await import('./WndSummonAngel')).WndSummonAngel,
  ),
  WndRandomScrollConfirm: new LazyWnd<typeof YesNoText.WndRandomScrollConfirm>(
    'WndRandomScrollConfirm',
    async () => (await import('./WndYesNoText')).WndRandomScrollConfirm,
  ),
  WndSmeltSafetyConfirm: new LazyWnd<typeof YesNoText.WndSmeltSafetyConfirm>(
    'WndSmeltSafetyConfirm',
    async () => (await import('./WndYesNoText')).WndSmeltSafetyConfirm,
  ),
  WndRemoveElem: new LazyWnd<typeof YesNoText.WndRemoveElem>(
    'WndRemoveElem',
    async () => (await import('./WndYesNoText')).WndRemoveElem,
  ),
  WndRestateConfirm: new LazyWnd<typeof YesNoText.WndRestateConfirm>(
    'WndRestateConfirm',
    async () => (await import('./WndYesNoText')).WndRestateConfirm,
  ),
  WndFunnyCoinConfirm: new LazyWnd<typeof YesNoText.WndFunnyCoinConfirm>(
    'WndFunnyCoinConfirm',
    async () => (await import('./WndYesNoText')).WndFunnyCoinConfirm,
  ),
  WndConfirmEnter: new LazyWnd<typeof YesNoText.WndConfirmEnter>(
    'WndConfirmEnter',
    async () => (await import('./WndYesNoText')).WndConfirmEnter,
  ),
  WndSelectChannel: new LazyWnd<typeof SelectChannel.WndSelectChannel>(
    'WndSelectChannel',
    async () => (await import('./WndSelectChannel')).WndSelectChannel,
  ),
  WndChangeName: new LazyWnd<typeof ChangeName.WndChangeName>(
    'WndChangeName',
    async () => (await import('./WndChangeName')).WndChangeName,
  ),
  WndChangeAttribute: new LazyWnd<typeof ChangeAttribute.WndChangeAttribute>(
    'WndChangeAttribute',
    async () => (await import('./WndChangeAttribute')).WndChangeAttribute,
  ),
  WndRandomOptionRetry: new LazyWnd<typeof RandomOptionRetry.WndRandomOptionRetry>(
    'WndRandomOptionRetry',
    async () => (await import('./WndRandomOptionRetry')).WndRandomOptionRetry,
  ),
} as const;
