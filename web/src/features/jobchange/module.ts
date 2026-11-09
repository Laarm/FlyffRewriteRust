// The `jobchange` feature module (Plan/45 §21, §21.1; WP-CFEAT-33): the fast job change window (`APP_FASTJOBCHANGE`,
// `features.fast_jobchange`), the job transfer windows of the class-change scroll (`APP_CHANGECLASS_1/2`) and the presentation of a
// job change (`JobChanged`).
//
// The fast job change window opens by itself - after the join greeting, when an own level-up reaches 15 or 60, and when the
// experience bar reaches 99.99 % at level 120 - exactly where the server would accept an `UpdateJob`: the question is
// `rules.fastJobWindow(level, job, expPercentX100)` and the classes on offer are `rules.fastJobTargets(level, job)` (the strict table
// of Plan/16 §11.4, the same functions the server validates with). "Select Class" sends `UpdateJob{job}` and closes; the answer
// arrives as `JobChanged`, `SetExperience`, `SetState`, `TaskBar`.
//
// The class-change scroll (`II_SYS_SYS_SCR_CHACLA`) opens `APP_CHANGECLASS_1` (first jobs) or `APP_CHANGECLASS_2` (second jobs) after
// its confirm; nothing is sent (the legacy `SendChangeJob` reaches a server handler that does not exist) and a master or hero gets no
// window.

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { peekFxRuntime } from '../../game/fx';
import { levelUpEffect } from '../../game/fx/levelUp';
import { peekRulesRuntime } from '../../game/rules';
import { OBJMSG } from '../../game/combat/constants';
import { MTI_LEVELUP } from '../../gen/defs/neuz';
import {
  APP_BANK,
  APP_COMMON_BANK,
  APP_REPAIR,
  APP_SHOPEX,
  APP_TRADE,
} from '../../gen/defs/resdata';
import { BGM_IN_LEVELUP } from '../../gen/defs/sound';
import {
  TID_EVE_CHGJOB,
  TID_GAME_EQUALJOB,
  TID_GAME_REPAIR_NOTUSE,
  TID_GAME_TRADELIMITUSING,
} from '../../gen/defs/text';
import { UpdateJobSchema } from '../../gen/proto/flyff/v1/character_pb';
import { n64 } from '../../net/int64';
import type { RulesFacade } from '../../rules';
import { type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { sysChat } from '../common/text';
import { inventoryOf } from '../inventory';
import { classText } from './texts';

/** `ANILOOP_1PLAY` and `MOP_FIXED`: the level-up motion plays once. */
const ANILOOP_1PLAY = 1;
const MOP_FIXED = 0x2;

/** The public face of the job-change feature of a context. */
export interface JobChangeFeature {
  /** Opens the fast job change window now (the classes on offer from the rules; an empty list is the window's error state). */
  open(): boolean;
  /** Is the window open? */
  isOpen(): boolean;
  /** The classes the character may change to at its level and job (`rules.fastJobTargets`). */
  targets(): readonly number[];
  /** The class-change scroll: opens `APP_CHANGECLASS_1/2` after the checks of its confirm; `false` = no window for this job. */
  openChangeClass(): boolean;
  /** What the last `ok` of a class-change window chose (nothing is sent): for the probe and the tests. */
  readonly lastChangeClass: { readonly job: number } | null;
}

const features = new WeakMap<ClientContext, JobChangeFeature>();

/** The job-change feature of a context; `undefined` before the module registered. */
export function jobChangeOf(ctx: ClientContext): JobChangeFeature | undefined {
  return features.get(ctx);
}

export const jobChangeModule: FeatureModule = {
  id: 'jobchange',
  milestone: 5,
  register(fc) {
    const ctx = fc.app;
    const { selfStore } = ctx.stores;
    const facade = (): RulesFacade | null =>
      peekRulesRuntime(ctx)?.rules() ?? (ctx.rules as RulesFacade | null);
    const flag = (): boolean => ctx.features.features.fast_jobchange;
    const say = (tid: number): void => {
      sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid);
    };
    const jobName = (job: number): string => ctx.data?.job(job)?.name ?? '';
    let lastChangeClass: { job: number } | null = null;

    // ---- the fast job change window -----------------------------------------------------------------------------------------------

    const targets = (): readonly number[] => {
      const s = selfStore.getState();
      try {
        return Array.from(facade()?.fastJobTargets(s.level, s.job) ?? []);
      } catch {
        return [];
      }
    };
    /** `fastJobWindow(level, job, expPercentX100)`: does the window open for this state? */
    const windowOpens = (level: number, job: number, expPercentX100: number): boolean => {
      try {
        return facade()?.fastJobWindow(level, job, expPercentX100) ?? false;
      } catch {
        return false;
      }
    };
    /** The experience bar of a level and an experience (`GetExpPercent()`), 0 until the rules module is there. */
    const percent = (level: number, exp: number): number => {
      try {
        return facade()?.expPercentX100(level, exp) ?? 0;
      } catch {
        return 0;
      }
    };

    const open = (): boolean => {
      const ui = fullUi(ctx);
      if (!ui || !flag()) return false;
      if (ui.jobs.fastJobChange()) return true;
      const w = ui.jobs.openFastJobChange({
        targets: targets(),
        jobName,
        description: classText,
        apply: (job) => {
          // legacy-send: UpdateJob
          fc.net.send({ case: 'updateJob', value: create(UpdateJobSchema, { job }) });
        },
      });
      return w !== null;
    };

    // `CMover::SetExperience` / `OnGameJoin`: the three moments the window opens
    let level = -1;
    fc.net.on('spawnSelf', (m) => {
      level = m.player?.level ?? -1;
    });
    fc.net.on('addGameJoin', () => {
      const s = selfStore.getState();
      if (windowOpens(s.level, s.job, percent(s.level, s.exp))) open();
    });
    fc.net.on('setExperience', (m, objId) => {
      if (objId !== 0 && ctx.entities.get(objId)?.isSelf !== true) return;
      const before = level;
      level = m.level;
      const s = selfStore.getState();
      const pct = percent(m.level, n64(m.exp));
      const fullBar = m.level === 120 && pct === 9999;
      const levelUp = before >= 0 && m.level > before && (m.level === 15 || m.level === 60);
      if ((fullBar || levelUp) && windowOpens(m.level, s.job, pct)) open();
    });
    fc.net.on('setLevel', (m, objId) => {
      if (objId !== 0 && ctx.entities.get(objId)?.isSelf !== true) return;
      const before = level;
      level = m.level;
      const s = selfStore.getState();
      if (before >= 0 && m.level > before && (m.level === 15 || m.level === 60)) {
        if (windowOpens(m.level, s.job, 0)) open();
      }
    });

    // ---- the presentation of a job change (`OnSetChangeJob`, `OnSetNearChangeJob`) -------------------------------------------------

    const sfxAt = (objId: number, job: number): void => {
      const fx = peekFxRuntime(ctx)?.fx();
      if (!fx || objId === 0) return;
      fx.sfx(levelUpEffect(job, ctx.features.features.v21_levelup_sfx), { src: objId });
    };
    fc.net.on('jobChanged', (m, objId) => {
      const self = objId === 0 || ctx.entities.get(objId)?.isSelf === true;
      const ownId = ctx.entities.self()?.objId ?? 0;
      if (self) {
        // the window of the old job is void
        fullUi(ctx)?.jobs.fastJobChange()?.destroy();
        // the level-up motion when standing (the sim ignores it for a sitting or busy mover)
        ctx.sim.localAct(OBJMSG.MOTION, [MTI_LEVELUP, ANILOOP_1PLAY, MOP_FIXED, 0, 0]);
        sfxAt(ownId, m.job);
        ctx.audio.cues?.music(BGM_IN_LEVELUP, 1);
        sysChat(fc.texts, fullUi(ctx)?.chat ?? null, TID_EVE_CHGJOB, [jobName(m.job)]);
      } else {
        // an observer: the effect only (the level-up motion of another mover has no command here)
        sfxAt(objId, m.job);
      }
    });

    // ---- the class-change scroll -----------------------------------------------------------------------------------------------------

    /** `CWndCommItemDlg::OnChildNotify` (OK of the scroll's confirm): the trade and repair windows block it, then the window of the job. */
    const openChangeClass = (): boolean => {
      const ui = fullUi(ctx);
      if (!ui) return false;
      if ([APP_SHOPEX, APP_BANK, APP_COMMON_BANK, APP_TRADE].some((a) => ui.wm.isOpen(a))) {
        say(TID_GAME_TRADELIMITUSING);
        return false;
      }
      if (ui.wm.isOpen(APP_REPAIR)) {
        say(TID_GAME_REPAIR_NOTUSE);
        return false;
      }
      const w = ui.jobs.openChangeClass({
        job: selfStore.getState().job,
        select: (job) => {
          // nothing is sent: the legacy `SendChangeJob` reaches a server handler that does not exist (45 §21.1)
          lastChangeClass = { job };
        },
        equalJob: () => {
          say(TID_GAME_EQUALJOB);
        },
      });
      return w !== null;
    };
    // the scroll's double click (D6) opens the windows through the dispatch's seam
    inventoryOf(ctx)?.registerDispatchWindows({
      changeClass: () => {
        openChangeClass();
      },
    });

    const api: JobChangeFeature = {
      open,
      isOpen: () => fullUi(ctx)?.jobs.fastJobChange() != null,
      targets,
      openChangeClass,
      get lastChangeClass() {
        return lastChangeClass;
      },
    };
    features.set(ctx, api);

    addProbeMember(ctx, 'jobChange', () => {
      const w = fullUi(ctx)?.jobs.fastJobChange();
      const s = selfStore.getState();
      return {
        open: w != null,
        current: w?.current ?? null,
        targets: targets(),
        job: s.job,
        level: s.level,
        enabled: flag(),
        changeClass: fullUi(ctx)?.jobs.changeClass1()
          ? 1
          : fullUi(ctx)?.jobs.changeClass2()
            ? 2
            : 0,
        lastChangeClass,
      };
    });
  },
};
