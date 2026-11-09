// `DialogSurface`: the display-only control that paints what `CWndDialog::OnDraw` paints by hand (WndDialog.cpp:56-160): the
// NPC portrait, the page text, the key buttons, the answer links, the clickable words, the empty-quest message and the two
// quest lists with their icons. The window computes all positions (`dialogLayout.ts`) and pushes them as plain data; the
// surface never decides anything (hit tests and clicks are the window's, which gets the pointer events of the frame).

import { type Point, type Rect } from '../../layout/geometry';
import { WTYPE_CUSTOM } from '../../layout/styles';
import { viewOf } from '../../controls/bind';
import type { RichLine } from '../../text/layout';
import { Control, type ControlViewState } from '../../wnd/Control';
import { DialogSurfaceView } from './DialogSurfaceView';

/** A word button (key button, answer link, text link) with its hover/selection flags. */
export interface SurfaceWord {
  readonly word: string;
  readonly rect: Rect;
  /** `bStatus`: the pointer is over it. */
  readonly hover: boolean;
  /** The purple key button of the last click (`m_nSelectKeyButton`). */
  readonly selected?: boolean;
}

export interface SurfaceQuestRow {
  readonly rect: Rect;
  /** The laid-out markup of the entry. */
  readonly lines: readonly RichLine[];
  /** The icon file (`QuestUiPaperGreen.tga`, ...). */
  readonly icon: string;
  readonly hover: boolean;
}

export interface SurfaceQuestList {
  readonly area: Rect;
  readonly rows: readonly SurfaceQuestRow[];
}

export interface DialogSurfaceState extends ControlViewState {
  portrait: { readonly path: string; readonly at: Point } | null;
  body: { readonly lines: readonly RichLine[]; readonly at: Point; readonly clip: Rect } | null;
  /** `TID_GAME_EMPTY_NEW_QUEST` / `..._CURRENT_QUEST` at (10, 10) of the text area. */
  empty: { readonly text: string; readonly at: Point } | null;
  keys: readonly SurfaceWord[];
  answers: readonly SurfaceWord[];
  /** The clickable words of the text; a hovered word (and the other half of a wrapped one) is redrawn blue and underlined. */
  links: readonly SurfaceWord[];
  quests: SurfaceQuestList | null;
}

const same = (a: readonly SurfaceWord[], b: readonly SurfaceWord[]): boolean =>
  a.length === b.length &&
  a.every(
    (w, i) =>
      w.word === b[i]?.word &&
      w.hover === b[i].hover &&
      w.selected === b[i].selected &&
      w.rect.l === b[i].rect.l &&
      w.rect.t === b[i].rect.t &&
      w.rect.r === b[i].rect.r &&
      w.rect.b === b[i].rect.b,
  );

export class DialogSurface extends Control<DialogSurfaceState> {
  constructor() {
    super(WTYPE_CUSTOM, null, {
      portrait: null,
      body: null,
      empty: null,
      keys: [],
      answers: [],
      links: [],
      quests: null,
    });
    this.view = viewOf<DialogSurface>(DialogSurfaceView);
  }

  /** A display: presses pass to the window (the frame hit-tests the words itself). */
  override isPickupSpace(): boolean {
    return false;
  }

  /** Diffed setters: an unchanged value emits nothing. */
  setPortrait(portrait: DialogSurfaceState['portrait']): void {
    const cur = this.state.get().portrait;
    if (
      cur?.path === portrait?.path &&
      cur?.at.x === portrait?.at.x &&
      cur?.at.y === portrait?.at.y
    )
      return;
    this.state.patch({ portrait });
  }

  setBody(body: DialogSurfaceState['body']): void {
    if (this.state.get().body === body) return;
    this.state.patch({ body });
  }

  setEmpty(empty: DialogSurfaceState['empty']): void {
    const cur = this.state.get().empty;
    if (cur?.text === empty?.text) return;
    this.state.patch({ empty });
  }

  setKeys(keys: readonly SurfaceWord[]): void {
    if (same(this.state.get().keys, keys)) return;
    this.state.patch({ keys });
  }

  setAnswers(answers: readonly SurfaceWord[]): void {
    if (same(this.state.get().answers, answers)) return;
    this.state.patch({ answers });
  }

  setLinks(links: readonly SurfaceWord[]): void {
    if (same(this.state.get().links, links)) return;
    this.state.patch({ links });
  }

  setQuests(quests: SurfaceQuestList | null): void {
    if (this.state.get().quests === quests) return;
    this.state.patch({ quests });
  }
}
