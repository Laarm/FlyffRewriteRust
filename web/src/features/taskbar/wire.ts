// The wire form of taskbar shortcuts (Plan/42 §11.6, 11 §7.3, 11a `taskbar.proto`): `Shortcut` <-> the protobuf message, the
// `TaskBar` join message -> the `TaskBarSnapshot` the model loads, and the four slot messages plus `SkillTaskBar`. The `UI`
// layer never sees protobuf types: it calls `TaskbarActions`, which are built here.

import { create } from '@bufbuild/protobuf';
import {
  type Shortcut as WireShortcut,
  ShortcutSchema,
} from '../../gen/proto/flyff/v1/mover_types_pb';
import {
  AddAppletTaskBarSchema,
  AddItemTaskBarSchema,
  RemoveAppletTaskBarSchema,
  RemoveItemTaskBarSchema,
  SkillTaskBarSchema,
  TaskBarQueueSlotSchema,
  type TaskBar,
} from '../../gen/proto/flyff/v1/taskbar_pb';
import { SC, type ScKind, type Shortcut } from '../../ui/dnd/shortcut';
import type { TaskbarActions } from '../../ui/taskbar/ports';
import type { TaskBarSnapshot, WireSlot } from '../../ui/taskbar/model';
import type { ClientContext } from '../../app/context';

/** `MAX_SHORTCUT_STRING - 1`: the longest chat macro in bytes (the server cuts at the same limit). */
export const MAX_SHORTCUT_TEXT = 127;

const KIND_MAX = SC.LORDSKILL;

/** `text` cut to `max` bytes of UTF-8 at a character boundary. */
export function cutUtf8(text: string, max: number): string {
  const enc = new TextEncoder();
  if (enc.encode(text).length <= max) return text;
  let out = '';
  let bytes = 0;
  for (const ch of text) {
    const n = enc.encode(ch).length;
    if (bytes + n > max) break;
    out += ch;
    bytes += n;
  }
  return out;
}

/** A wire shortcut as the UI model; `null` for a kind outside `0..11`. */
export function shortcutFromWire(w: WireShortcut, slot: number): Shortcut | null {
  if (w.shortcutType > KIND_MAX) return null;
  const sc: Shortcut = {
    kind: w.shortcutType as ScKind,
    id: w.id,
    type: w.type,
    // the loader overwrites the stored index with the slot (`m_dwIndex = nIndex`)
    index: slot,
    userId: w.userId,
    data: w.data,
  };
  if (w.shortcutType === SC.CHAT) sc.text = w.text;
  return sc;
}

/** The wire message of a slot shortcut (the text only for CHAT, cut to 127 bytes). */
export function shortcutToWire(sc: Shortcut): WireShortcut {
  return create(ShortcutSchema, {
    shortcutType: sc.kind,
    id: sc.id,
    type: sc.type,
    index: sc.index,
    userId: sc.userId,
    data: sc.data,
    text: sc.kind === SC.CHAT ? cutUtf8(sc.text ?? '', MAX_SHORTCUT_TEXT) : '',
  });
}

/** The `TaskBar` of the join batch as the model's snapshot; entries of an illegal kind are dropped. */
export function snapshotFromWire(m: TaskBar): TaskBarSnapshot {
  const applets: WireSlot[] = [];
  for (const a of m.applets) {
    const sc = a.shortcut ? shortcutFromWire(a.shortcut, a.slot) : null;
    if (sc && sc.kind !== SC.NONE) applets.push({ index: a.slot, shortcut: sc });
  }
  const items: { page: number; slot: WireSlot }[] = [];
  for (const i of m.items) {
    const sc = i.shortcut ? shortcutFromWire(i.shortcut, i.slot) : null;
    if (sc && sc.kind !== SC.NONE)
      items.push({ page: i.bar, slot: { index: i.slot, shortcut: sc } });
  }
  const queue: WireSlot[] = [];
  for (const q of m.queue) {
    const sc = q.shortcut ? shortcutFromWire(q.shortcut, q.slot) : null;
    if (sc && sc.kind !== SC.NONE) {
      // the queue never carries a text (`SCnoStr`)
      const rest = { ...sc };
      delete rest.text;
      queue.push({ index: q.slot, shortcut: rest });
    }
  }
  return { applets, items, queue, actionPoints: m.actionPoint };
}

/**
 * The four slot messages and `SkillTaskBar` over the client's sender. `legacy-send` markers (rule C9) are on the sends. The
 * client applies every change locally at once; there is no echo (§11.6).
 */
export function createWireActions(ctx: ClientContext): TaskbarActions {
  const send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]): void => {
    ctx.net.sender.send(msg);
  };
  return {
    addApplet(slot, sc) {
      // legacy-send: SendAddAppletTaskBar
      send({
        case: 'addAppletTaskBar',
        value: create(AddAppletTaskBarSchema, { index: slot, shortcut: shortcutToWire(sc) }),
      });
    },
    removeApplet(slot) {
      // legacy-send: SendRemoveAppletTaskBar
      send({
        case: 'removeAppletTaskBar',
        value: create(RemoveAppletTaskBarSchema, { index: slot }),
      });
    },
    addItem(page, slot, sc) {
      // legacy-send: SendAddItemTaskBar
      send({
        case: 'addItemTaskBar',
        value: create(AddItemTaskBarSchema, { bar: page, slot, shortcut: shortcutToWire(sc) }),
      });
    },
    removeItem(page, slot) {
      // legacy-send: SendRemoveItemTaskBar
      send({
        case: 'removeItemTaskBar',
        value: create(RemoveItemTaskBarSchema, { bar: page, slot }),
      });
    },
    sendQueue(queue) {
      // legacy-send: SendSkillTaskBar
      send({
        case: 'skillTaskBar',
        value: create(SkillTaskBarSchema, {
          slots: queue.slice(0, 5).map((sc, i) =>
            create(TaskBarQueueSlotSchema, {
              slot: i,
              // an empty slot is sent as kind NONE so that the server clears it ("slots not listed keep their value")
              shortcut: shortcutToWire(
                sc ?? { kind: SC.NONE, id: 0, type: 0, index: i, userId: 0, data: 2 },
              ),
            }),
          ),
        }),
      });
    },
  };
}
