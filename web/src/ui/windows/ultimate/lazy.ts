// The ultimate-weapon windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports the shells dynamically.
import { LazyWnd } from '../../wnd/lazy';
import type * as ChangeWeapon from './WndChangeWeapon';
import type * as Extraction from './WndExtraction';
import type * as MixJewel from './WndMixJewel';
import type * as RemoveJewel from './WndRemoveJewel';
import type * as SmeltJewel from './WndSmeltJewel';

export const ULTIMATE_LAZY = {
  WndMixJewel: new LazyWnd<typeof MixJewel.WndMixJewel>(
    'WndMixJewel',
    async () => (await import('./WndMixJewel')).WndMixJewel,
  ),
  WndMixJewelConfirm: new LazyWnd<typeof MixJewel.WndMixJewelConfirm>(
    'WndMixJewelConfirm',
    async () => (await import('./WndMixJewel')).WndMixJewelConfirm,
  ),
  WndExtraction: new LazyWnd<typeof Extraction.WndExtraction>(
    'WndExtraction',
    async () => (await import('./WndExtraction')).WndExtraction,
  ),
  WndSmeltJewel: new LazyWnd<typeof SmeltJewel.WndSmeltJewel>(
    'WndSmeltJewel',
    async () => (await import('./WndSmeltJewel')).WndSmeltJewel,
  ),
  WndChangeWeapon: new LazyWnd<typeof ChangeWeapon.WndChangeWeapon>(
    'WndChangeWeapon',
    async () => (await import('./WndChangeWeapon')).WndChangeWeapon,
  ),
  WndRemoveJewel: new LazyWnd<typeof RemoveJewel.WndRemoveJewel>(
    'WndRemoveJewel',
    async () => (await import('./WndRemoveJewel')).WndRemoveJewel,
  ),
  WndRemoveJewelConfirm: new LazyWnd<typeof RemoveJewel.WndRemoveJewelConfirm>(
    'WndRemoveJewelConfirm',
    async () => (await import('./WndRemoveJewel')).WndRemoveJewelConfirm,
  ),
} as const;
