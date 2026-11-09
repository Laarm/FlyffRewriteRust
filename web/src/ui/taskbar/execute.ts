// `ObjectExecutor(SHORTCUT)` for the kinds that are not applets (Plan/42 §11.5, WndManager.cpp:2617-3238): the table that
// dispatches a taskbar shortcut to the feature port of its kind (45 §19.4). APPLET shortcuts go through
// `wm.objectExecutorApplet` (the taskbar window does that itself); ETC, SCRIPT and NONE do nothing.

import { SC, type Shortcut } from '../dnd/shortcut';
import type { ShortcutPorts } from './ports';

/** What `executeShortcut` did (the tests read it; `null` = no port for that kind or an unexecutable kind). */
export type Executed =
  'item' | 'skill' | 'motion' | 'chat' | 'emoticon' | 'lord' | 'object' | 'actionSlot' | null;

/**
 * Dispatches `sc` to its port:
 *
 * | kind | call |
 * |---|---|
 * | ITEM | `items.useFromShortcut(id)` |
 * | SKILL | `skills.useShortcut(type, id)` |
 * | MOTION | `motions.execute(id)` |
 * | CHAT | `chat.sendMacro(text)` |
 * | EMOTICON | `chat.sendEmoticon(id)` |
 * | LORDSKILL | `lord.useSkill(id)` |
 * | OBJECT | `messenger.openMessage(id)` |
 * | SKILLFUN | `actionSlot.use()` |
 */
export function dispatchShortcut(sc: Shortcut, ports: ShortcutPorts): Executed {
  switch (sc.kind) {
    case SC.ITEM:
      if (!ports.items) return null;
      ports.items.useFromShortcut(sc.id);
      return 'item';
    case SC.SKILL:
      if (!ports.skills) return null;
      ports.skills.useShortcut(sc.type, sc.id);
      return 'skill';
    case SC.MOTION:
      if (!ports.motions) return null;
      ports.motions.execute(sc.id);
      return 'motion';
    case SC.CHAT:
      if (!ports.chat) return null;
      ports.chat.sendMacro(sc.text ?? '');
      return 'chat';
    case SC.EMOTICON:
      if (!ports.chat) return null;
      ports.chat.sendEmoticon(sc.id);
      return 'emoticon';
    case SC.LORDSKILL:
      if (!ports.lord) return null;
      ports.lord.useSkill(sc.id);
      return 'lord';
    case SC.OBJECT:
      if (!ports.messenger) return null;
      ports.messenger.openMessage(sc.id);
      return 'object';
    case SC.SKILLFUN:
      if (!ports.actionSlot) return null;
      ports.actionSlot.use();
      return 'actionSlot';
    case SC.NONE:
    case SC.ETC:
    case SC.APPLET:
    case SC.SCRIPT:
      return null;
  }
}
