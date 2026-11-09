// The `chat` feature module (Plan/45 §8; WP-CFEAT-06): the display routing of the chat messages, the input pipeline behind
// the chat window's `submit`, the ports of 42's chat shells (`ui.chat.setPorts`) and the overlay sink of 41 (balloons,
// emoticons) and of the centre messages. 42's `ChatRouter` owns the targets and the ring; this module decides text, colour,
// style and speaker of every line (display.ts) and what a typed line does (input.ts).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { AUTH_GAMEMASTER } from '../../app/hosts';
import { visibleCommands } from '../../gen/gm';
import { TID_GAME_PARTYNOTCHAT, TID_SYS_HELP } from '../../gen/defs/text';
import { ChatReqSchema } from '../../gen/proto/flyff/v1/chat_pb';
import type { EmotionRow } from './emotes';
import type { ChatPorts, OverlaySink } from '../../ui/chat/ports';
import { CHATSTY } from '../../ui/chat/styles';
import { hudOf } from '../hud';
import { type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { sysChat } from '../common/text';
import { EntityKind, type EntityRecord } from '../../game/entities';
import { localCommandsOf } from './commands';
import {
  type ChatAction,
  COLOR_TEXT_GENERAL,
  type DisplayEnv,
  displayChatNtf,
  displayGuildChat,
  displayPartyChat,
  displayReturnSay,
  displayShout,
  displayWhisper,
  type SpeakerView,
} from './display';
import { helpList, helpOne } from './helpCommand';
import { type LineRoute, routeLine } from './input';
import { type ChatStore, defineChatStore } from './store';
import { createWordFilter, type FilterPair, type WordFilter } from './wordFilter';
import { defineOptionsStore } from '../options';
import { WI_WORLD_QUIZ } from '../../gen/defs/world';

/** The balloon / emoticon requests of 41's overlay (`RenderFx`), as far as the chat needs them. */
export interface BalloonFx {
  balloon(id: number, b: { text: string; kind: 0 | 1 | 2 | 3 } | null): void;
  emoticon(id: number, set: 'chat' | 'dialog', index: number): void;
}

/** The public face of the chat feature of a context (`chatOf(ctx)`). */
export interface ChatApi {
  /** The input pipeline: what the chat window's Enter calls. Returns what became of the line. */
  submit(line: string, opts?: { shortcut?: boolean }): LineRoute['kind'];
  /** `chat.sendMacro`: a taskbar chat shortcut (`ChatReq.shortcut = true`). */
  sendMacro(text: string): LineRoute['kind'];
  readonly store: ChatStore;
}

const apis = new WeakMap<ClientContext, ChatApi>();

/** The chat feature of a context; `undefined` before the module registered. */
export function chatOf(ctx: ClientContext): ChatApi | undefined {
  return apis.get(ctx);
}

/** `ctx.render.fx` when the renderer is up (the port may not expose it yet; then balloons are dropped, they are cosmetic). */
function fxOf(ctx: ClientContext): BalloonFx | null {
  const fx = (ctx.render as { fx?: BalloonFx } | null)?.fx;
  return fx ?? null;
}

function speakerOf(rec: EntityRecord | undefined): SpeakerView | undefined {
  if (!rec || rec.removed) return undefined;
  const v = rec.visual;
  return {
    objId: rec.objId,
    isPlayer: rec.kind === EntityKind.Player,
    isNpc: rec.kind === EntityKind.Npc,
    name: v?.name ?? '',
    authorization: v?.authorization ?? 0,
    mode: v?.mode ?? 0,
    vendorOpen: v?.vendorTitle != null,
  };
}

export const chatModule: FeatureModule = {
  id: 'chat',
  milestone: 2,
  register(fc) {
    const ctx = fc.app;
    const store = defineChatStore(ctx.stores.registry);
    const options = defineOptionsStore(ctx.stores.registry);
    let filter: WordFilter | null | undefined;

    const emotions = (): readonly EmotionRow[] => {
      const t = ctx.bundles.get('texts') as { emotions?: EmotionRow[] } | undefined;
      return t?.emotions ?? [];
    };

    /** The sender-side filter (built once from `names.chat_filter` when the data says the client must filter). */
    const clientFilter = (): WordFilter | null => {
      if (ctx.features.fidelity.chat_filter_server_side) return null;
      if (filter === undefined) {
        const names = ctx.bundles.get('names') as { chat_filter?: FilterPair[] } | undefined;
        filter = names?.chat_filter ? createWordFilter(names.chat_filter) : null;
      }
      return filter;
    };

    const env = (): DisplayEnv => {
      const g = options.getState().game;
      return {
        texts: fc.texts,
        emotions: emotions(),
        selfName: ctx.stores.selfStore.getState().name,
        worldId: ctx.stores.worldStore.getState().worldId,
        speaker: (id) => speakerOf(ctx.entities.get(id)),
        ignoreNpcDialogue: g.ignoreNpcDialogue,
      };
    };

    // ---- the ports of 42 and the sink of 41 -------------------------------------------------------------------

    const installed = new WeakSet<object>();
    const sink: OverlaySink = {
      clientMsg: (text, color) => {
        hudOf(ctx).clientMsg(text, color, ctx.now());
      },
      balloon: (objId, text, _color, kind) => {
        fxOf(ctx)?.balloon(objId, { text, kind: kind ?? 0 });
      },
    };

    const apply = (actions: readonly ChatAction[]): void => {
      for (const a of actions) {
        switch (a.type) {
          case 'put':
            fullUi(ctx)?.chat.putString(a.text, a.objId, a.color, a.style, a.speaker);
            break;
          case 'balloon':
            fxOf(ctx)?.balloon(a.objId, { text: a.text, kind: a.kind });
            break;
          case 'emoticon':
            fxOf(ctx)?.emoticon(a.objId, a.set, a.index);
            break;
          case 'lastWhisperer':
            store.setState({ lastWhisperer: a.name });
            break;
        }
      }
    };

    const submit: ChatApi['submit'] = (line, opts) => {
      const authority = ctx.stores.sessionStore.getState().authority;
      const lastWhisperer = store.getState().lastWhisperer;
      const route = routeLine(line, {
        maxChars: ctx.features.gameplay.chat_max_chars,
        lastWhisperer: () => lastWhisperer,
        partyMembers: () => partyMembers(ctx),
        authority: () => authority,
        hasLocal: (row) => localCommandsOf(ctx).get(row) !== undefined,
        hasLocalNamed: (name) => localCommandsOf(ctx).getNamed(name) !== undefined,
        clientFilter,
        partyNotChatTid: TID_GAME_PARTYNOTCHAT,
      });
      switch (route.kind) {
        case 'local': {
          const local = localCommandsOf(ctx);
          (local.getNamed(route.command.name) ?? local.get(route.command.row))?.({
            args: route.args,
            line: route.line,
          });
          break;
        }
        case 'refuse':
          sysChat(fc.texts, fullUi(ctx)?.chat ?? null, route.tid);
          break;
        case 'send':
          // legacy-send: SendChat
          fc.net.send({
            case: 'chatReq',
            value: create(ChatReqSchema, { text: route.text, shortcut: opts?.shortcut === true }),
          });
          break;
        case 'empty':
          break;
      }
      return route.kind;
    };
    // `/help` is answered here (BFIX1 #006): the list in a scrollable window, `/help <command>` in two chat lines. Nothing is sent.
    /** A green line in the chat box (`HELP` style: `HELP_CHAT` is in the default etc filter, so it is a chat line, not a centre notice). */
    const helpLine = (text: string): void => {
      fullUi(ctx)?.chat.putString(text, null, COLOR_TEXT_GENERAL, CHATSTY.HELP);
    };
    localCommandsOf(ctx).setNamed('help', ({ args }) => {
      const authority = ctx.stores.sessionStore.getState().authority;
      const arg = args[0];
      if (arg !== undefined) {
        for (const l of helpOne(arg, authority)) helpLine(l);
        return;
      }
      const { header, lines } = helpList(authority);
      if (fullUi(ctx)?.chat.openCommandHelp({ title: 'Commands', header, lines }) != null) return;
      // no window (no UI, or it could not be created): the whole list into the chat box
      helpLine(header);
      for (const l of lines) helpLine(l);
    });
    const api: ChatApi = {
      submit,
      sendMacro: (text) => submit(text, { shortcut: true }),
      store,
    };
    apis.set(ctx, api);

    /** Installs the ports once the UI exists (it is built at boot step B5, possibly after this module registered). */
    const ensureUi = (): void => {
      const ui = fullUi(ctx);
      if (!ui || installed.has(ui)) return;
      installed.add(ui);
      const ports: ChatPorts = {
        view: {
          lastWhisperer: () => store.getState().lastWhisperer,
          inParty: () => ctx.stores.selfStore.getState().partyId !== 0,
          inGuild: () => ctx.stores.selfStore.getState().guildId !== 0,
          isGm: () => ctx.stores.sessionStore.getState().authority >= AUTH_GAMEMASTER,
          selfName: () => ctx.stores.selfStore.getState().name,
          helpPrefix: () => fc.texts.text(TID_SYS_HELP),
        },
        actions: {
          submit: (line) => {
            submit(line);
          },
        },
        commands: {
          names: () =>
            visibleCommands(ctx.stores.sessionStore.getState().authority).map((c) => c.name),
        },
        filter: {
          setFilters: (filters, etc) => {
            options.setState((o) => ({
              ...o,
              ui: { ...o.ui, chatFilters: [...filters], chatFilterEtc: etc },
            }));
          },
        },
      };
      ui.chat.setPorts(ports);
      ui.chat.setOverlays(sink);
    };
    ctx.tickSystems.register('chat.ports', ensureUi);
    ctx.machine.subscribe(() => {
      ensureUi();
    });

    // ---- the handlers -----------------------------------------------------------------------------------------

    const on: typeof fc.net.on = (c, h) => {
      fc.net.on(c, h);
    };
    on('chatNtf', (m, objId) => {
      ensureUi();
      apply(displayChatNtf(env(), objId, m.text));
    });
    on('shout', (m) => {
      ensureUi();
      apply(displayShout(env(), m.objId, m.name, m.text, m.color));
    });
    on('whisper', (m) => {
      ensureUi();
      apply(displayWhisper(env(), m));
    });
    on('say', (m) => {
      // the instant messages (`Say`) are the messenger's (`features/messenger`); what stays here is the answer of a partner who is not online
      ensureUi();
      if (m.search !== 0) {
        apply(displayWhisper(env(), { from: m.from, to: m.to, text: m.text, search: m.search }));
      }
    });
    on('returnSay', (m) => {
      ensureUi();
      apply(displayReturnSay(env(), ctx.stores.selfStore.getState().name, m.flag, m.text));
    });
    on('partyChat', (m) => {
      ensureUi();
      apply(displayPartyChat(env(), m.objId, m.name, m.text));
    });
    on('guildChat', (m) => {
      ensureUi();
      apply(displayGuildChat(env(), m.objId, m.name, m.text));
    });

    // e2e reads (`__flyffDebug.features`): the ring, the HUD messages and the chat state
    addProbeMember(ctx, 'chatLines', (n?: number) =>
      (fullUi(ctx)?.chat.router.log.all() ?? [])
        .slice(-(n ?? 20))
        .map((l) => ({ text: l.text, color: l.color, style: l.style })),
    );
    addProbeMember(ctx, 'hudMessages', () => ({
      centre: hudOf(ctx).centre.lines.map((l) => l.text),
      captions: hudOf(ctx).captions.list.map((c) => c.text),
    }));
    addProbeMember(ctx, 'commandHelp', () => fullUi(ctx)?.chat.commandHelp() ?? null);
    addProbeMember(ctx, 'chat', () => ({
      lastWhisperer: store.getState().lastWhisperer,
      quizWorld: ctx.stores.worldStore.getState().worldId === WI_WORLD_QUIZ,
    }));
  },
};

/** The number of members of the own party: the party feature's store when it exists, else `2` for "in a party" (the server decides). */
function partyMembers(ctx: ClientContext): number {
  const party = ctx.stores.registry.get('party')?.getState() as
    { members?: readonly unknown[] } | undefined;
  if (party?.members) return party.members.length;
  return ctx.stores.selfStore.getState().partyId !== 0 ? 2 : 0;
}
