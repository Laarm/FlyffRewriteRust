// The upgrade, element, awakening and piercing windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports the shells dynamically.
import { LazyWnd } from '../../wnd/lazy';
import type * as Commercial from './WndCommercialElem';
import type * as Piercing from './WndPiercing';
import type * as RemoveAttribute from './WndRemoveAttribute';
import type * as RemovePiercing from './WndRemovePiercing';
import type * as SelectAwake from './WndSelectAwakeCase';
import type * as Smelt from './WndSmeltSafety';
import type * as StartItem from './WndStartItem';
import type * as UpgradeBase from './WndUpgradeBase';

export const UPGRADE_LAZY = {
  WndUpgradeBase: new LazyWnd<typeof UpgradeBase.WndUpgradeBase>(
    'WndUpgradeBase',
    async () => (await import('./WndUpgradeBase')).WndUpgradeBase,
  ),
  WndSmeltSafety: new LazyWnd<typeof Smelt.WndSmeltSafety>(
    'WndSmeltSafety',
    async () => (await import('./WndSmeltSafety')).WndSmeltSafety,
  ),
  WndRemoveAttribute: new LazyWnd<typeof RemoveAttribute.WndRemoveAttribute>(
    'WndRemoveAttribute',
    async () => (await import('./WndRemoveAttribute')).WndRemoveAttribute,
  ),
  WndRemoveAttributeConfirm: new LazyWnd<typeof RemoveAttribute.WndRemoveAttributeConfirm>(
    'WndRemoveAttributeConfirm',
    async () => (await import('./WndRemoveAttribute')).WndRemoveAttributeConfirm,
  ),
  WndCommercialElem: new LazyWnd<typeof Commercial.WndCommercialElem>(
    'WndCommercialElem',
    async () => (await import('./WndCommercialElem')).WndCommercialElem,
  ),
  WndAwakening: new LazyWnd<typeof StartItem.WndAwakening>(
    'WndAwakening',
    async () => (await import('./WndStartItem')).WndAwakening,
  ),
  WndSelectAwakeCase: new LazyWnd<typeof SelectAwake.WndSelectAwakeCase>(
    'WndSelectAwakeCase',
    async () => (await import('./WndSelectAwakeCase')).WndSelectAwakeCase,
  ),
  WndCancelBlessing: new LazyWnd<typeof StartItem.WndCancelBlessing>(
    'WndCancelBlessing',
    async () => (await import('./WndStartItem')).WndCancelBlessing,
  ),
  WndLvReqDown: new LazyWnd<typeof StartItem.WndLvReqDown>(
    'WndLvReqDown',
    async () => (await import('./WndStartItem')).WndLvReqDown,
  ),
  WndPetAwakCancel: new LazyWnd<typeof StartItem.WndPetAwakCancel>(
    'WndPetAwakCancel',
    async () => (await import('./WndStartItem')).WndPetAwakCancel,
  ),
  WndPiercing: new LazyWnd<typeof Piercing.WndPiercing>(
    'WndPiercing',
    async () => (await import('./WndPiercing')).WndPiercing,
  ),
  WndRemovePiercing: new LazyWnd<typeof RemovePiercing.WndRemovePiercing>(
    'WndRemovePiercing',
    async () => (await import('./WndRemovePiercing')).WndRemovePiercing,
  ),
} as const;
