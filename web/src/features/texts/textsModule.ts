// The `texts` feature module (Plan/45 §7.11, §7.12, §8.3; WP-CFEAT-07): `DefinedText`, `Text`, `SystemNotice`, `Caption`,
// `WorldMsg`, `BlockNtf`, `EventMessage`, the join greeting lines and the browser-side commands. Lines go to 42's chat router,
// captions to the HUD (`features/hud`), the upper message box to 42's message-box service.

import type { ClientContext } from '../../app/context';
import { STYLE_CLIENT_LINE } from '../chat/display';
import { type FeatureModule, fullUi } from '../common/module';
import { hudOf } from '../hud';
import { messengerOf } from '../messenger/api';
import { questOf } from '../quest/api';
import { registerClientCommands } from './clientCommands';
import {
  blockActions,
  captionActions,
  definedTextActions,
  eventMessageActions,
  joinGreetingActions,
  systemNoticeActions,
  textActions,
  worldMsgActions,
  type TextAction,
} from './format';

export const textsModule: FeatureModule = {
  id: 'notice',
  milestone: 2,
  register(fc) {
    const ctx: ClientContext = fc.app;

    const run = (actions: readonly TextAction[]): void => {
      const ui = fullUi(ctx);
      for (const a of actions) {
        if (a.type === 'put') {
          ui?.chat.putString(a.text, a.objId, a.color, a.style, a.speaker);
        } else if (a.type === 'caption') {
          hudOf(ctx).caption(a.text, a.small, ctx.now());
          if (a.log) ui?.chat.logWindow()?.put(a.text, 0xffffffff);
        } else if (a.type === 'box') {
          ui?.msgbox.openUpper(a.text);
        }
      }
    };

    fc.net.on('definedText', (m) => {
      run(definedTextActions(fc.texts, m.tid, m.args, m.display));
    });
    fc.net.on('text', (m) => {
      run(textActions(m.kind, m.text));
    });
    fc.net.on('systemNotice', (m) => {
      run(systemNoticeActions(fc.texts, m.text));
    });
    fc.net.on('caption', (m) => {
      run(captionActions(m.small, m.worldId, ctx.stores.worldStore.getState().worldId, m.text));
    });
    fc.net.on('worldMsg', (m) => {
      run(worldMsgActions(m.text));
    });
    fc.net.on('blockNtf', (m) => {
      const r = blockActions(fc.texts, m.gu, m.name);
      run(r.actions);
      // the IM window of the refusing player closes (`pWndMessage->Destroy()`)
      if (r.closeIm) messengerOf(ctx)?.closeMessage(m.name);
    });
    fc.net.on('eventMessage', (m, objId) => {
      const self = ctx.stores.selfStore.getState();
      run(eventMessageActions(fc.texts, m.flag, m.text, objId, self.objId === objId));
    });
    fc.net.on('addGameJoin', (m) => {
      const self = ctx.stores.selfStore.getState();
      run(
        joinGreetingActions(
          fc.texts,
          self.level,
          m.savedPlayTimeS,
          m.welcomeMessages,
          questOf(ctx)?.joinTitles() ?? [],
        ),
      );
    });

    registerClientCommands(fc);
  },
};

export { STYLE_CLIENT_LINE };
