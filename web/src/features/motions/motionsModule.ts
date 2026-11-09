// The `motions` feature module (Plan/45 §19.4; WP-CFEAT-51): the ports `motions.execute(id)` and `chat.sendEmoticon(index)` of the
// taskbar, the data of the motion window (`ui.char.setPorts({motion})`: the motion and emoticon pages), the pending motion chat
// (1000 ms, replaced by a newer motion) and the registration hook for the rows of later features.

import type { ClientContext } from '../../app/context';
import { entityInfoOf, selfOf } from '../../app/inputHost';
import { EntityKind, type EntityRecord } from '../../game/entities';
import { IK3_WAND } from '../../gen/defs/itemkind';
import type { InputInstance } from '../../input/input';
import type { EmoticonEntry, MotionEntry, MotionView } from '../../ui/windows/char/ports';
import { type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { derivedView, type ViewSource } from '../common/view';
import { sysChat } from '../common/text';
import { chatOf } from '../chat/chatModule';
import { taskbarOf } from '../taskbar';
import { MotionExecutor, type MotionEnv, type MotionHandler, type MotionRow } from './execute';
import { sendMotion } from './senders';
import { PendingChat } from './pendingChat';
import { REQ_USESKILL } from '../../input/worldTick';

/** The atlas of the chat emoticons (`Theme/texChatEmoticon.inc`). */
export const EMOTICON_ATLAS = 'theme/texchatemoticon.inc';
/** A deferred `local_act` is tried for this many 60 Hz frames before it is given up. */
const ACT_RETRY_FRAMES = 30;

/** Where a motion tooltip is shown: the motion window or a taskbar slot. */
export type MotionTooltipVariant = 'window' | 'taskbar';
/** A composer of the tooltip text of one motion. */
export type MotionTooltip = (description: string, variant: MotionTooltipVariant) => string;

/** The public face of the motions feature of a context. */
/** The position a ground item's `SpawnItem` carries (`null` when it has none). */
function spawnPosition(rec: EntityRecord): readonly [number, number, number] | null {
  const spawn = rec.spawn;
  const p = 'obj' in spawn ? spawn.obj?.pos : undefined;
  return p ? [p.x, p.y, p.z] : null;
}

export interface MotionsFeature {
  /** `motions.execute(id)`: a taskbar MOTION shortcut or a click in the motion window. */
  execute(id: number): void;
  /** `chat.sendEmoticon(index)`: `"/" + texts.emotions[index].command` through the chat pipeline. */
  sendEmoticon(index: number): void;
  /** Registers a row of a later feature (ESCAPE: WP-CFEAT-14; CHEER, COUPLE: WP-CFEAT-40). */
  register(id: number, handler: MotionHandler): void;
  /**
   * Registers the live tooltip of a motion (the cheer motion adds the cheer points and the wait for the next one, WP-CFEAT-40): `fn(description, variant)` returns the text; the motion
   * window asks on every hover (`'window'`), the taskbar slot with its own gap (`'taskbar'`).
   */
  registerTooltip(id: number, fn: MotionTooltip): void;
  /** The tooltip text of a motion: its registered composer over the description of the row, else the description; `null` for an id without a row. */
  tooltip(id: number, variant?: MotionTooltipVariant): string | null;
  readonly pending: PendingChat;
  readonly view: ViewSource<MotionView>;
  readonly executor: MotionExecutor;
}

const features = new WeakMap<ClientContext, MotionsFeature>();
export function motionsOf(ctx: ClientContext): MotionsFeature | undefined {
  return features.get(ctx);
}

/** The input core (commands, targeting, attack), `null` on the null input port. */
function coreOf(ctx: ClientContext): NonNullable<Partial<InputInstance>['core']> | null {
  return (ctx.input as Partial<InputInstance>).core ?? null;
}

export const motionsModule: FeatureModule = {
  id: 'motions',
  milestone: 3,
  register(fc) {
    const ctx = fc.app;
    const selfInfo = selfOf(ctx);
    const info = entityInfoOf(ctx);
    const pending = new PendingChat();
    const retries: {
      msg: number;
      p: readonly [number, number, number, number, number];
      then: (r: number) => void;
      left: number;
    }[] = [];

    const rows = (): readonly MotionRow[] => ctx.data?.bundle('tables')?.emotes ?? [];
    const emotions = (): readonly { id: number; text: string }[] => {
      const t = ctx.data?.bundle('texts');
      return t?.emotions ?? [];
    };

    const env: MotionEnv = {
      vendorOpen: () => fullUi(ctx)?.wm.isVendorWindowOpen() ?? false,
      row: (id) => rows().find((r) => r.id === id),
      queueChat: (text) => {
        pending.set(text, ctx.now());
      },
      self: () => {
        const s = selfInfo();
        return s === null
          ? null
          : {
              dead: s.dead,
              state: s.state,
              sitting: s.sitting === true,
              combat: s.combat === true,
            };
      },
      skillPending: () => ((coreOf(ctx)?.reqFlags ?? 0) & REQ_USESKILL) !== 0,
      sendMotion: (msg) => {
        sendMotion(ctx, msg);
      },
      act: (msg, p, then) => {
        const r = ctx.sim.localAct(msg, p);
        if (r === undefined) retries.push({ msg, p, then, left: ACT_RETRY_FRAMES });
        else then(r);
      },
      itemsAround: () => {
        const me = selfInfo();
        if (!me) return [];
        const out: { id: number; distSq: number }[] = [];
        ctx.entities.forEach(EntityKind.Item, (rec) => {
          // a ground item is not in the sim's render view: it lies where its `SpawnItem` put it
          const pos = info(rec.objId)?.pos ?? spawnPosition(rec);
          if (!pos) return;
          const dx = pos[0] - me.pos[0];
          const dy = pos[1] - me.pos[1];
          const dz = pos[2] - me.pos[2];
          out.push({ id: rec.objId, distSq: dx * dx + dy * dy + dz * dz });
        });
        return out;
      },
      useItem: (id) => {
        coreOf(ctx)?.commands.useItem(id);
      },
      clearDest: () => {
        coreOf(ctx)?.commands.clearDest();
      },
      face: (deg) => {
        ctx.sim.cmd.localFace(deg);
      },
      report: () => {
        // legacy-send: SendPlayerBehavior
        (ctx.moveReporter as { armBehavior?: () => void } | null)?.armBehavior?.();
      },
      focusId: () => coreOf(ctx)?.targeting.focus ?? null,
      attackable: (id) => coreOf(ctx)?.attack.attackable(id) ?? false,
      isMover: (id) => info(id)?.kind === 'mover',
      setAutoAttack: (on) => {
        const c = coreOf(ctx);
        if (c) c.targeting.autoAttack = on;
      },
      wandInHand: () => coreOf(ctx)?.deps.hand?.()?.kind3 === IK3_WAND,
      wandAutoattacks: () => ctx.features.features.wand_autoattacks,
      magicAttack: (id, power) => {
        coreOf(ctx)?.commands.magicAttack(id, power);
      },
      say: (tid) => {
        sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid);
      },
    };
    const executor = new MotionExecutor(env);

    const sendEmoticon = (index: number): void => {
      const row = emotions()[index];
      if (!row) return;
      // legacy-send: SendChat
      chatOf(ctx)?.submit(`/${row.text}`, { shortcut: true });
    };

    // ---- the data of the motion window ----------------------------------------------------------------------------------
    const tooltips = new Map<number, MotionTooltip>();
    const tooltipOf = (
      id: number,
      description: string,
      variant: MotionTooltipVariant = 'window',
    ): string => tooltips.get(id)?.(description, variant) ?? description;
    const dataSource = {
      subscribe: (fn: () => void) =>
        ctx.data?.onBundle(() => {
          fn();
        }) ?? (() => undefined),
    };
    const view = derivedView<MotionView>(
      () => {
        const motions: MotionEntry[] = rows().map((r) => ({
          id: r.id,
          icon: `icon/${r.icon}`,
          description: r.desc,
        }));
        const emoticons: EmoticonEntry[] = emotions().map((e) => ({
          atlas: EMOTICON_ATLAS,
          sprite: e.id,
          command: e.text,
        }));
        return { motions, emoticons, tooltip: (m) => tooltipOf(m.id, m.description) };
      },
      [dataSource],
      (a, b) => a.motions.length === b.motions.length && a.emoticons.length === b.emoticons.length,
    );

    const api: MotionsFeature = {
      execute: (id) => {
        executor.execute(id);
      },
      sendEmoticon,
      register: (id, handler) => {
        executor.register(id, handler);
      },
      registerTooltip: (id, fn) => {
        tooltips.set(id, fn);
      },
      tooltip: (id, variant = 'window') => {
        const row = rows().find((r) => r.id === id);
        return row ? tooltipOf(id, row.desc, variant) : null;
      },
      pending,
      view,
      executor,
    };
    features.set(ctx, api);

    // the pending motion chat goes out 1000 ms after the last motion; a deferred `local_act` is retried
    ctx.tickSystems.register('motions', (due) => {
      const text = pending.take(ctx.now());
      if (text !== null) {
        // legacy-send: SendChat
        chatOf(ctx)?.submit(text, { shortcut: true });
      }
      for (let i = retries.length - 1; i >= 0; i -= 1) {
        const r = retries[i];
        if (!r) continue;
        const res = ctx.sim.localAct(r.msg, r.p);
        r.left -= due;
        if (res !== undefined || r.left <= 0) {
          retries.splice(i, 1);
          if (res !== undefined) r.then(res);
        }
      }
    });

    const installed = new WeakSet<object>();
    const ensureUi = (): void => {
      const ui = fullUi(ctx);
      if (!ui || installed.has(ui)) return;
      installed.add(ui);
      ui.char.setPorts({
        motion: {
          view,
          actions: {
            execute: (id) => {
              api.execute(id);
            },
            sendEmoticon,
          },
        },
      });
      taskbarOf(ctx)?.setShortcutPorts({
        motions: {
          execute: (id) => {
            api.execute(id);
          },
        },
        chat: {
          sendMacro: (text) => {
            chatOf(ctx)?.sendMacro(text);
          },
          sendEmoticon,
        },
      });
    };
    ctx.tickSystems.register('motions.ports', ensureUi);
    ctx.machine.subscribe(() => {
      ensureUi();
    });
    ensureUi();

    addProbeMember(ctx, 'motions', () => ({
      last: executor.last,
      pending: pending.text,
      rows: rows().length,
      emoticons: emotions().length,
    }));
  },
};
