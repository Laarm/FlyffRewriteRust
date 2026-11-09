// The job windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-38): the lazy classes of `lazy.ts`.
import type { WndCtor } from '../../catalog';
import { JOBS_LAZY } from './lazy';

export const JOB_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_FASTJOBCHANGE: () => JOBS_LAZY.WndFastJobChange.load(),
  APP_CHANGECLASS_1: () => JOBS_LAZY.WndChangeClass1.load(),
  APP_CHANGECLASS_2: () => JOBS_LAZY.WndChangeClass2.load(),
};
