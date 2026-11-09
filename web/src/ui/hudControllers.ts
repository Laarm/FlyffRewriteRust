// The HUD shells of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`; WP-UI-14 taskbar, WP-UI-16 chat windows, WP-UI-17
// countdown): `APP_*` name -> loader of the shell class. The classes are part of the UI chunk anyway (`createUi` builds the
// taskbar and the chat service), so the loaders resolve at once instead of splitting a chunk that cannot be split.
import type { WndCtor } from './catalog';
import { ChatFilterWnd } from './chat/ChatFilterWnd';
import { ChatLogWnd } from './chat/ChatLogWnd';
import { ChatWnd } from './chat/ChatWnd';
import { CommandWnd } from './chat/CommandWnd';
import { LogoutCountdownWnd } from './session/LogoutCountdownWnd';
import { TaskbarWnd } from './taskbar/TaskbarWnd';

const of = (ctor: WndCtor) => (): Promise<WndCtor> => Promise.resolve(ctor);

export const HUD_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_COMMUNICATION_CHAT: of(ChatWnd),
  APP_CHAT_LOG: of(ChatLogWnd),
  APP_CHAT_FILTER: of(ChatFilterWnd),
  APP_COMMAND: of(CommandWnd),
  APP_MESSAGE_TEXT: of(LogoutCountdownWnd),
  // all eight layouts are the one `TaskbarWnd` class (the width picks the layout)
  APP_TASKBAR800: of(TaskbarWnd),
  APP_TASKBAR1024: of(TaskbarWnd),
  APP_TASKBAR1280: of(TaskbarWnd),
  APP_TASKBAR1360W: of(TaskbarWnd),
  APP_TASKBAR1400: of(TaskbarWnd),
  APP_TASKBAR1440W: of(TaskbarWnd),
  APP_TASKBAR1600: of(TaskbarWnd),
  APP_TASKBAR1680W: of(TaskbarWnd),
};
