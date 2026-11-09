// The lord windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports these shells dynamically. The skill window (`WndLordSkill`) is a plain class: the applet registry builds it
// synchronously. One class per line (the vitest transform `lazyWndSync` reads this file line by line).
import { LazyWnd } from '../../wnd/lazy';

import type * as State from './WndLordState';
import type * as Tender from './WndLordTender';
import type * as Pledge from './WndLordPledge';
import type * as Vote from './WndLordVote';
import type * as Event from './WndLordEvent';
import type * as SkillConfirm from './WndLordSkillConfirm';
import type * as Confirm from './WndLordConfirm';
import type * as Info from './WndLordInfo';
import type * as RPInfo from './WndLordRPInfo';

export const LORD_LAZY = {
  WndLordState: new LazyWnd<typeof State.WndLordState>(
    'WndLordState',
    async () => (await import('./WndLordState')).WndLordState,
  ),
  WndLordTender: new LazyWnd<typeof Tender.WndLordTender>(
    'WndLordTender',
    async () => (await import('./WndLordTender')).WndLordTender,
  ),
  WndLordPledge: new LazyWnd<typeof Pledge.WndLordPledge>(
    'WndLordPledge',
    async () => (await import('./WndLordPledge')).WndLordPledge,
  ),
  WndLordVote: new LazyWnd<typeof Vote.WndLordVote>(
    'WndLordVote',
    async () => (await import('./WndLordVote')).WndLordVote,
  ),
  WndLordEvent: new LazyWnd<typeof Event.WndLordEvent>(
    'WndLordEvent',
    async () => (await import('./WndLordEvent')).WndLordEvent,
  ),
  WndLordSkillConfirm: new LazyWnd<typeof SkillConfirm.WndLordSkillConfirm>(
    'WndLordSkillConfirm',
    async () => (await import('./WndLordSkillConfirm')).WndLordSkillConfirm,
  ),
  WndLordConfirm: new LazyWnd<typeof Confirm.WndLordConfirm>(
    'WndLordConfirm',
    async () => (await import('./WndLordConfirm')).WndLordConfirm,
  ),
  WndLordInfo: new LazyWnd<typeof Info.WndLordInfo>(
    'WndLordInfo',
    async () => (await import('./WndLordInfo')).WndLordInfo,
  ),
  WndLordRPInfo: new LazyWnd<typeof RPInfo.WndLordRPInfo>(
    'WndLordRPInfo',
    async () => (await import('./WndLordRPInfo')).WndLordRPInfo,
  ),
} as const;
