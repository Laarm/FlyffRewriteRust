// The `minigame` feature (Plan/45 §28.0, 22 §3; WP-CFEAT-45; ref/09b §3.15): the NPC mini-games of the "Misc events" - Kawibawibo (rock-paper-scissors), find word, the dice game (Five System) and the
// puzzle (Reassemble).
//
//   MiniGame (7 subs)     one handler per sub (`reducers.ts`) over the machines of the OPEN windows; a sub for a window that is not open is dropped (except the GETITEM box and the reassemble chat line)
//   windows               the five `APP_MINIGAME_*` windows, the confirm and the two dice boxes: requests over `ui.minigame` (`ui/windows/minigame`); the rules are the machines'
//   menu                  ids 44-47 (`menu.ts`): an open game answers `TID_GAME_MINIGAMEUSING`; 45 opens the inventory first
//   senders               `SendKawibawibo*`, `SendAlphabet*`, `SendReassemble*`, `SendFiveSystem*` (`senders.ts`, ten markers)
//   closing               one transactional row (all reasons) closes the windows; the inventory closing destroys the word and puzzle windows (`WndField.cpp:1048-1056`)
//   seams                 `miniGameOf( ctx )`, the probe member `minigame`
//
// Live in the shipped data only for Kawibawibo (`MaFl_KAWIBAWIBO` in world 207); find word, dice and puzzle have no placed NPC and are proven by the vitest flows and the fixture-driven server tests.
// At logout `WindowManager.closeField` destroys the windows; the dice window's `FiveSystemDestroyWnd` is then refused by the send gate and the server refunds through its logout hook.

import type { ClientContext } from '../../../app/context';
import {
  APP_INVENTORY,
  APP_MESSAGEBOX,
  APP_MINIGAME_DICE,
  APP_MINIGAME_KAWIBAWIBO,
  APP_MINIGAME_PUZZLE,
  APP_MINIGAME_WORD,
} from '../../../gen/defs/resdata';
import { II_SYS_SYS_EVE_KAWIBAWIBO } from '../../../gen/defs/item';
import {
  TID_GAME_DICEGAME_FAIL,
  TID_GAME_DICEGAME_SUCCESS,
  TID_GAME_KAWIBAWIBO_STARTGUID1,
  TID_GAME_MINIGAMEUSING,
} from '../../../gen/defs/text';
import { MessageBoxWnd } from '../../../ui/msgbox/MessageBoxWnd';
import type { PrizeView } from '../../../ui/windows/minigame/ports';
import { fullUi, type FeatureContext } from '../../common/module';
import { itemIconPath } from '../../common/itemIcon';
import { ItemViewCache } from '../../common/itemView';
import { addProbeMember } from '../../common/probe';
import { fmtNumberComma } from '../../common/numbers';
import { formatTid, SYSTEM_CHAT } from '../../common/text';
import { inventoryOf } from '../../inventory';
import { menuOf } from '../../npc/menuModule';
import { eventOf } from '../api';
import { registerEvents } from '../eventsModule';
import { withHelpPanels } from '../helpPanels';
import type { CardFacts } from './cards';
import { DiceMachine, type DiceOut } from './dice';
import { FindWordMachine } from './findWord';
import { KawibawiboMachine, type KawiOut, type KawiWinInfo } from './kawibawibo';
import { menuTable } from './menu';
import { plainItem } from './prizes';
import { PuzzleMachine } from './puzzle';
import {
  MINIGAME_REDUCERS,
  type MiniGameEffect,
  type MiniGameMachines,
  reduceMiniGame,
} from './reducers';
import { miniGameSenders, type MiniGameSenders } from './senders';

/** The public face of the mini-game feature of a context. */
export interface MiniGameFeature {
  readonly senders: MiniGameSenders;
  /** The machines of the open games (tests and the probe read them). */
  machines(): MiniGameMachines;
  /** Opens the Kawibawibo question (menu 44). */
  openKawibawibo(): void;
  openFindWord(): void;
  openDice(): void;
  openPuzzle(): void;
  /** Closes every mini-game window and drops the machines (the transactional row). */
  closeAll(): void;
  /** The last Kawibawibo replies, newest last (the probe). */
  lastKawibawibo(): readonly { result: number; winCount: number }[];
}

const features = new WeakMap<ClientContext, MiniGameFeature>();

/** The mini-game feature of a context; `undefined` before the module registered. */
export function miniGameOf(ctx: ClientContext): MiniGameFeature | undefined {
  return features.get(ctx);
}

/** The oneof cases of `MiniGame.sub` the module handles (`every_minigame_sub_is_handled` pins them against the descriptor). */
export const MINIGAME_CASES = Object.keys(MINIGAME_REDUCERS);

/** `rand() % n`. */
const randomBelow = (n: number): number => Math.floor(Math.random() * n);

/** Registers the mini-games (after the events feature: `minigamesM7.ts` registers both under the one handler module `events.minigames`). */
export function registerMiniGames(fc: FeatureContext): MiniGameFeature {
  const ctx = fc.app;
  // the events feature registers first (its flags and HUD timer); the mini-games need no slice of the store
  if (!eventOf(ctx)) registerEvents(fc);
  const senders = miniGameSenders((msg) => ctx.net.sender.send(msg));
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const inv = () => inventoryOf(ctx);
  const views = new ItemViewCache(() => ctx.data);
  const kawiReplies: { result: number; winCount: number }[] = [];

  let kawi: KawibawiboMachine | null = null;
  let word: FindWordMachine | null = null;
  let puzzle: PuzzleMachine | null = null;
  let dice: DiceMachine | null = null;
  const machines = (): MiniGameMachines => ({ kawi, word, puzzle, dice });

  const say = (tid: number): void => {
    fc.chat?.putString(text(tid), null, fc.texts.textColor(tid), SYSTEM_CHAT);
  };
  const box = (tid: number): void => {
    fullUi(ctx)?.msgbox.open(text(tid));
  };
  const cue = (e: 'tick' | 'win'): void => {
    ctx.audio.cues?.miniGame(e);
  };
  const itemName = (id: number): string => ctx.data?.item(id)?.name ?? '';
  const withPanel = (key: string, fn: (panel: string) => void): void => {
    withHelpPanels(ctx, (panels) => {
      fn(panels[key] ?? '');
    });
  };

  // ---- the facts of the card games --------------------------------------------------------------------------------------------

  const facts: CardFacts = {
    item: (objId) => {
      const it = inv()?.item(objId);
      return it ? { objId: it.objId, itemId: it.itemId, num: it.num, extra: it.extra } : null;
    },
    setExtra: (objId, extra) => {
      inv()?.setExtra(objId, extra);
    },
    icon: (itemId) => itemIconPath(ctx.data?.item(itemId)),
  };

  // ---- Kawibawibo ---------------------------------------------------------------------------------------------------------------

  const prize = (itemId: number, count: number): PrizeView => {
    const prop = ctx.data?.item(itemId);
    return {
      itemId,
      count,
      icon: itemIconPath(prop),
      tooltip: prop ? views.view(plainItem(itemId, 1)) : null,
    };
  };
  const startKawibawibo = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    const out: KawiOut = {
      tick: () => {
        cue('tick');
      },
      win: () => {
        cue('win');
      },
      box,
      hasCoupon: () => (inv()?.countOf(II_SYS_SYS_EVE_KAWIBAWIBO) ?? 0) > 0,
      sendStart: () => {
        senders.kawibawiboStart();
      },
      sendGetItem: () => {
        senders.kawibawiboGetItem();
      },
      openWin: (info: KawiWinInfo) => {
        const m = kawi;
        if (!m) return;
        fullUi(ctx)?.minigame.openKawibawiboWin({
          prize: prize(info.itemId, info.count),
          nextPrize: prize(info.nextItemId, info.nextCount),
          nextDisabled: info.nextDisabled,
          end: () => {
            m.end();
          },
          next: () => {
            m.next();
          },
          closed: () => {
            m.winClosed();
          },
        });
      },
      closeGame: () => {
        fullUi(ctx)?.minigame.kawibawibo()?.destroy();
      },
    };
    // `SAFE_DELETE( m_pWndKawiBawiBoGame ); new CWndKawiBawiBoGame`
    const m = new KawibawiboMachine(out);
    m.onClosed = () => {
      if (kawi === m) kawi = null;
      // the win window is a child of the game window
      fullUi(ctx)?.minigame.kawibawiboWin()?.destroy();
    };
    kawi = m;
    ui.minigame.openKawibawibo(m);
  };
  const openKawibawibo = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    if (ui.minigame.isOpen(APP_MINIGAME_KAWIBAWIBO)) {
      say(TID_GAME_MINIGAMEUSING);
      return;
    }
    if ((inv()?.countOf(II_SYS_SYS_EVE_KAWIBAWIBO) ?? 0) > 0) {
      // one question at a time (`m_pWndKawiBawiBoGameConfirm == NULL`)
      if (!ui.minigame.isBoxOpen('confirm')) {
        ui.minigame.openKawibawiboConfirm({ ok: startKawibawibo });
      }
    } else {
      box(TID_GAME_KAWIBAWIBO_STARTGUID1);
    }
  };

  // ---- find word ----------------------------------------------------------------------------------------------------------------

  const openFindWord = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    if (ui.minigame.isOpen(APP_MINIGAME_WORD)) {
      say(TID_GAME_MINIGAMEUSING);
      return;
    }
    // the cards come from the bag
    ui.wm.openApplet(APP_INVENTORY);
    withPanel('minigamefindworddesc', (panel) => {
      const m = new FindWordMachine(
        facts,
        {
          box,
          sendStart: (q, ids) => {
            senders.alphabetStart(q, ids);
          },
        },
        panel,
      );
      m.onClosed = () => {
        if (word === m) word = null;
      };
      word = m;
      fullUi(ctx)?.minigame.openFindWord({
        opened: () => {
          senders.alphabetOpenWnd();
        },
        view: () => m.view(),
        drop: (slot, objId) => m.drop(slot, objId),
        empty: (slot) => {
          m.empty(slot);
        },
        start: () => {
          m.start();
        },
        closed: () => {
          m.closed();
        },
      });
    });
  };

  // ---- the puzzle -----------------------------------------------------------------------------------------------------------------

  const openPuzzle = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    if (ui.minigame.isOpen(APP_MINIGAME_PUZZLE)) {
      say(TID_GAME_MINIGAMEUSING);
      return;
    }
    withPanel('minigamepuzzledesc', (panel) => {
      const m = new PuzzleMachine(
        facts,
        {
          box,
          sendStart: (ids) => {
            senders.reassembleStart(ids);
          },
        },
        panel,
      );
      m.onClosed = () => {
        if (puzzle === m) puzzle = null;
      };
      puzzle = m;
      fullUi(ctx)?.minigame.openPuzzle({
        opened: () => {
          senders.reassembleOpenWnd();
        },
        view: () => m.view(),
        drop: (objId) => m.drop(objId),
        empty: (slot) => {
          m.empty(slot);
        },
        start: () => {
          m.start();
        },
        closed: () => {
          m.closed();
        },
      });
    });
  };

  // ---- the dice -------------------------------------------------------------------------------------------------------------------

  const openDice = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    if (ui.minigame.isOpen(APP_MINIGAME_DICE)) {
      say(TID_GAME_MINIGAMEUSING);
      return;
    }
    withPanel('minigamedicedesc', (panel) => {
      const ui2 = fullUi(ctx);
      if (!ui2) return;
      const out: DiceOut = {
        tick: () => {
          cue('tick');
        },
        win: () => {
          cue('win');
        },
        random: randomBelow,
        commas: fmtNumberComma,
        box,
        openMessage: (message) => {
          ui2.minigame.openDiceMessage({
            text: message,
            ok: () => {
              dice?.messageOk();
            },
          });
        },
        openTender: (face, preset, min, max) => {
          ui2.minigame.openDiceTender({
            face,
            preset,
            min,
            max,
            bet: (value) => {
              dice?.bet(face, value);
            },
          });
        },
        sendBet: (face, penya) => {
          senders.fiveSystemBet(face, penya);
        },
        sendStart: () => {
          senders.fiveSystemStart();
        },
        sendDestroy: () => {
          senders.fiveSystemDestroyWnd();
        },
        closeBoxes: () => {
          const u = fullUi(ctx);
          u?.minigame.diceMessage()?.destroy();
          u?.minigame.diceTender()?.destroy();
        },
        success: (face, payout) => formatTid(fc.texts, TID_GAME_DICEGAME_SUCCESS, [face, payout]),
        fail: () => text(TID_GAME_DICEGAME_FAIL),
      };
      const m = new DiceMachine(out);
      m.description = panel;
      m.onClosed = () => {
        if (dice === m) dice = null;
      };
      dice = m;
      ui2.minigame.openDice({
        opened: () => {
          senders.fiveSystemOpenWnd();
        },
        view: () => m.view(),
        frame: () => {
          m.frame();
        },
        openStake: (face) => {
          m.openStake(face);
        },
        start: () => {
          m.start();
        },
        closed: () => {
          m.closed();
        },
      });
    });
  };

  // ---- the messages ---------------------------------------------------------------------------------------------------------------

  const run = (effects: readonly MiniGameEffect[]): void => {
    for (const e of effects) {
      switch (e.kind) {
        case 'box':
          fullUi(ctx)?.msgbox.open(formatTid(fc.texts, e.tid, e.args ?? []));
          break;
        case 'chat':
          fc.chat?.putString(e.text, null, fc.texts.textColor(e.colorTid), e.style);
          break;
        case 'close':
          fullUi(ctx)?.minigame[e.window === 'word' ? 'findWord' : 'puzzle']()?.destroy();
          break;
        case 'cue':
          cue(e.cue);
          break;
      }
    }
  };
  fc.net.on('miniGame', (m) => {
    if (m.sub.case === 'kawibawiboResult') {
      kawiReplies.push({ result: m.sub.value.result, winCount: m.sub.value.winCount });
      if (kawiReplies.length > 32) kawiReplies.shift();
    }
    run(reduceMiniGame(machines(), m.sub, { itemName }));
  });

  // ---- the menu, the bag and the lifetime -----------------------------------------------------------------------------------------

  const table = menuTable({
    kawibawibo: openKawibawibo,
    findWord: openFindWord,
    dice: openDice,
    puzzle: openPuzzle,
  });
  for (const [id, fn] of table) {
    menuOf(ctx)?.registerAction(id, () => {
      fn();
    });
  }
  // the double click of a card in the bag (D3) goes to the open window
  inv()?.registerFeed(APP_MINIGAME_WORD, (item) => word?.feed(item.objId) ?? false);
  inv()?.registerFeed(APP_MINIGAME_PUZZLE, (item) => puzzle?.drop(item.objId) ?? false);

  const closeAll = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    // the word window takes the inventory with it (the original's pair, `WndWorld.cpp:8372-8380`)
    const hadWord = ui.minigame.isOpen(APP_MINIGAME_WORD);
    ui.minigame.closeAll();
    if (hadWord) ui.wm.close(APP_INVENTORY);
  };
  fc.transactional.register({
    id: 'minigame',
    reasons: 'all',
    close: () => {
      closeAll();
    },
  });
  // the inventory closing destroys the word and puzzle windows (`WndField.cpp:1048-1056`); the UI may be created after the feature registered (some rigs), so the hook is installed lazily
  const hooked = new WeakSet<object>();
  const ensureUi = (): void => {
    const ui = fullUi(ctx);
    if (!ui || hooked.has(ui)) return;
    hooked.add(ui);
    ui.wm.onEvent((e) => {
      if (e.type !== 'closed' || e.frame.appId !== APP_INVENTORY) return;
      ui.minigame.findWord()?.destroy();
      ui.minigame.puzzle()?.destroy();
    });
  };
  ctx.tickSystems.register('minigame.ui', ensureUi);
  ctx.machine.subscribe(ensureUi);
  ensureUi();

  const feature: MiniGameFeature = {
    senders,
    machines,
    openKawibawibo,
    openFindWord,
    openDice,
    openPuzzle,
    closeAll,
    lastKawibawibo: () => kawiReplies,
  };
  features.set(ctx, feature);
  addProbeMember(ctx, 'minigame', () => ({
    windows: fullUi(ctx)?.minigame.openApps() ?? [],
    // the plain text of the standard message box that is open (`OpenMessageBox`), `null` for none
    box: (() => {
      const w = fullUi(ctx)?.wm.get(APP_MESSAGEBOX, 'main');
      return w instanceof MessageBoxWnd ? w.message : null;
    })(),
    confirm: fullUi(ctx)?.minigame.isBoxOpen('confirm') ?? false,
    tender: fullUi(ctx)?.minigame.isBoxOpen('tender') ?? false,
    message: fullUi(ctx)?.minigame.isBoxOpen('message') ?? false,
    kawibawibo: kawi
      ? {
          status: kawi.status,
          my: kawi.myChoice,
          com: kawi.comChoice,
          winning: kawi.winning,
          maxWin: kawi.maxWin,
          enabled: kawi.enabled,
          checkCoupon: kawi.checkCoupon,
        }
      : null,
    word: word ? { slots: [...word.slots], questionId: word.questionId } : null,
    puzzle: puzzle
      ? { slots: [...puzzle.slots], cards: puzzle.cards ? [...puzzle.cards] : null }
      : null,
    dice: dice
      ? { stakes: [...dice.stakes], status: dice.status, face: dice.face, starts: dice.starts }
      : null,
    replies: [...kawiReplies],
  }));
  return feature;
}
