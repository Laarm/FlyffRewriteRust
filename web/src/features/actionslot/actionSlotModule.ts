// The `actionslot` feature (Plan/45 §19.3, §19.4; WP-CFEAT-32, with the skill side of WP-SKILL-12 / WP-INPUT-13, -14): everything that
// connects the skill runtime of 17 (`game/combat/skills.ts`) with the taskbar of 42:
//
//   * the skill source of the taskbar: icon, drop facts (`CheckSkill`, combo style, cool-down), tooltip and the cool-down radar of a
//     skill shortcut (`RenderCollTime`)
//   * the shortcut ports: an F-key / slot click on a skill is `useSkillShortcut`, the action-slot shortcut (SHORTCUT_SKILLFUN) is the
//     `C` key (`nextSkill = ACTIONSLOT`)
//   * the action-queue row: the rules (`checkAddSkill`, the `SkillTaskBar` send), the refusal line, the action-point display
//     (`SetActionPoint`, the shade over the cells, the cost hints) and the execution state mirrored into the taskbar model
//   * the chat lines of the skill runtime (`TID_GAME_SKILL_SELF` / `TID_GAME_SKILL_MATE`)
//
// Other features reach it with `actionSlotOf(ctx)`: the party-skill port (`partySkillUse`), the skill window's double click
// (`useSkillShortcut`).

import type { ClientContext } from '../../app/context';
import { skillsRuntime, type SkillRuntime } from '../../game/combat/skills';
import { isNullNumber } from '../../game/combat/skillProps';
import { wallNow } from '../../platform/time';
import type { SkillDropInfo } from '../../ui/taskbar/ports';
import { MAX_SLOT_ITEM, MAX_SLOT_QUEUE } from '../../ui/taskbar/layout';
import { queueApCost } from '../../ui/taskbar/queueView';
import { type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { SYSTEM_CHAT, sysChat } from '../common/text';
import { SM } from '../buffs/sm';
import { isHeroJob, isMasterJob } from '../hud/targetFrame/model';
import { queueActions } from '../skills/queue';
import { skillTreeOf } from '../skills/tree';
import { taskbarOf, type TaskbarSkillSource } from '../taskbar/taskbarModule';

/** The public face of the feature. */
export interface ActionSlotFeature {
  readonly runtime: SkillRuntime;
  /** `CWndMgr::UseSkillShortCut(0, slot)`. */
  useSkillShortcut(slot: number): void;
  /** `CWndWorld::UseSkill()` (the C key / the action-slot shortcut): the queue starts on the next tick. */
  useActionSlot(): void;
  /** `CMover::CheckSkill(skill)`: the skill is learnable (the drop rule of the taskbar). */
  checkSkill(skillId: number): boolean;
  /** The party skills of the troupe (type 2 shortcuts) are the party feature's: it registers its executor here. */
  partySkillUse: ((id: number) => void) | null;
}

const features = new WeakMap<ClientContext, ActionSlotFeature>();

/** The action-slot feature of a context; `undefined` before the module registered. */
export function actionSlotOf(ctx: ClientContext): ActionSlotFeature | undefined {
  return features.get(ctx);
}

export const actionSlotModule: FeatureModule = {
  id: 'skills',
  milestone: 5,
  register(fc) {
    const ctx = fc.app;
    const rt = skillsRuntime(ctx);
    const { selfStore } = ctx.stores;
    const nowUnix = (): number => Math.floor(wallNow() / 1000);
    let partySkillUse: ((id: number) => void) | null = null;

    /** `CMover::CheckSkill(dwSkill)`: the skill is learnable (`nLog != 1`, the level requirement, the prerequisites). */
    const checkSkill = (skillId: number): boolean => {
      const prop = rt.props.prop(skillId);
      if (!prop || prop.log === 1) return false;
      const self = selfStore.getState();
      const exempt = isMasterJob(self.job) || isHeroJob(self.job);
      if (!exempt && self.level < prop.req_dis_lv && !isNullNumber(prop.req_dis_lv)) return false;
      for (const [req, lv] of [
        [prop.re_skill1, prop.re_skill_level1],
        [prop.re_skill2, prop.re_skill_level2],
      ] as const) {
        if (isNullNumber(req)) continue;
        const slot = rt.slotOf(req);
        const have = slot >= 0 ? rt.table[slot] : undefined;
        // a prerequisite the character does not have is not checked (`if( pSkillBuf )`)
        if (have && have.level < lv) return false;
      }
      return true;
    };

    // ---- the skill source of the taskbar -----------------------------------------------------------------------------
    const info = (type: number, id: number): SkillDropInfo | null => {
      if (type === 2) {
        const p = ctx.data?.partySkill(id);
        return p ? { level: 1, usable: true, combo: p.combo_style, name: p.name } : null;
      }
      const s = rt.skillAt(id);
      if (!s) return null;
      const prop = rt.props.prop(s.skillId);
      if (!prop) return null;
      const add = s.level > 0 ? rt.props.add(prop, s.level) : undefined;
      const cooldown = add && !isNullNumber(add.cooldown) ? add.cooldown : 0;
      return {
        level: s.level,
        usable: checkSkill(s.skillId),
        combo: prop.combo_style,
        cooldown,
        name: prop.name,
      };
    };
    const source: TaskbarSkillSource = {
      info,
      icon: (type, id) => {
        if (type === 2) {
          const p = ctx.data?.partySkill(id);
          return p && p.icon !== '' ? `icon/${p.icon}` : null;
        }
        const s = rt.skillAt(id);
        const prop = s ? rt.props.prop(s.skillId) : undefined;
        // the skill icons live in `Icon/` (204 of them; `Item/` has the nine event skills): the load path and the queue of the original
        return prop && prop.icon !== '' ? `icon/${prop.icon}` : null;
      },
      tooltip: (type, id) => {
        if (type === 2) return { skillId: id, level: 1 };
        const s = rt.skillAt(id);
        return s ? { skillId: s.skillId, level: s.level } : null;
      },
      cooldown: (type, id) => (type === 2 ? null : rt.cooldown(id)),
    };

    const tb = taskbarOf(ctx);
    // the basic source is the skill tree's (it registers when its window ports are installed); this one is the fallback, and the
    // additions (cool-down radar, queue drop facts) are independent of whichever source is installed last
    tb?.setSkills(source);
    tb?.setSkillExtras({
      cooldown: (type, id) => (type === 2 ? null : rt.cooldown(id)),
      dropInfo: (type, id) => {
        if (type === 2) return null;
        const full = info(type, id);
        return full ? { cooldown: full.cooldown ?? 0, name: full.name ?? '' } : null;
      },
    });

    const api: ActionSlotFeature = {
      runtime: rt,
      useSkillShortcut: (slot) => {
        ctx.input.skills?.useSkillShortcut(slot);
      },
      useActionSlot: () => {
        ctx.input.skills?.useActionSlot();
      },
      checkSkill,
      get partySkillUse() {
        return partySkillUse;
      },
      set partySkillUse(fn) {
        partySkillUse = fn;
      },
    };
    features.set(ctx, api);

    // ---- the shortcut ports ---------------------------------------------------------------------------------------------
    tb?.setShortcutPorts({
      skills: {
        useShortcut: (type, id) => {
          if (type === 2) partySkillUse?.(id);
          else api.useSkillShortcut(id);
        },
      },
      actionSlot: {
        use: () => {
          api.useActionSlot();
        },
      },
    });

    // ---- the action-queue row: the rules, the lines, the points ------------------------------------------------------------
    const apFree = (): boolean => {
      const now = nowUnix();
      return selfStore.getState().smTimers.some((t) => t.kind === SM.ACTPOINT && t.endUnix > now);
    };
    tb?.setQueueActions(queueActions(ctx));
    tb?.setQueueHooks({
      messageFmt: (tid, ...args) => {
        sysChat(fc.texts, fc.chat, tid, args);
      },
      actionPointsFree: apFree,
      animating: () => {
        for (let slot = 0; slot < rt.table.length; slot += 1) {
          if (rt.cooldown(slot) !== null) return true;
        }
        return false;
      },
      queueTooltip: (ap, slot, cost) => {
        const base = `Action points: ${String(ap)} / 100`;
        return slot > 0 && !apFree() && cost > 0
          ? `${base}\nUsing this skill costs ${String(queueApCost(slot))}`
          : base;
      },
    });

    // the reuse delay line of the skill tooltip (`GetReuseDelay`): the skill tree registers after this module, so the source is installed by the
    // first tick that finds the feature
    let reuseInstalled = false;
    ctx.tickSystems.register('actionslot.reuse', () => {
      if (reuseInstalled) return;
      const tree = skillTreeOf(ctx);
      if (!tree) return;
      reuseInstalled = true;
      tree.setReuseSource((skillId) => {
        const slot = rt.slotOf(skillId);
        const c = slot < 0 ? null : rt.cooldown(slot);
        return c ? Math.max(0, c.endMs - ctx.now()) : 0;
      });
    });

    // the chat lines of the runtime
    rt.hooks.chatLine = (text, tid) => {
      fc.chat?.putString(text, null, fc.texts.textColor(tid), SYSTEM_CHAT);
    };

    // the join batch carries the points (`TaskBar.action_point`); `SetActionPoint` follows them
    fc.net.on('taskBar', (m) => {
      rt.actionSlot.actionPoint = Math.max(0, Math.min(100, m.actionPoint));
      sync();
    });

    // 42's `ActionQueue` is the queue port of the feature once the UI exists (the skill window's double click adds through it)
    const installQueue = (): void => {
      const ui = fullUi(ctx);
      if (ui) tb?.setQueue(ui.taskbar.queue);
    };
    ctx.tickSystems.register('actionslot.queue', installQueue);
    installQueue();

    // ---- the taskbar model mirrors the runtime ----------------------------------------------------------------------------------
    const sync = (): void => {
      const model = fullUi(ctx)?.taskbar.model;
      if (!model) return;
      model.setActionPoints(rt.actionSlot.actionPoint);
      model.setExecuting(rt.actionSlot.executing, rt.actionSlot.usedIndex);
    };
    rt.subscribe(sync);
    ctx.tickSystems.register('actionslot.sync', sync);

    // the e2e reads the state instead of pixels (`__flyffDebug.features.actionslot()`): the cast, the cool-downs, the action points, the
    // queue state, and the centre of every taskbar cell in CSS pixels (the specs drag skills there and press the keys of the cells)
    addProbeMember(ctx, 'actionslot', () => {
      const now = ctx.now();
      const ui = fullUi(ctx);
      const scale = ui?.scaler.get().scale ?? 1;
      const root =
        typeof document === 'undefined'
          ? null
          : document.querySelector('[data-testid="ui-root"]')?.getBoundingClientRect();
      const centre = (
        row: 'item' | 'queue',
        slot: number,
      ): { slot: number; x: number; y: number } | null => {
        const r = ui?.taskbar.window?.slotScreenRect({ row, slot });
        if (!r) return null;
        return {
          slot,
          x: Math.round((root?.left ?? 0) + ((r.l + r.r) / 2) * scale),
          y: Math.round((root?.top ?? 0) + ((r.t + r.b) / 2) * scale),
        };
      };
      const cast = rt.cast;
      return {
        executing: rt.actionSlot.executing,
        usedIndex: rt.actionSlot.usedIndex,
        actionPoint: rt.actionSlot.actionPoint,
        cast: cast
          ? {
              skillId: cast.skillId,
              level: cast.level,
              slot: cast.slot,
              useType: cast.useType,
              elapsedMs: now - cast.startMs,
              totalMs: cast.endMs - cast.startMs,
            }
          : null,
        cooldowns: rt.table.flatMap((s, slot) => {
          const c = rt.cooldown(slot, now);
          return c
            ? [
                {
                  slot,
                  skillId: s.skillId,
                  totalMs: c.endMs - c.startMs,
                  remainingMs: c.endMs - now,
                },
              ]
            : [];
        }),
        stats: { ...rt.stats },
        pending: rt.pending,
        items: Array.from({ length: MAX_SLOT_ITEM }, (_, i) => centre('item', i)).filter(
          (c) => c !== null,
        ),
        queue: Array.from({ length: MAX_SLOT_QUEUE }, (_, i) => centre('queue', i)).filter(
          (c) => c !== null,
        ),
      };
    });
  },
};
