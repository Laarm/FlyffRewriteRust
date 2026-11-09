// The geometry of the NPC dialog (`src:_Interface/WndDialog.cpp`, Plan/45 §11.3; WP-UI-24): where the key buttons, the answer
// links, the answer image buttons and the clickable words of the text go. Pure functions of the text metrics, tested apart
// from the window. All rects are in client coordinates of `APP_DIALOG_EX` (the layout rects of `WIDC_CUSTOM1` / `WIDC_CUSTOM2`).

import { type Rect } from '../../layout/geometry';
import type { RichLine } from '../../text/layout';

/** The four answers that are image buttons (`buttYes.tga`, ...); every other answer word is an underlined text link. */
export const ANSWER_IMAGES: Readonly<
  Record<string, { readonly id: string; readonly texture: string }>
> = {
  __YES__: { id: 'WIDC_YES', texture: 'buttYes.tga' },
  __NO__: { id: 'WIDC_NO', texture: 'buttNo.tga' },
  __CANCEL__: { id: 'WIDC_CANCEL', texture: 'buttCancel.tga' },
  __OK__: { id: 'WIDC_OK', texture: 'buttOk.tga' },
};

export const isImageAnswer = (word: string): boolean => Object.hasOwn(ANSWER_IMAGES, word);

/** Pixels between two image answers (`x += 90`) and the width the C++ gives them before `FitTextureSize`. */
export const ANSWER_PITCH = 90;
export const ANSWER_WIDTH = 70;

/** The padding of a key button (`__IMPROVE_QUEST_INTERFACE`: `size.cx += 16`) and of an answer text link (`+= 10`). */
export const KEY_PAD = 16;
export const ANSWER_LINK_PAD = 10;

/** The extra pixels below a text line (`TextOut_EditString(.., nLineSpace = 6)`, `GetMaxHeight() + 6`). */
export const LINE_SPACE = 6;

export interface Measure {
  /** The width of a string in the text font (`GetTextExtent(..).cx`). */
  width(text: string): number;
  /** The text font's line cell (`GetTextExtent(..).cy`, `GetMaxHeight()`). */
  readonly cell: number;
}

/** One word button (key button or answer link) with its hit rect. */
export interface WordRect {
  readonly word: string;
  readonly rect: Rect;
}

/**
 * `MakeKeyButton`: the key buttons flow left to right inside `area` (`WIDC_CUSTOM2`), each as wide as its word plus 16 px,
 * wrapping at the right edge; the "New quest" / "Current quest" pseudo buttons start a new row the first time one of them is
 * placed (`bQuestButtonLinefeedSwitch`).
 */
export function layoutKeyButtons(
  words: readonly string[],
  area: Rect,
  m: Measure,
  isQuestWord: (word: string) => boolean,
): WordRect[] {
  const pitch = m.cell + LINE_SPACE;
  const out: WordRect[] = [];
  let x = 0;
  let y = 0;
  let linefeed = false;
  for (const word of words) {
    const cx = m.width(word) + KEY_PAD;
    const cy = m.cell;
    let rect: Rect = {
      l: area.l + x,
      t: area.t + y * pitch,
      r: area.l + x + cx,
      b: area.t + y * pitch + cy,
    };
    if (rect.r > area.r) {
      x = 0;
      y += 1;
      rect = { l: area.l, t: area.t + y * pitch, r: area.l + cx, b: area.t + y * pitch + cy };
    } else if (isQuestWord(word) && !linefeed) {
      x = 0;
      y += 1;
      rect = { l: area.l, t: area.t + y * pitch, r: area.l + cx, b: area.t + y * pitch + cy };
      linefeed = true;
    }
    out.push({ word, rect });
    x += cx;
  }
  return out;
}

export interface AnswerLayout {
  /** The text links (everything but the four image answers), with their rect; `index` is the position in the answer list. */
  readonly links: readonly (WordRect & { readonly index: number })[];
  /** The image buttons: the answer index, the button id name, the texture and the rect before `FitTextureSize`. */
  readonly images: readonly {
    readonly index: number;
    readonly id: string;
    readonly texture: string;
    readonly rect: Rect;
  }[];
}

/**
 * `MakeAnswerButton` (only on the last page): the text links flow under the text, `lineCount` lines below `area.t`; the image
 * answers are centred in the area, `ANSWER_PITCH` apart, 20 px above its bottom. KEEP-QUIRK: the x position advances by 90 for
 * EVERY answer, a text link in between included, while the centring counts the image buttons only.
 */
export function layoutAnswers(
  words: readonly string[],
  lineCount: number,
  area: Rect,
  m: Measure,
): AnswerLayout {
  const pitch = m.cell + LINE_SPACE;
  const yBegin = area.t + lineCount * pitch + pitch;
  const links: (WordRect & { index: number })[] = [];
  let x = 0;
  let y = 0;
  let imageCount = 0;
  words.forEach((word, index) => {
    if (isImageAnswer(word)) {
      imageCount += 1;
      return;
    }
    const cx = m.width(word) + ANSWER_LINK_PAD;
    const cy = m.cell;
    let rect: Rect = {
      l: area.l + x,
      t: yBegin + y * pitch,
      r: area.l + x + cx,
      // KEEP-QUIRK: the original's bottom is `yBegin + y * size.cy + dwMaxHeight` (the row count scales with the text height)
      b: yBegin + y * cy + pitch,
    };
    if (rect.r > area.r) {
      x = 0;
      y += 1;
      rect = {
        l: area.l,
        t: yBegin + y * pitch,
        r: area.l + cx,
        b: yBegin + y * cy + pitch,
      };
    }
    x += cx;
    links.push({ word, rect, index });
  });
  let ix = area.l + Math.floor((area.r - area.l) / 2) - Math.floor((ANSWER_PITCH * imageCount) / 2);
  const images: AnswerLayout['images'][number][] = [];
  words.forEach((word, index) => {
    const img = ANSWER_IMAGES[word];
    if (img) {
      images.push({
        index,
        id: img.id,
        texture: img.texture,
        rect: { l: ix, t: area.b - 20, r: ix + ANSWER_WIDTH, b: area.b + 10 },
      });
    }
    ix += ANSWER_PITCH;
  });
  return { links, images };
}

/** One clickable word of the text (`m_aContextButton`). */
export interface ContextLink {
  readonly word: string;
  /** The key the word stands for (`m_mapWordToOriginal`; the two halves of a wrapped word share the key). */
  readonly key: string;
  readonly rect: Rect;
  /** The index of the other half of a word that wraps onto the next line, -1 for a word on one line. */
  readonly link: number;
}

/**
 * `MakeContextButton`: walks the laid-out lines of the shown page character by character; the marks say where a link word
 * starts (1, 4) and ends (3, 4). A word that wraps is two buttons that highlight together and share the key.
 */
export function layoutContextLinks(
  lines: readonly RichLine[],
  marks: readonly number[],
  keyOf: ReadonlyMap<string, string>,
  area: Rect,
  m: Measure,
  charWidth: (ch: string) => number,
): ContextLink[] {
  const pitch = m.cell + LINE_SPACE;
  const out: { word: string; key: string; rect: Rect; link: number }[] = [];
  let y = area.t;
  let rectWord: Rect = { l: 0, t: 0, r: 0, b: 0 };
  let linked = false;
  // `dwMark` of the C++ lives outside both loops: an empty line keeps the mark of the line before
  let mark = 0;
  for (const line of lines) {
    let x = area.l;
    let word = '';
    let j = 0;
    for (const ch of lineChars(line)) {
      const w = charWidth(ch);
      mark = marks[line.offset + j] ?? 0;
      j += 1;
      if (mark !== 0) {
        word += ch;
        rectWord = { ...rectWord, r: rectWord.r + w };
      }
      if (mark === 1 || mark === 4) {
        rectWord = { l: x, t: y, r: x + w, b: y + pitch };
        if (mark === 4) mark = 3;
      }
      if (mark === 3) {
        let full = word;
        let link = -1;
        if (linked) {
          const first = out[out.length - 1];
          link = out.length - 1;
          full = (first?.word ?? '') + word;
        }
        const key = keyOf.get(full) ?? '';
        out.push({ word, key, rect: rectWord, link });
        if (linked) {
          // the first half takes the key of the whole word
          const first = out[out.length - 2];
          if (first) out[out.length - 2] = { ...first, key };
        }
        linked = false;
        word = '';
      }
      x += w;
    }
    y += pitch;
    // the line ended inside a link word: its rest continues on the next line
    if (mark === 1 || mark === 2) {
      linked = true;
      out.push({ word, key: keyOf.get(word) ?? '', rect: rectWord, link: out.length + 1 });
      rectWord = { l: area.l, t: y, r: area.l, b: y + pitch };
    }
  }
  return out;
}

/** The characters of a laid-out line without the `\n` (the runs hold the text of the line only). */
function lineChars(line: RichLine): string[] {
  const chars: string[] = [];
  for (const run of line.runs) for (const ch of run.text) chars.push(ch);
  return chars;
}
