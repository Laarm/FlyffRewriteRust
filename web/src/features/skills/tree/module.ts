// The `skills` feature module, skill-tree part (Plan/45 §19.1-§19.2; WP-CFEAT-31): the local editable copy of the 45 job skills
// (`skillTreeStore`), the ports of `APP_SKILL3` (`ui.skills.setPorts({tree})`), the finish with `rules.skillPlanCheck` and
// `DoUseSkillPointReq`, the server updates (`SpawnSelf`, `JobChanged`, `DoUseSkillPointNtf`, `InitSkillPoint`, `CmdSetSkillLevel`,
// `SetExperience`, `LegendSkillUpResult`) with their texts and effects, the skill tooltip, the applet / Esc policy of the skill window
// (`wm.skillTreeEsc`), the hero skill-up window (`MMI_LEGEND_SKILLUP`, the five gems) and the skill source of the taskbar.
//
// Other features reach it with `skillTreeOf(ctx)`: reads (`serverTable`, `levelOf`), the seam of the reuse delay for the tooltip and the
// seam of the skill-queue port.

import { create } from '@bufbuild/protobuf';
import type { StoreApi } from 'zustand/vanilla';
import type { ClientContext } from '../../../app/context';
import { peekFxRuntime } from '../../../game/fx';
import { levelUpEffect } from '../../../game/fx/levelUp';
import { peekRulesRuntime } from '../../../game/rules';
import {
  II_GEN_MAT_DIAMOND,
  II_GEN_MAT_EMERALD,
  II_GEN_MAT_RUBY,
  II_GEN_MAT_SAPPHIRE,
  II_GEN_MAT_TOPAZ,
} from '../../../gen/defs/item';
import { MMI_LEGEND_SKILLUP } from '../../../gen/defs/neuz';
import { BGM_IN_LEVELUP } from '../../../gen/defs/sound';
import { APP_HERO_SKILLUP, APP_QUEITMWARNING, APP_SKILL3 } from '../../../gen/defs/resdata';
import {
  TID_GAME_HEROSKILLUP_FAIL,
  TID_GAME_HEROSKILLUP_MAX,
  TID_GAME_HEROSKILLUP_SUCCESS,
  TID_GAME_REAPSKILL,
  TID_GAME_RECCURENCE,
  TID_GAME_SKILLLEVELUP,
  TID_GAME_SKILLLEVEL_CHANGE,
  TID_GAME_SKILLPOINT_UP,
  TID_GAME_WND_OVERLAPED,
  TID_RESKILLPOINT_ERROR,
} from '../../../gen/defs/text';
import {
  DoUseSkillPointReqSchema,
  LegendSkillUpStartSchema,
} from '../../../gen/proto/flyff/v1/skill_pb';
import { SkillSchema } from '../../../gen/proto/flyff/v1/mover_types_pb';
import type { RulesFacade } from '../../../rules';
import { skillPlanErrorCode, SKILL_PLAN_ERROR } from '../../../rules';
import type {
  HeroSkillUpRequest,
  SkillTreeActions,
  SkillTreeView,
} from '../../../ui/windows/skills/ports';
import { derivedView, type ViewSource } from '../../common/view';
import { type FeatureModule, fullUi } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { formatTemplate, sysChat, SYSTEM_CHAT } from '../../common/text';
import { hudOf } from '../../hud';
import { inventoryOf } from '../../inventory';
import { menuOf } from '../../npc';
import { taskbarOf, type TaskbarSkillSource } from '../../taskbar';
import {
  addPoint,
  checkSkill,
  hasEdits,
  NULL_ID,
  reset,
  select,
  subPoint,
  tableWords,
  type SkillTable,
  type TreeEnv,
  type TreeState,
} from './model';
import {
  applyCmdSetSkillLevel,
  applyDoUseSkillPoint,
  applyInitSkillPoint,
  applyJobChanged,
  applySetExperience,
  applySpawn,
} from './reducers';
import { defineSkillTreeStore, type SkillTreeStore } from './store';
import { createSkillComposer } from './tooltip';
import { buildTreeView, createTreeEnv, isLegend, playerOf } from './view';

/** The request-gate key of the finish (`DoUseSkillPointReq` until `DoUseSkillPointNtf`). */
export const FINISH_GATE = 'skillTree.finish';
/** How long the buttons wait for the answer of a finish before they come back (the server may refuse without an answer). */
export const FINISH_TIMEOUT_MS = 5000;
/** `NULL_CHGPARAM`: "no override" in the `chg` half of a DST view. */
const NULL_CHGPARAM = 0x7fff_ffff;
const DST_COUNT = 95;
/** The five gems of the hero skill-up window, in slot order. */
export const HERO_GEMS = [
  II_GEN_MAT_DIAMOND,
  II_GEN_MAT_EMERALD,
  II_GEN_MAT_SAPPHIRE,
  II_GEN_MAT_RUBY,
  II_GEN_MAT_TOPAZ,
] as const;

/** The public face of the skill-tree feature of a context. */
export interface SkillTreeFeature {
  readonly store: SkillTreeStore;
  readonly view: ViewSource<SkillTreeView>;
  readonly actions: SkillTreeActions;
  readonly env: TreeEnv;
  /** The table the server knows (`g_pPlayer->m_aJobSkill`). */
  serverTable(): SkillTable;
  /** The level the server knows of a skill, `undefined` when the table lacks it (`GetSkill`). */
  levelOf(skillId: number): number | undefined;
  /** The unspent edits of the window (`GetCurrSkillPoint() != m_nSkillPoint`). */
  hasUnspentEdits(): boolean;
  /** Opens (or closes) the skill window the way the applet does (K): refuses while the re-skill warning is open. */
  toggle(): void;
  /** `MMI_LEGEND_SKILLUP`: opens the hero skill-up window (`TID_GAME_WND_OVERLAPED` while it is open). */
  openHeroSkillUp(): void;
  /** The remaining reuse delay of a skill in ms for the tooltip (the skill runtime installs it). */
  setReuseSource(fn: ((skillId: number) => number) | null): void;
}

const features = new WeakMap<ClientContext, SkillTreeFeature>();

/** The skill-tree feature of a context; `undefined` before the module registered. */
export function skillTreeOf(ctx: ClientContext): SkillTreeFeature | undefined {
  return features.get(ctx);
}

export const skillTreeModule: FeatureModule = {
  id: 'skills',
  milestone: 5,
  register(fc) {
    const ctx = fc.app;
    const { selfStore } = ctx.stores;
    const store = defineSkillTreeStore(ctx.stores.registry);
    const facade = (): RulesFacade | null =>
      peekRulesRuntime(ctx)?.rules() ?? (ctx.rules as RulesFacade | null);
    const env = createTreeEnv({ data: () => ctx.data, rules: facade });
    const player = () => {
      const s = selfStore.getState();
      return playerOf(s.job, s.level);
    };
    const tree = (): TreeState => store.getState().tree;
    const say = (tid: number, args: readonly (string | number)[] = []): void => {
      sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid, args);
    };
    let reuse: ((skillId: number) => number) | null = null;

    const dataSource = {
      subscribe: (fn: () => void) =>
        ctx.data?.onBundle(() => {
          fn();
        }) ?? (() => undefined),
    };
    const view = derivedView(
      () =>
        buildTreeView({
          state: store.getState(),
          job: selfStore.getState().job,
          level: selfStore.getState().level,
          data: ctx.data,
          env,
        }),
      [store as StoreApi<unknown>, selfStore, dataSource],
      (a, b) => a === b,
    );

    // ---- sends -------------------------------------------------------------------------------------------------------------------

    /** `CWndReSkillWarning` Yes: the plan check of the rules, then `SendDoUseSkillPoint` (45 §19.1). */
    const submit = (): void => {
      const t = tree();
      if (!hasEdits(t)) return;
      const s = selfStore.getState();
      const rules = facade();
      if (rules) {
        let answer: number;
        try {
          answer = rules.skillPlanCheck(
            tableWords(t.server),
            tableWords(t.local),
            s.level,
            s.job,
            t.serverSp,
          );
        } catch {
          answer = 0;
        }
        const code = skillPlanErrorCode(answer);
        if (code !== null) {
          // `NothingToDo` is ignored; every other refusal is told
          if (code !== SKILL_PLAN_ERROR.NOTHING_TO_DO) say(TID_RESKILLPOINT_ERROR);
          return;
        }
      }
      if (!fc.gate.tryAcquire(FINISH_GATE)) return;
      finishAt = ctx.now();
      store.setState({ busy: true });
      // legacy-send: SendDoUseSkillPoint
      fc.net.send({
        case: 'doUseSkillPointReq',
        value: create(DoUseSkillPointReqSchema, {
          skills: t.local.map((x) => create(SkillSchema, { skillId: x.skillId, level: x.level })),
        }),
      });
    };
    let finishAt = 0;
    const release = (): void => {
      fc.gate.release(FINISH_GATE);
      if (store.getState().busy) store.setState({ busy: false });
    };

    // ---- the warning ---------------------------------------------------------------------------------------------------------------

    /**
     * `CWndReSkillWarning`: "the skill levels will change" with Yes / No. The warning of the close paths (the title bar, the applet,
     * Esc) closes the skill tree with itself (`InitItem(TRUE)`: `m_bParentDestroy`), the one of the Finish button does not.
     */
    const openWarning = (parentDestroy: boolean): void => {
      const ui = fullUi(ctx);
      if (!ui || ui.wm.isOpen(APP_QUEITMWARNING)) return;
      ui.dialogs.openQueItemWarning({
        text: fc.texts.text(TID_GAME_SKILLLEVEL_CHANGE),
        yes: () => {
          submit();
        },
        closed: () => {
          if (parentDestroy) ui.wm.get(APP_SKILL3)?.destroy();
        },
      });
    };

    // ---- effects ---------------------------------------------------------------------------------------------------------------------

    /** The level-up effect of the own character at its position (`__V21_LEVELUP_SFX`: the hero mark of the job line). */
    const levelUpSfx = (): void => {
      const fx = peekFxRuntime(ctx)?.fx();
      const src = ctx.entities.self()?.objId;
      if (!fx || src === undefined) return;
      fx.sfx(levelUpEffect(selfStore.getState().job, ctx.features.features.v21_levelup_sfx), {
        src,
      });
    };
    const levelUpMusic = (): void => {
      ctx.audio.cues?.music(BGM_IN_LEVELUP, 1);
    };

    // ---- the actions of the window -------------------------------------------------------------------------------------------

    const actions: SkillTreeActions = {
      select(index) {
        const next = select(tree(), index, env, player());
        if (next !== tree()) store.setState({ tree: next });
      },
      add() {
        if (store.getState().busy) return;
        const next = addPoint(tree(), env, player());
        if (next !== tree()) store.setState({ tree: next });
      },
      sub() {
        if (store.getState().busy) return;
        const next = subPoint(tree(), env);
        if (next !== tree()) store.setState({ tree: next });
      },
      reset() {
        if (store.getState().busy) return;
        // the original reloads only with unspent edits, and forgets the selection either way
        const t = tree();
        store.setState({ tree: reset(t) });
      },
      finish() {
        if (store.getState().busy || !hasEdits(tree())) return;
        openWarning(false);
      },
      queue(index) {
        const ui = fullUi(ctx);
        const taskbar = taskbarOf(ctx);
        const slot = tree().server[index];
        if (!ui || !taskbar?.ports.queue || !slot || slot.skillId === NULL_ID) return;
        // `g_pPlayer->CheckSkill`: the skill is usable in the server table; only while no skill queue runs
        if (!checkSkill(tree().server, index, env, player())) return;
        if (ui.taskbar.model.get().executing !== 0) return;
        const icon = ctx.data?.skill(slot.skillId)?.icon;
        taskbar.ports.queue.add(
          ui.taskbar.model.queueCount(),
          0,
          index,
          icon ? `icon/${icon}` : null,
        );
      },
      close() {
        // the title bar's close button (`nID == 10000`): unspent edits open the warning that closes the window with itself
        if (!hasEdits(tree())) return false;
        openWarning(true);
        return true;
      },
    };

    // ---- the tooltip ------------------------------------------------------------------------------------------------------------------

    const dstParam = (dst: number, base: number): number => {
      const id = selfStore.getState().objId;
      let v: Int32Array | undefined;
      try {
        v = facade()?.dstView(id);
      } catch {
        v = undefined;
      }
      if (!v || dst < 0 || dst >= DST_COUNT) return base;
      const chg = v[DST_COUNT + dst] ?? NULL_CHGPARAM;
      if (chg !== NULL_CHGPARAM) return chg;
      return base + (v[dst] ?? 0);
    };
    const composer = createSkillComposer({
      data: () => ctx.data,
      level: () => selfStore.getState().level,
      legend: () => isLegend(selfStore.getState().job),
      table: () => tree().server,
      param: dstParam,
      reuseMs: (id) => reuse?.(id) ?? 0,
    });

    // ---- the taskbar's view of the skills ----------------------------------------------------------------------------------------

    const taskbarSkills: TaskbarSkillSource = {
      info(type, id) {
        if (type !== 0) return null;
        const slot = tree().server[id];
        if (!slot || slot.skillId === NULL_ID) return null;
        const row = ctx.data?.skill(slot.skillId);
        if (!row) return null;
        return {
          level: slot.level,
          usable: checkSkill(tree().server, id, env, player()),
          combo: row.combo_style,
        };
      },
      icon(type, id) {
        if (type !== 0) return null;
        const slot = tree().server[id];
        const icon = slot ? ctx.data?.skill(slot.skillId)?.icon : undefined;
        return icon ? `icon/${icon}` : null;
      },
      tooltip(type, id) {
        if (type !== 0) return null;
        const slot = tree().server[id];
        return slot && slot.skillId !== NULL_ID
          ? { skillId: slot.skillId, level: slot.level }
          : null;
      },
    };

    // ---- the hero skill-up window ---------------------------------------------------------------------------------------------------

    const heroRequest = (): HeroSkillUpRequest => {
      const inv = inventoryOf(ctx);
      const iconOf = (itemId: number): string | null => {
        const icon = ctx.data?.item(itemId)?.icon;
        return icon ? `item/${icon}` : null;
      };
      return {
        jewelIds: HERO_GEMS,
        jewelIcons: HERO_GEMS.map(iconOf),
        description: heroDescription(),
        itemId: (objId) => inv?.item(objId)?.itemId,
        itemIcon: (objId) => {
          const it = inv?.item(objId);
          return it ? iconOf(it.itemId) : null;
        },
        reserve: (objId, on) => {
          const it = inv?.item(objId);
          // `SetExtra(GetExtra() + 1)` when a gem is put in, `SetExtra(0)` when it is taken out
          if (it) inv?.setExtra(objId, on ? it.extra + 1 : 0);
        },
        start: (objIds) => {
          // legacy-send: SendLegendSkillUp
          fc.net.send({
            case: 'legendSkillUpStart',
            value: create(LegendSkillUpStartSchema, { itemObjIds: [...objIds] }),
          });
        },
        result: (code) => {
          const tid =
            code === -1
              ? TID_GAME_HEROSKILLUP_MAX
              : code === 0
                ? TID_GAME_HEROSKILLUP_FAIL
                : code === 1
                  ? TID_GAME_HEROSKILLUP_SUCCESS
                  : 0;
          if (tid !== 0) fullUi(ctx)?.msgbox.open(fc.texts.text(tid));
        },
      };
    };
    const heroDescription = (): string => {
      const panels = (ctx.data?.bundle('help') as { panels?: Record<string, string> } | undefined)
        ?.panels;
      return panels?.['heroskillup'] ?? '';
    };
    const openHeroSkillUp = (): void => {
      const ui = fullUi(ctx);
      if (!ui) return;
      if (ui.skills.isHeroSkillUpOpen() || ui.wm.isOpen(APP_HERO_SKILLUP)) {
        say(TID_GAME_WND_OVERLAPED);
        return;
      }
      ui.skills.openHeroSkillUp(heroRequest());
    };

    // ---- the public face ---------------------------------------------------------------------------------------------------------

    const api: SkillTreeFeature = {
      store,
      view,
      actions,
      env,
      serverTable: () => tree().server,
      levelOf: (id) => tree().server.find((s) => s.skillId === id)?.level,
      hasUnspentEdits: () => hasEdits(tree()),
      toggle() {
        fullUi(ctx)?.wm.objectExecutorApplet(APP_SKILL3);
      },
      openHeroSkillUp,
      setReuseSource(fn) {
        reuse = fn;
      },
    };
    features.set(ctx, api);

    // ---- messages ----------------------------------------------------------------------------------------------------------------

    const isSelf = (objId: number): boolean =>
      objId === 0 || ctx.entities.get(objId)?.isSelf === true;
    const patchSelf = (skillPoints: number): void => {
      if (selfStore.getState().skillPoints !== skillPoints) selfStore.setState({ skillPoints });
    };

    fc.net.on('spawnSelf', (m) => {
      const self = m.self;
      store.setState(applySpawn(store.getState(), self?.jobSkills ?? [], self?.skillPoint ?? 0));
    });
    fc.net.on('jobChanged', (m, objId) => {
      if (!isSelf(objId)) return;
      store.setState(applyJobChanged(store.getState(), m.skills));
    });
    fc.net.on('doUseSkillPointNtf', (m) => {
      store.setState(applyDoUseSkillPoint(store.getState(), m.skills, m.skillPoint));
      patchSelf(m.skillPoint);
      release();
    });
    fc.net.on('initSkillPoint', (m) => {
      const before = store.getState().tree.serverSp;
      store.setState(applyInitSkillPoint(store.getState(), m.skillPoint, (id) => env.rule(id)));
      patchSelf(m.skillPoint);
      release();
      if (before < m.skillPoint) {
        levelUpSfx();
        levelUpMusic();
      }
      say(TID_GAME_RECCURENCE);
    });
    fc.net.on('cmdSetSkillLevel', (m) => {
      const next = applyCmdSetSkillLevel(store.getState(), m.skillId, m.level);
      if (!next) return;
      store.setState(next);
      const name = ctx.data?.skill(m.skillId)?.name;
      if (name !== undefined) {
        const text = formatTemplate(fc.texts.text(TID_GAME_SKILLLEVELUP), [name, m.level]);
        fullUi(ctx)?.chat.putString(
          text,
          null,
          fc.texts.textColor(TID_GAME_REAPSKILL),
          SYSTEM_CHAT,
        );
      }
    });
    // `SetSkillLevel` (another player's skill level) is read and ignored (KEEP, ref/09a §5.6)
    fc.net.on('setExperience', (m, objId) => {
      if (!isSelf(objId)) return;
      const before = store.getState().tree.serverSp;
      const next = applySetExperience(store.getState(), m.skillPoint);
      if (next !== store.getState()) store.setState(next);
      if (m.skillPoint > before && before >= 0 && store.getState().ready) {
        // `g_Caption1.AddCaption(TID_GAME_SKILLPOINT_UP)`; the level-up effect and jingle come with the level-up itself
        hudOf(ctx).caption(fc.texts.text(TID_GAME_SKILLPOINT_UP), false, ctx.now());
      }
    });
    fc.net.on('legendSkillUpResult', (m) => {
      fullUi(ctx)?.skills.heroSkillUp()?.receiveResult(m.result);
    });
    fc.net.on('definedText', (m) => {
      // a refusal of the finish comes as this text without a `DoUseSkillPointNtf`
      if (m.tid === TID_RESKILLPOINT_ERROR) release();
    });

    // the buttons come back when the server never answered the finish
    ctx.tickSystems.register('skills.finish', () => {
      if (store.getState().busy && ctx.now() - finishAt > FINISH_TIMEOUT_MS) release();
    });

    // ---- installation into the UI ----------------------------------------------------------------------------------------------

    const installed = new WeakSet<object>();
    const ensureUi = (): void => {
      const ui = fullUi(ctx);
      if (!ui || installed.has(ui)) return;
      installed.add(ui);
      ui.skills.setPorts({ tree: { view, actions } });
      ui.tooltip.registerComposers({ skill: composer });
      // the applet and Esc policy of the skill window (ref/22 §3.4): unspent edits open the warning instead of closing
      ui.wm.skillTreeEsc = {
        hasUnspentEdits: () => hasEdits(tree()),
        openWarning: () => {
          openWarning(true);
        },
      };
      taskbarOf(ctx)?.setSkills(taskbarSkills);
    };
    ctx.tickSystems.register('skills.ports', ensureUi);
    ctx.machine.subscribe(() => {
      ensureUi();
    });
    ensureUi();

    // the legend NPC opens the five-gem window
    menuOf(ctx)?.registerAction(MMI_LEGEND_SKILLUP, () => {
      openHeroSkillUp();
    });
    // the double click of a gem in the bag while the window is open
    inventoryOf(ctx)?.registerFeed(APP_HERO_SKILLUP, (item) => {
      return fullUi(ctx)?.skills.heroSkillUp()?.setJewel(item.objId) ?? false;
    });

    addProbeMember(ctx, 'skillTree', () => {
      const t = tree();
      const v = view.get();
      return {
        ready: store.getState().ready,
        busy: store.getState().busy,
        sp: t.localSp,
        serverSp: t.serverSp,
        selected: t.selected,
        edits: hasEdits(t),
        open: fullUi(ctx)?.wm.isOpen(APP_SKILL3) ?? false,
        sections: fullUi(ctx)?.skills.tree()?.sections ?? null,
        skills: v.slots.map((s) => ({
          index: s.index,
          id: s.skillId,
          level: s.level,
          serverLevel: s.serverLevel,
          learnable: s.learnable,
          name: ctx.data?.skill(s.skillId)?.name ?? '',
        })),
        buttons: v.buttons,
        // the centre of every drawn icon in CSS pixels (the e2e clicks there)
        points: v.slots.flatMap((s) => {
          const r = fullUi(ctx)?.skills.tree()?.iconScreenRect(s.index);
          if (!r) return [];
          const scale = fullUi(ctx)?.scaler.get().scale ?? 1;
          const root = document.querySelector('[data-testid="ui-root"]')?.getBoundingClientRect();
          return [
            {
              index: s.index,
              x: Math.round((root?.left ?? 0) + ((r.l + r.r) / 2) * scale),
              y: Math.round((root?.top ?? 0) + ((r.t + r.b) / 2) * scale),
            },
          ];
        }),
      };
    });
  },
};
