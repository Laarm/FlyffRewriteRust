// The pet windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports these shells dynamically. The pet filter is an APPLET (the registry builds it
// synchronously), so its class is a plain import of `index.ts`.
import { LazyWnd } from '../../wnd/lazy';
import type * as BuffPetStatus from './WndBuffPetStatus';
import type * as FoodConfirm from './WndFoodConfirm';
import type * as FoodMill from './WndPetFoodMill';
import type * as Miracle from './WndPetMiracle';
import type * as PetRes from './WndPetRes';
import type * as PetStatus from './WndPetStatus';
import type * as TransEggs from './WndPetTransEggs';

export const PETS_LAZY = {
  WndPetStatus: new LazyWnd<typeof PetStatus.WndPetStatus>(
    'WndPetStatus',
    async () => (await import('./WndPetStatus')).WndPetStatus,
  ),
  WndFoodConfirm: new LazyWnd<typeof FoodConfirm.WndFoodConfirm>(
    'WndFoodConfirm',
    async () => (await import('./WndFoodConfirm')).WndFoodConfirm,
  ),
  WndPetFoodMill: new LazyWnd<typeof FoodMill.WndPetFoodMill>(
    'WndPetFoodMill',
    async () => (await import('./WndPetFoodMill')).WndPetFoodMill,
  ),
  WndPetMiracle: new LazyWnd<typeof Miracle.WndPetMiracle>(
    'WndPetMiracle',
    async () => (await import('./WndPetMiracle')).WndPetMiracle,
  ),
  WndPetTransEggs: new LazyWnd<typeof TransEggs.WndPetTransEggs>(
    'WndPetTransEggs',
    async () => (await import('./WndPetTransEggs')).WndPetTransEggs,
  ),
  WndPetRes: new LazyWnd<typeof PetRes.WndPetRes>(
    'WndPetRes',
    async () => (await import('./WndPetRes')).WndPetRes,
  ),
  WndBuffPetStatus: new LazyWnd<typeof BuffPetStatus.WndBuffPetStatus>(
    'WndBuffPetStatus',
    async () => (await import('./WndBuffPetStatus')).WndBuffPetStatus,
  ),
} as const;
