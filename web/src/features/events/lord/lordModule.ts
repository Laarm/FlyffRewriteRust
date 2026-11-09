// The `lord` feature (Plan/45 §28.2; WP-CFEAT-43; 22 §9; ref/09b §3.16, §7.11): the lord, the election, the lord events and the lord skills.
//
//   12 server messages          `eventStore.lord`, one pure reducer per message (`reducers.ts`); every message is applied whether or not a window is open (PL-124)
//   windows                     the election, the event window, the skill applet with its name prompt and the information windows: ports over `ui.lord` (`ui/windows/lord`)
//   menu                        the lord entries of the NPC menu, ids 91-94, 101, 102 (`menu.ts`)
//   senders                     `SendElectionAddDeposit`, `SendElectionSetPledge`, `SendElectionIncVote`, `SendLEventCreate`, `SendLordSkillUse` (`senders.ts`)
//   lord skills                 `lord.useSkill( id )` (the focus rules, the name prompt), the taskbar source (icon, radar, tooltip) and the clean-up of the lord shortcuts (`skills.ts`)
//   seams                       `lordOf( ctx )` for the other features, the probe members `lord`, `lordPoints` and `gc1to1Windows`
//
// The lord-skill effects (`src_sfx` / `dst_sfx`) go through the fx runtime; their rendering belongs to the render task (WP-REND-27 verifies them). The caption tint 0xFFFFFF99 of the lord
// events is not supported by `hud.caption`: the default colour (RESUME item).

import type { ClientContext } from '../../../app/context';
import { entityInfoOf, selfOf } from '../../../app/inputHost';
import { EntityKind } from '../../../game/entities';
import { OBJMSG } from '../../../game/combat/constants';
import { peekFxRuntime } from '../../../game/fx';
import type { LordSkill } from '../../../gen/bundles';
import { TID_GAME_STILLNOTUSE, TID_TOOLTIP_COOLTIME } from '../../../gen/defs/text';
import type { InputInstance } from '../../../input/input';
import { headingDegrees } from '../../../input/skill';
import { SC } from '../../../ui/dnd/shortcut';
import type { LordActions, LordPorts, LordView, ViewSource } from '../../../ui/windows/lord/ports';
import { fullUi, type FeatureContext } from '../../common/module';
import { playerDataOf } from '../../common/playerData';
import { formatTid, SYSTEM_CHAT } from '../../common/text';
import { derivedView } from '../../common/view';
import { hudOf } from '../../hud/hudModule';
import { menuOf } from '../../npc/menuModule';
import { deepEqual } from '../../party/view';
import { taskbarOf } from '../../taskbar';
import { type EventFeature, eventOf } from '../api';
import { registerEvents } from '../eventsModule';
import type { EventStore } from '../store';
import { menuTable } from './menu';
import { registerLordProbe } from './probe';
import {
  type LordEffect,
  type LordEnv,
  reduceAddDeposit,
  reduceBeginCandidacy,
  reduceBeginVote,
  reduceEndVote,
  reduceEventCreate,
  reduceEventInitialize,
  reduceEventTick,
  reduceIncVote,
  reduceLord,
  reduceSetPledge,
  reduceSkillTick,
  reduceSkillUse,
  type Reduced,
} from './reducers';
import { lordSenders, type LordSenders } from './senders';
import {
  cooldownSpan,
  cooltimeLine,
  decideUseSkill,
  type LordFocus,
  lordSlots,
  slotTooltipText,
} from './skills';
import { initialLord, type LordSlice } from './slice';
import { lordView, type LordViewFacts, skillIconPath } from './view';

/** The oneof cases of the 12 server messages of `lord.proto` (a new proto case fails `every_lord_message_has_a_handler`). */
export const LORD_CASES = [
  'lord',
  'electionAddDepositNtf',
  'electionSetPledgeNtf',
  'electionIncVoteNtf',
  'electionBeginCandidacy',
  'electionBeginVote',
  'electionEndVote',
  'lEventCreateNtf',
  'lEventInitialize',
  'lEventTick',
  'lordSkillTick',
  'lordSkillUseNtf',
] as const;

/** `MTI_ATK1`, `ANILOOP_1PLAY` and `MOP_SWDFORCE | MOP_NO_TRANS | MOP_FIXED` of the casting lord. */
const MTI_ATK1 = 23;
const ANILOOP_1PLAY = 1;
const MOP_CAST = 1 | 4 | 2;

/** The public face of the lord feature of a context. */
export interface LordFeature {
  readonly store: EventStore;
  readonly senders: LordSenders;
  readonly view: ViewSource<LordView>;
  /** The slice now. */
  slice(): LordSlice;
  /** `CCLord::Get()`, `null` for none. */
  lord(): number | null;
  /** The own character is the lord. */
  isLord(): boolean;
  /** `ObjectExecutor( SHORTCUT_LORDSKILL, id )` with the stored `LI_*` id. */
  useSkill(id: number): void;
}

const features = new WeakMap<ClientContext, LordFeature>();

/** The lord feature of a context; `undefined` before the module registered. */
export function lordOf(ctx: ClientContext): LordFeature | undefined {
  return features.get(ctx);
}

/** Registers the lord (after the events feature: `lordM7.ts` registers both under the one handler module `events.lord`). */
export function registerLord(fc: FeatureContext): LordFeature {
  const ctx = fc.app;
  const { selfStore } = ctx.stores;
  const events: EventFeature = eventOf(ctx) ?? registerEvents(fc);
  const store = events.store;
  const playerData = playerDataOf(ctx);
  const senders = lordSenders((msg) => ctx.net.sender.send(msg));
  const text = (tid: number): string => data()?.text?.(tid) ?? fc.texts.text(tid);
  const slice = (): LordSlice => store.getState().lord;
  // the data index may lack its bundle reader in a partial test context: read it as optional
  const data = (): Partial<NonNullable<ClientContext['data']>> | null => ctx.data;
  const skillRows = (): readonly LordSkill[] => data()?.bundle?.('tables')?.lord_skills ?? [];
  const selfChar = (): number => selfStore.getState().idPlayer;
  const posOf = entityInfoOf(ctx);

  // ---- the facts the reducers and the windows read -----------------------------------------------------------------------------

  const moverOfCharacter = (charId: number): number | null => {
    if (charId === selfChar()) return selfStore.getState().objId;
    let out: number | null = null;
    ctx.entities.forEach(EntityKind.Player, (rec) => {
      if (!rec.removed && rec.idPlayer === charId) out = rec.objId;
    });
    return out;
  };
  const env = (): LordEnv => ({
    now: ctx.now(),
    selfCharId: selfChar(),
    selfObjId: selfStore.getState().objId,
    name: (id) => playerData.nameOf(id),
    skill: (id) => {
      const s = skillRows().find((x) => x.id === id);
      return s ? { cooltime: s.cooltime, srcSfx: s.src_sfx, dstSfx: s.dst_sfx } : null;
    },
    hasObject: (objId) => {
      const rec = ctx.entities.get(objId);
      return rec !== undefined && !rec.removed;
    },
    moverOfCharacter,
  });

  const viewFacts = (): LordViewFacts => {
    const t = data()?.bundle?.('tables');
    return {
      selfId: selfChar(),
      player: (id) => {
        const e = playerData.get(id);
        return e ? { level: e.level, job: e.job } : undefined;
      },
      nameOf: (id) => playerData.nameOf(id),
      jobName: (job) => data()?.job?.(job)?.name ?? '',
      skills: t?.lord_skills ?? [],
      expRates: t?.lord_events.exp ?? [],
      itemRates: t?.lord_events.item ?? [],
      selectLord: ctx.features.features.select_lord,
      playerLordEvent: ctx.features.features.player_lordevent,
      maxLordEvent: ctx.features.gameplay.max_lord_event,
    };
  };
  const view = derivedView(
    () => lordView(slice(), viewFacts()),
    [store, playerData.store],
    deepEqual,
  );

  // ---- the panels of the lazy help bundle --------------------------------------------------------------------------------------

  const withPanels = (fn: () => void): void => {
    if (data()?.bundle?.('help')) {
      fn();
      return;
    }
    ctx.bundles.ensure('help').then(fn, fn);
  };
  const panel = (key: string): string => data()?.bundle?.('help')?.panels[key] ?? '';

  // ---- the lord skills ----------------------------------------------------------------------------------------------------------

  const focus = (): LordFocus => {
    const id = (ctx.input as Partial<InputInstance>).core?.targeting.focus ?? null;
    if (id === null) return { kind: 'none' };
    const rec = ctx.entities.get(id);
    if (!rec || rec.removed) return { kind: 'none' };
    return rec.kind === EntityKind.Player
      ? { kind: 'player', name: rec.visual?.name ?? '' }
      : { kind: 'other' };
  };
  const say = (tid: number): void => {
    const chat = fc.chat;
    if (chat) chat.putString(text(tid), null, fc.texts.textColor(tid), SYSTEM_CHAT);
  };
  const executeSkill = (id: number): void => {
    const d = decideUseSkill(id, {
      hasSkill: (i) => skillRows().some((s) => s.id === i),
      tick: (i) => slice().skillTicks[i] ?? 0,
      focus,
      selfName: () => selfStore.getState().name,
    });
    switch (d.kind) {
      case 'confirm':
        fullUi(ctx)?.lord.openSkillConfirm(d.skill);
        break;
      case 'stillNotUse':
        say(TID_GAME_STILLNOTUSE);
        break;
      case 'send':
        senders.useSkill(d.skill, d.name);
        break;
      case 'nothing':
        break;
    }
  };

  // ---- the taskbar -------------------------------------------------------------------------------------------------------------

  const skillView = (id: number) => view.get().skills.find((s) => s.id === id);
  taskbarOf(ctx)?.setShortcutPorts({
    lord: {
      useSkill: executeSkill,
      icon: (id) => {
        const row = skillRows().find((s) => s.id === id);
        return row ? skillIconPath(row.icon) : null;
      },
      cooldown: (id) => {
        const s = skillView(id);
        return s ? cooldownSpan(s, slice().skillTicksAt) : null;
      },
      tooltip: (id) => {
        const s = skillView(id);
        return s
          ? slotTooltipText(s, (tick) => cooltimeLine(text(TID_TOOLTIP_COOLTIME), tick))
          : null;
      },
    },
  });
  /** `DoEndVoteComplete`: the LORDSKILL slots of the item pages, then of the applet row, are removed on the server and emptied in the model. */
  const cleanShortcuts = (): void => {
    const ui = fullUi(ctx);
    const actions = taskbarOf(ctx)?.ports.actions;
    if (!ui || !actions) return;
    const model = ui.taskbar.model;
    const state = model.get();
    const isLord = (row: 'item' | 'applet', page: number, slot: number): boolean =>
      (row === 'item' ? state.items[page]?.[slot] : state.applets[slot])?.kind === SC.LORDSKILL;
    for (const ref of lordSlots(isLord)) {
      if (ref.row === 'item') {
        actions.removeItem(ref.page, ref.slot);
        model.setItem(ref.page, ref.slot, null);
      } else {
        actions.removeApplet(ref.slot);
        model.setApplet(ref.slot, null);
      }
    }
  };
  const repaintTaskbar = (): void => {
    fullUi(ctx)?.taskbar.window?.invalidate();
  };

  // ---- carrying out the effects of a reducer ------------------------------------------------------------------------------------

  const run = (effects: readonly LordEffect[]): void => {
    for (const e of effects) {
      switch (e.kind) {
        case 'box':
          fullUi(ctx)?.msgbox.open(formatTid(fc.texts, e.tid, e.args));
          break;
        case 'boxUpper':
          fullUi(ctx)?.msgbox.openUpper(formatTid(fc.texts, e.tid, e.args));
          break;
        case 'chat':
          fc.chat?.putString(
            formatTid(fc.texts, e.tid, e.args),
            null,
            fc.texts.textColor(e.colorTid ?? e.tid),
            e.style ?? SYSTEM_CHAT,
          );
          break;
        case 'caption':
          hudOf(ctx).caption(formatTid(fc.texts, e.tid, e.args), e.small, ctx.now());
          break;
        case 'need':
          playerData.need(e.id);
          break;
        case 'needAll':
          playerData.needAll(e.ids);
          break;
        case 'close':
          fullUi(ctx)?.wm.get(e.app)?.destroy();
          break;
        case 'refreshTender':
          fullUi(ctx)?.lord.tender()?.refreshDeposit();
          break;
        case 'cleanShortcuts':
          cleanShortcuts();
          break;
        case 'sfx': {
          const fx = peekFxRuntime(ctx)?.fx();
          const pos = posOf(e.objId)?.pos;
          fx?.sfx(e.xi, { src: e.objId, ...(pos ? { pos } : {}) });
          break;
        }
        case 'localMotion': {
          if (e.faceObjId !== null) {
            const target = posOf(e.faceObjId)?.pos;
            const me = selfOf(ctx)()?.pos;
            if (target && me) {
              ctx.sim.cmd.localFace(headingDegrees(target[0] - me[0], target[2] - me[2]));
            }
          }
          ctx.sim.localAct(OBJMSG.MOTION, [MTI_ATK1, ANILOOP_1PLAY, MOP_CAST, 0, 0]);
          break;
        }
      }
    }
  };
  const apply = (r: Reduced): void => {
    if (r.slice !== slice()) store.setState({ lord: r.slice });
    run(r.effects);
  };

  fc.net.on('lord', (m) => {
    apply(reduceLord(slice(), m, env()));
  });
  fc.net.on('electionAddDepositNtf', (m) => {
    apply(reduceAddDeposit(slice(), m, env()));
  });
  fc.net.on('electionSetPledgeNtf', (m) => {
    apply(reduceSetPledge(slice(), m));
  });
  fc.net.on('electionIncVoteNtf', (m) => {
    apply(reduceIncVote(slice(), m, env()));
  });
  fc.net.on('electionBeginCandidacy', () => {
    apply(reduceBeginCandidacy(slice()));
  });
  fc.net.on('electionBeginVote', (m) => {
    apply(reduceBeginVote(slice(), m, env()));
  });
  fc.net.on('electionEndVote', (m) => {
    apply(reduceEndVote(slice(), m, env()));
  });
  fc.net.on('lEventCreateNtf', (m) => {
    apply(reduceEventCreate(slice(), m, env()));
  });
  fc.net.on('lEventInitialize', () => {
    apply(reduceEventInitialize(slice()));
  });
  fc.net.on('lEventTick', (m) => {
    apply(reduceEventTick(slice(), m));
  });
  fc.net.on('lordSkillTick', (m) => {
    apply(reduceSkillTick(slice(), m, env()));
    repaintTaskbar();
  });
  fc.net.on('lordSkillUseNtf', (m, objId) => {
    apply(reduceSkillUse(slice(), m, env(), objId));
    repaintTaskbar();
  });

  // ---- the windows and the NPC menu --------------------------------------------------------------------------------------------

  const actions: LordActions = {
    addDeposit: (total) => {
      senders.addDeposit(total);
    },
    setPledge: (pledge) => {
      senders.setPledge(pledge);
    },
    incVote: (candidate) => {
      senders.incVote(candidate);
    },
    createEvent: (e, i) => {
      senders.createEvent(e, i);
    },
    useSkill: (skill, name) => {
      senders.useSkill(skill, name);
    },
    castSkill: (skill) => {
      executeSkill(skill);
    },
  };
  const ports: LordPorts = {
    view,
    actions,
    nowS: () => ctx.net.clock.serverUnixMsNow(ctx.now()) / 1000,
    panel,
    say,
    sayText: (line) => {
      fc.chat?.putString(line, null, 0xffffffff, SYSTEM_CHAT);
    },
  };
  const installed = new WeakSet<object>();
  const ensureUi = (): void => {
    const ui = fullUi(ctx);
    if (!ui || installed.has(ui)) return;
    installed.add(ui);
    ui.lord.setPorts({ lord: ports });
  };
  ctx.tickSystems.register('lord.ui', ensureUi);
  ctx.machine.subscribe(ensureUi);
  ensureUi();

  const table = menuTable({
    openState: () => {
      fullUi(ctx)?.lord.openState();
    },
    openTender: () => {
      withPanels(() => {
        fullUi(ctx)?.lord.openTender();
      });
    },
    openVote: () => {
      fullUi(ctx)?.lord.openVote();
    },
    openEvent: () => {
      fullUi(ctx)?.lord.openEvent();
    },
    openInfo: () => {
      withPanels(() => {
        fullUi(ctx)?.lord.openInfo();
      });
    },
    openRPInfo: () => {
      withPanels(() => {
        fullUi(ctx)?.lord.openRPInfo();
      });
    },
  });
  for (const [id, fn] of table) {
    menuOf(ctx)?.registerAction(id, () => {
      fn();
    });
  }

  // ---- the lifetime: the character -----------------------------------------------------------------------------------------------

  // the lord state of a character that left the world (logout, kick, back to the character list) goes with it; a world change keeps it, the join batch sends it again
  ctx.machine.subscribe((screen, previous) => {
    if (previous.s !== 'InGame') return;
    if (screen.s === 'InGame' || screen.s === 'Reconnecting') return;
    if (screen.s === 'Loading' || screen.s === 'Entering') return;
    fullUi(ctx)?.lord.closeAll();
    store.setState({ lord: initialLord() });
  });

  // ---- the public face ------------------------------------------------------------------------------------------------------------

  const feature: LordFeature = {
    store,
    senders,
    view,
    slice,
    lord: () => slice().lord,
    isLord: () => slice().lord !== null && slice().lord === selfChar(),
    useSkill: executeSkill,
  };
  features.set(ctx, feature);

  registerLordProbe(ctx, { slice, view });
  return feature;
}
