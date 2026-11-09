// The `taskbar` feature module (Plan/42 §11, 45 §19.4; WP-UI-19): the `TaskBar` join message into the client model, the four slot
// messages and `SkillTaskBar` (the senders of 42 §11.6 live here because `ui/` has no network), and the ports of the shortcut
// kinds. What a shortcut points at (an inventory item, a skill, a motion) is another feature's: the feature layer registers
// its sources with `taskbarOf(ctx)` (`setItems`, `setSkills`, `setShortcutPorts`, `setQueue`); a missing source makes its
// kind inert (no icon, no execution) and never empties a slot.

import type { ClientContext } from '../../app/context';
import { APP_INVENTORY } from '../../gen/defs/resdata';
import { TID_GAME_MAX_SHORTCUT_CHAT } from '../../gen/defs/text';
import { SC, type Shortcut } from '../../ui/dnd/shortcut';
import { dispatchShortcut } from '../../ui/taskbar/execute';
import type {
  QueueActions,
  ShortcutPorts,
  SkillDropInfo,
  SlotIcon,
  TaskbarItemPort,
  TaskbarPorts,
  TaskbarQueuePort,
} from '../../ui/taskbar/ports';
import { parseMarkup } from '../../ui/text/markup';
import type { ItemView } from '../../ui/tooltip/types';
import { type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { sysChat } from '../common/text';
import { chatOf } from '../chat/chatModule';
import { motionsOf } from '../motions/motionsModule';
import { createWireActions, snapshotFromWire } from './wire';
import type { TaskBar } from '../../gen/proto/flyff/v1/taskbar_pb';

/** The inventory as the taskbar sees it: the feature that owns the inventory registers it. */
export interface TaskbarItemSource extends TaskbarItemPort {
  /** `item/...` icon of an inventory item. */
  icon(objId: number): string | null;
  /** The number drawn on a stackable item (the total of that item in the inventory), `null` = none. */
  count(objId: number): number | null;
  /** The cooldown group of the item (absolute `host.now()` ms). */
  cooldown?(objId: number): { startMs: number; endMs: number } | null;
  /** `dwParts` of the item's prop (`-1` / `null` = not equipment). */
  parts?(objId: number): number | null;
  /** The tooltip view of the item (`null` = unknown). */
  view?(objId: number): ItemView | null;
}

/** The skills as the taskbar sees them. */
export interface TaskbarSkillSource {
  info(type: number, id: number): SkillDropInfo | null;
  /** `icon/...` (party skills) or `item/...` (normal skills, KEEP-QUIRK) icon path. */
  icon(type: number, id: number): string | null;
  /** The skill id and level for the tooltip. */
  tooltip?(type: number, id: number): { skillId: number; level: number } | null;
  /** The cool-down of the skill (`RenderCollTime`: absolute `host.now()` ms), `null` when it is ready (party skills have none). */
  cooldown?(type: number, id: number): { startMs: number; endMs: number } | null;
}

/**
 * What the action slot adds to ANY skill source (the skill tree registers the basic one, `setSkills`): the cool-down of a skill
 * (`RenderCollTime`) and the drop facts only the skill runtime knows (the cool-down in ms and the name for the queue refusal line).
 * `setSkillExtras` is independent of `setSkills`: whichever is installed last, the other keeps working.
 */
export interface SkillExtras {
  cooldown?(type: number, id: number): { startMs: number; endMs: number } | null;
  dropInfo?(type: number, id: number): Partial<SkillDropInfo> | null;
}

/** The late-bound hooks of the action-queue row (45 WP-CFEAT-32): the lines, the action-point mode and the tooltip. */
export interface QueueHooks {
  messageFmt?: (tid: number, ...args: string[]) => void;
  actionPointsFree?: () => boolean;
  animating?: () => boolean;
  queueTooltip?: (ap: number, slot: number, cost: number) => string | null;
}

/** The public face of the taskbar feature of a context. */
export interface TaskbarFeature {
  /** The ports 42 uses (installed into `ui.taskbar`). */
  readonly ports: TaskbarPorts;
  setItems(source: TaskbarItemSource | null): void;
  setSkills(source: TaskbarSkillSource | null): void;
  setQueue(port: TaskbarQueuePort | null): void;
  /** The additions of the action slot to the skill source (see {@link SkillExtras}). */
  setSkillExtras(extras: SkillExtras | null): void;
  /** The rules of the action-queue row (`CheckAddSkill`, the `SkillTaskBar` send): 42's `ActionQueue` runs on them. */
  setQueueActions(actions: QueueActions | null): void;
  /** The hooks of the queue row (see {@link QueueHooks}); each call merges into the registered ones. */
  setQueueHooks(hooks: QueueHooks): void;
  /** The ports of the shortcut kinds (§11.5); each call merges into the registered ones. */
  setShortcutPorts(ports: Partial<ShortcutPorts>): void;
  /** `trade.request` for an `APP_TRADE` applet slot. */
  setTradeRequest(fn: (() => void) | null): void;
}

const features = new WeakMap<ClientContext, TaskbarFeature>();

/** The taskbar feature of a context; `undefined` before the module registered. */
export function taskbarOf(ctx: ClientContext): TaskbarFeature | undefined {
  return features.get(ctx);
}

/** `Icon/<name>` of a motion shortcut (`propMotion.txt` `szIconName`, `tables.emotes`). */
export function motionIcon(ctx: ClientContext, id: number): string | null {
  const tables = ctx.bundles.get('tables') as
    { emotes?: { id: number; icon: string }[] } | undefined;
  const row = tables?.emotes?.find((e) => e.id === id);
  return row ? `icon/${row.icon}` : null;
}

export const taskbarModule: FeatureModule = {
  id: 'taskbar',
  milestone: 3,
  register(fc) {
    const ctx = fc.app;
    let items: TaskbarItemSource | null = null;
    let skills: TaskbarSkillSource | null = null;
    let queue: TaskbarQueuePort | null = null;
    let skillExtras: SkillExtras | null = null;
    let queueActions: QueueActions | null = null;
    let queueHooks: QueueHooks = {};
    let tradeRequest: (() => void) | null = null;
    let shortcutPorts: ShortcutPorts = {};
    let pending: TaskBar | null = null;
    const installed = new WeakSet<object>();

    // the chat shortcuts are executable from the chat feature (WP-CFEAT-06): macros and `/emoticon` commands
    const chatPorts = (): NonNullable<ShortcutPorts['chat']> => ({
      sendMacro: (text) => {
        chatOf(ctx)?.sendMacro(text);
      },
      sendEmoticon: (index) => {
        const texts = ctx.bundles.get('texts') as
          { emotions?: { id: number; text: string }[] } | undefined;
        const row = texts?.emotions?.find((e) => e.id === index);
        if (row) chatOf(ctx)?.submit(`/${row.text}`, { shortcut: true });
      },
    });

    const iconOf = (sc: Shortcut): string | null => {
      if (sc.kind === SC.ITEM) return items?.icon(sc.id) ?? null;
      if (sc.kind === SC.SKILL) return skills?.icon(sc.type, sc.id) ?? null;
      if (sc.kind === SC.MOTION) return motionIcon(ctx, sc.id);
      if (sc.kind === SC.CHAT) return 'icon/icon_macrochat.dds';
      // ---- lord (ui-lord, M7)
      if (sc.kind === SC.LORDSKILL) return shortcutPorts.lord?.icon?.(sc.id) ?? null;
      return null;
    };

    const ports: TaskbarPorts = {
      execute: (sc) => {
        dispatchShortcut(sc, { chat: chatPorts(), ...shortcutPorts });
      },
      slotIcon: (sc): SlotIcon | null => {
        const path = iconOf(sc);
        if (path === null) return null;
        if (sc.kind === SC.ITEM && items) {
          const count = items.count(sc.id);
          const cd = items.cooldown?.(sc.id) ?? null;
          return {
            path,
            ...(count !== null && count > 1 ? { count } : {}),
            ...(cd ? { cooldown: cd } : {}),
          };
        }
        if (sc.kind === SC.SKILL) {
          const cd = (skillExtras?.cooldown ?? skills?.cooldown)?.(sc.type, sc.id);
          if (cd) return { path, cooldown: cd };
        }
        // ---- lord (ui-lord, M7): the radar of a lord skill that is cooling down
        if (sc.kind === SC.LORDSKILL) {
          const cd = shortcutPorts.lord?.cooldown?.(sc.id) ?? null;
          if (cd) return { path, cooldown: cd };
        }
        return { path };
      },
      slotTooltip: (sc) => {
        if (sc.kind === SC.CHAT) return sc.text ?? null;
        // a motion shows its description; the cheer motion adds the points and the wait for the next one (WP-CFEAT-40)
        if (sc.kind === SC.MOTION) return motionsOf(ctx)?.tooltip(sc.id, 'taskbar') ?? null;
        // ---- lord (ui-lord, M7): the text with its markup (the rich path below parses it)
        if (sc.kind === SC.LORDSKILL) return shortcutPorts.lord?.tooltip?.(sc.id) ?? null;
        return null;
      },
      slotTooltipRich: (sc, rect) => {
        const ui = fullUi(ctx);
        const owner = ui?.taskbar.window;
        if (!ui || !owner) return false;
        if (sc.kind === SC.ITEM) {
          const view = items?.view?.(sc.id);
          if (!view) return false;
          ui.tooltip.putItem(owner, view, rect, APP_INVENTORY);
          return true;
        }
        if (sc.kind === SC.SKILL) {
          const t = skills?.tooltip?.(sc.type, sc.id);
          if (!t) return false;
          if (sc.type === 2) ui.tooltip.putTroupe(owner, sc.id, rect);
          else ui.tooltip.putSkill(owner, t.skillId, t.level, rect, true);
          return true;
        }
        // ---- lord (ui-lord, M7): `g_toolTip.PutToolTip( 10000, CEditString.SetParsingString( ... ) )`
        if (sc.kind === SC.LORDSKILL) {
          const text = shortcutPorts.lord?.tooltip?.(sc.id);
          if (!text) return false;
          ui.tooltip.put(owner, parseMarkup(text), rect, 0);
          return true;
        }
        return false;
      },
      actions: createWireActions(ctx),
      newFontColor: () => ctx.features.features.new_font_color,
      slotIsEquip: (sc) => {
        if (sc.kind !== SC.ITEM) return false;
        const parts = items?.parts?.(sc.id);
        return parts !== null && parts !== undefined && parts !== -1 && parts !== 0xffffffff;
      },
      skills: {
        info: (t, id) => {
          const base = skills?.info(t, id) ?? null;
          const more = skillExtras?.dropInfo?.(t, id);
          return base && more ? { ...base, ...more } : base;
        },
      },
      get items() {
        return items ?? undefined;
      },
      get queue() {
        return queue ?? undefined;
      },
      get queueActions() {
        return queueActions ?? undefined;
      },
      messageFmt: (tid, ...args) => {
        queueHooks.messageFmt?.(tid, ...args);
      },
      actionPointsFree: () => queueHooks.actionPointsFree?.() ?? false,
      animating: () => queueHooks.animating?.() ?? false,
      queueTooltip: (ap, slot, cost) => queueHooks.queueTooltip?.(ap, slot, cost) ?? null,
      message: (tid) => {
        sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid);
      },
      tradeRequest: () => {
        tradeRequest?.();
      },
      iconOf,
    };

    const api: TaskbarFeature = {
      ports,
      setItems(s) {
        items = s;
      },
      setSkills(s) {
        skills = s;
      },
      setQueue(q) {
        queue = q;
      },
      setSkillExtras(e) {
        skillExtras = e;
      },
      setQueueActions(a) {
        queueActions = a;
      },
      setQueueHooks(h) {
        queueHooks = { ...queueHooks, ...h };
      },
      setShortcutPorts(p) {
        shortcutPorts = { ...shortcutPorts, ...p };
      },
      setTradeRequest(fn) {
        tradeRequest = fn;
      },
    };
    features.set(ctx, api);

    /** Installs the ports into the UI once it exists (it is built at boot step B5, possibly after this module registered). */
    const ensureUi = (): void => {
      const ui = fullUi(ctx);
      if (!ui || installed.has(ui)) return;
      installed.add(ui);
      ui.taskbar.setPorts(ports);
      if (pending) {
        apply(pending);
        pending = null;
      }
    };
    const apply = (m: TaskBar): void => {
      const ui = fullUi(ctx);
      if (!ui) {
        pending = m;
        return;
      }
      const snap = snapshotFromWire(m);
      ui.taskbar.model.replaceAll(snap, (type, id) => {
        const info = skills?.info(type, id);
        return info !== null && info !== undefined && (info.combo === 1 || info.combo === 4);
      });
      // ITEM entries of stackable items record the item id (`m_dwItemId`); the model only knows ids
      if (items) {
        const m2 = ui.taskbar.model;
        const st = m2.get();
        st.applets.forEach((sc, i) => {
          if (sc?.kind === SC.ITEM) {
            const itemId = items?.stackItemId(sc.id);
            if (itemId !== null && itemId !== undefined) m2.setApplet(i, { ...sc, itemId });
          }
        });
        st.items.forEach((row, page) => {
          row.forEach((sc, slot) => {
            if (sc?.kind === SC.ITEM) {
              const itemId = items?.stackItemId(sc.id);
              if (itemId !== null && itemId !== undefined)
                m2.setItem(page, slot, { ...sc, itemId });
            }
          });
        });
      }
    };

    ctx.tickSystems.register('taskbar.ports', ensureUi);
    ctx.machine.subscribe(() => {
      ensureUi();
    });
    fc.net.on('taskBar', (m) => {
      ensureUi();
      apply(m);
    });

    // the e2e reads the model instead of pixels (`__flyffDebug.features.taskbar()`)
    addProbeMember(ctx, 'taskbar', () => {
      const ui = fullUi(ctx);
      const s = ui?.taskbar.model.get();
      if (!s) return null;
      const flat = (sc: Shortcut | null) =>
        sc === null
          ? null
          : { kind: sc.kind, id: sc.id, type: sc.type, index: sc.index, text: sc.text };
      return {
        applets: s.applets.map(flat),
        items: s.items.map((p) => p.map(flat)),
        queue: s.queue.map(flat),
        actionPoints: s.actionPoints,
        page: s.page,
      };
    });
  },
};

export { TID_GAME_MAX_SHORTCUT_CHAT };
