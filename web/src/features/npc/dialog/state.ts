// The dialog store and its reducers (Plan/45 §11.3, §2.3 `dialogStore`; ref/16 §5.7-5.9; WP-CFEAT-18). The state mirrors the
// members of `CWndDialog` that decide what the window shows (`m_aKeyButton`, `m_aWordButton`, `m_strArray`, `m_nCurArray`,
// `m_string`, the two quest list boxes); every `DialogOps` operation and every click is a pure function of the state.
// `session_seq` rule: a message of another NPC or with a sequence number not above the last applied one is dropped.

import type { StoreApi } from 'zustand';
import { TID_GAME_CURRENT_QUEST, TID_GAME_NEW_QUEST } from '../../../gen/defs/text';
import type { StoreRegistry } from '../../../stores/registry';
import type { DialogOp } from '../../../gen/proto/flyff/v1/npc_pb';
import type { DialogButton, DialogPage, DialogQuestEntry } from '../../../ui/windows/npc/ports';
import {
  parseDialogPage,
  sayMarkup,
  type DialogTextDiagnostics,
} from '../../../ui/windows/npc/dialogText';

/** `m_aKeyButton[32]`, `m_aWordButton[32]`. */
export const MAX_BUTTONS = 32;
/** The one-second pause between two requests (`m_timer.Set(SEC(1))`). */
export const REQUEST_PAUSE_MS = 1000;
/** The key of a quest entry that is shown but cannot be clicked (the player's level is too low). */
export const QUEST_NEXT_LEVEL = 'QUEST_NEXT_LEVEL';

export interface DialogState {
  /** The window is open (the state below belongs to it). */
  readonly open: boolean;
  readonly npcId: number;
  /** The `character.inc` key of the NPC. */
  readonly npcKey: string;
  /** The last applied `session_seq`. */
  readonly seq: number;
  /** `WIDC_GROUP_BOX_TITLE`: the word of the clicked key button (`''` = the default title). */
  readonly title: string;
  /** `m_bSay`. */
  readonly say: boolean;
  /** `m_nSelectKeyButton`. */
  readonly selectedKey: number;
  readonly keys: readonly DialogButton[];
  /** `m_aWordButton`. */
  readonly answers: readonly DialogButton[];
  /** `m_strArray`. */
  readonly pages: readonly DialogPage[];
  /** `m_nCurArray`. */
  readonly page: number;
  /** `m_string`: stays after a click until the next `say`. */
  readonly shown: DialogPage | null;
  readonly newQuests: readonly DialogQuestEntry[];
  readonly currentQuests: readonly DialogQuestEntry[];
  /** `host.now()` of the last request (`null` before the first): the click throttle. */
  readonly lastRequestMs: number | null;
  readonly version: number;
}

export const initialDialogState = (): DialogState => ({
  open: false,
  npcId: 0,
  npcKey: '',
  seq: 0,
  title: '',
  say: false,
  selectedKey: -1,
  keys: [],
  answers: [],
  pages: [],
  page: 0,
  shown: null,
  newQuests: [],
  currentQuests: [],
  lastRequestMs: null,
  version: 0,
});

export type DialogStore = StoreApi<DialogState>;

/** Scope `world`: the window and its NPC do not survive a world change. */
export function defineDialogStore(registry: StoreRegistry): DialogStore {
  const existing = registry.get('dialog');
  if (existing) return existing as unknown as DialogStore;
  return registry.defineStore<DialogState>({
    name: 'dialog',
    scope: 'world',
    owner: '45',
    initial: initialDialogState,
  });
}

/** What the reducers need from the rest of the client. */
export interface DialogEnv {
  /** The texts bundle (`TID_GAME_NEW_QUEST`, ...). */
  text(tid: number): string;
  /** The quest title and the level range of a quest id, `null` for an unknown quest (the quest data files are empty in V15). */
  quest(
    questId: number,
  ): { readonly title: string; readonly levelMin: number; readonly levelMax: number } | null;
  /** Reports malformed script text. */
  readonly diag?: DialogTextDiagnostics;
}

/** An effect of an operation outside the dialog (the window, the own stats, a rename window). */
export type DialogEffect =
  | { readonly kind: 'exit' }
  /** `INITSTAT` and its four single-stat forms: those stats become 15, the remaining growth points = `value`. */
  | {
      readonly kind: 'initStat';
      readonly stat: 'all' | 'str' | 'sta' | 'dex' | 'int';
      readonly value: number;
    }
  | { readonly kind: 'renamePlayer' }
  | {
      readonly kind: 'navigator';
      readonly world: number;
      readonly pos: readonly [number, number, number];
    };

const bump = (s: DialogState): number => s.version + 1;

/** `CWndDialog::EndSay`: the first page is shown again (every key, answer and text operation ends with it). */
function endSay(s: DialogState): DialogState {
  if (s.pages.length === 0) return s;
  return { ...s, shown: s.pages[0] ?? null, page: 0 };
}

/** `AddKeyButton`: a word that is already there is ignored (the key may repeat, the word may not). */
function addKey(s: DialogState, b: DialogButton): DialogState {
  if (s.keys.some((k) => k.word === b.word)) return s;
  if (s.keys.length >= MAX_BUTTONS) return s;
  return endSay({ ...s, keys: [...s.keys, b] });
}

/** `RemoveKeyButton`: the first button with that KEY goes (`EndSay` runs whether or not one was found). */
function removeKey(s: DialogState, key: string): DialogState {
  const i = s.keys.findIndex((k) => k.key === key);
  const keys = i < 0 ? s.keys : s.keys.filter((_, j) => j !== i);
  return endSay({ ...s, keys });
}

/** `MakeQuestKeyButton`: the pseudo key "New quest" / "Current quest" the first time a list gets an entry. */
function questKey(s: DialogState, word: string): DialogState {
  if (s.keys.some((k) => k.word === word)) return s;
  if (s.keys.length >= MAX_BUTTONS) return s;
  return endSay({ ...s, keys: [...s.keys, { word, key: word, param: 0, questId: 0 }] });
}

/** The entry text of a quest list (`AddQuestList`): the level range and the title in bold. */
function questEntry(env: DialogEnv, b: DialogButton): DialogQuestEntry {
  const q = env.quest(b.questId);
  const text = q
    ? `[#b${String(q.levelMin)}#nb~#b${String(q.levelMax)}#nb ] #b${b.word}#nb`
    : b.word;
  return { ...b, text, valid: b.key !== QUEST_NEXT_LEVEL };
}

function pageOf(env: DialogEnv, text: string, questId: number): DialogPage {
  const q = questId !== 0 ? env.quest(questId) : null;
  return parseDialogPage(sayMarkup(text, q?.title ?? null), questId, env.diag);
}

const btn = (m: { word: string; key: string; param: number; questId: number }): DialogButton => ({
  word: m.word,
  key: m.key,
  param: m.param,
  questId: m.questId,
});

/**
 * Applies the operations of one `DialogOps` message in order (`CDPClient::OnRunScriptFunc`). The caller already checked the
 * NPC and the sequence number. Returns the new state and the effects outside the dialog.
 */
export function applyDialogOps(
  state: DialogState,
  ops: readonly DialogOp[],
  seq: number,
  env: DialogEnv,
): { state: DialogState; effects: DialogEffect[] } {
  let s: DialogState = state;
  const effects: DialogEffect[] = [];
  for (const o of ops) {
    const op = o.op;
    switch (op.case) {
      case 'addKey':
        s = addKey(s, btn(op.value));
        break;
      case 'removeKey':
        s = removeKey(s, op.value.text);
        break;
      case 'removeAllKey':
        // both quest lists are emptied too (`RemoveAllKeyButton`)
        s = endSay({ ...s, keys: [], newQuests: [], currentQuests: [] });
        break;
      case 'addAnswer':
        if (s.answers.length < MAX_BUTTONS)
          s = endSay({ ...s, answers: [...s.answers, btn(op.value)] });
        break;
      case 'say':
      case 'sayQuest': {
        // `m_bSay = TRUE; Say(..)`: a page is appended and `EndSay` shows the first one again
        const page = pageOf(env, op.value.text, op.value.questId);
        s = endSay({ ...s, say: true, pages: [...s.pages, page] });
        break;
      }
      case 'newQuest': {
        s = questKey(s, env.text(TID_GAME_NEW_QUEST));
        const e = questEntry(env, btn(op.value));
        if (!s.newQuests.some((q) => q.word === e.word))
          s = { ...s, newQuests: [...s.newQuests, e] };
        break;
      }
      case 'currQuest': {
        s = questKey(s, env.text(TID_GAME_CURRENT_QUEST));
        const e = questEntry(env, btn(op.value));
        if (!s.currentQuests.some((q) => q.word === e.word)) {
          s = { ...s, currentQuests: [...s.currentQuests, e] };
        }
        break;
      }
      case 'setMark':
      case 'goMark':
      case 'endSay':
      case 'createGuild':
      case 'destroyGuild':
        break;
      case 'exit':
        effects.push({ kind: 'exit' });
        break;
      case 'initStat':
        effects.push({ kind: 'initStat', stat: 'all', value: op.value.value });
        break;
      case 'initStr':
        effects.push({ kind: 'initStat', stat: 'str', value: op.value.value });
        break;
      case 'initSta':
        effects.push({ kind: 'initStat', stat: 'sta', value: op.value.value });
        break;
      case 'initDex':
        effects.push({ kind: 'initStat', stat: 'dex', value: op.value.value });
        break;
      case 'initInt':
        effects.push({ kind: 'initStat', stat: 'int', value: op.value.value });
        break;
      case 'setNavigator': {
        const p = op.value.pos;
        effects.push({
          kind: 'navigator',
          world: op.value.value,
          pos: [p?.x ?? 0, p?.y ?? 0, p?.z ?? 0],
        });
        break;
      }
      case 'querySetPlayerName':
        effects.push({ kind: 'renamePlayer' });
        break;
      case undefined:
      default:
        break;
    }
  }
  return { state: { ...s, seq, version: bump(s) }, effects };
}

// ---- the stale-message rule ----------------------------------------------------------------------------------------------------

/** Whether a `DialogOps` message belongs to the open window: its NPC and a sequence number above the last applied one. */
export function acceptsOps(state: DialogState, npcObjId: number, seq: number): boolean {
  return state.open && state.npcId === npcObjId && seq > state.seq;
}

// ---- clicks ---------------------------------------------------------------------------------------------------------------------

/** `BeginText`: pages, the answers and the links are cleared; the shown page and the key buttons stay. */
export function beginText(s: DialogState): DialogState {
  return { ...s, pages: [], answers: [], page: 0 };
}

export interface ClickResult {
  readonly state: DialogState;
  /** The request to send, `null` when the click only changed the display (a quest list opens locally). */
  readonly request: {
    readonly key: string;
    readonly param: number;
    readonly questId: number;
  } | null;
}

/** The throttle (`m_timer.IsTimeOut()`): a click is ignored until one second after the previous request. */
export function mayClick(s: DialogState, now: number): boolean {
  return s.lastRequestMs === null || now - s.lastRequestMs >= REQUEST_PAUSE_MS;
}

function requested(
  s: DialogState,
  now: number,
  b: { key: string; param: number; questId: number },
): ClickResult {
  return {
    state: { ...s, lastRequestMs: now, version: bump(s) },
    request: { key: b.key, param: b.param, questId: b.questId },
  };
}

/** A click on key button `index` (`OnLButtonUp`, the key loop). */
export function clickKey(
  s: DialogState,
  index: number,
  now: number,
  env: DialogEnv,
): ClickResult | null {
  const k = s.keys[index];
  if (!k) return null;
  let n: DialogState = { ...s, selectedKey: index, title: k.word, say: false };
  n = beginText(n);
  const isNew = k.word === env.text(TID_GAME_NEW_QUEST);
  const isCurrent = k.word === env.text(TID_GAME_CURRENT_QUEST);
  if ((isNew && s.newQuests.length > 0) || (isCurrent && s.currentQuests.length > 0)) {
    // the list of quests opens in place of the text: nothing is requested
    return { state: { ...n, version: bump(n) }, request: null };
  }
  return requested(n, now, k);
}

/** A click on answer `index` (an answer button or an underlined answer word). */
export function clickAnswer(s: DialogState, index: number, now: number): ClickResult | null {
  const a = s.answers[index];
  if (!a) return null;
  return requested(beginText(s), now, a);
}

/** A click on a clickable word of the text: `RunScript(key, 0, 0)`. */
export function clickLink(s: DialogState, key: string, now: number): ClickResult {
  return requested(beginText(s), now, { key, param: 0, questId: 0 });
}

/** A click on an entry of a quest list: its key, `param` and quest id go back; a "next level" entry is not clickable. */
export function clickQuest(
  s: DialogState,
  list: 'new' | 'current',
  index: number,
  now: number,
): ClickResult | null {
  const e = (list === 'new' ? s.newQuests : s.currentQuests)[index];
  if (!e) return null;
  if (list === 'new' && e.key === QUEST_NEXT_LEVEL) return null;
  return requested(beginText({ ...s, say: false }), now, e);
}

/** `Next` (`WIDC_BUTTON1`): the next page. */
export function nextPage(s: DialogState): DialogState {
  if (s.page >= s.pages.length - 1) return s;
  const page = s.page + 1;
  return { ...s, page, shown: s.pages[page] ?? s.shown, version: bump(s) };
}
