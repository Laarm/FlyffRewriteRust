// The pure model of the hair and face shops (Plan/45 §17; WP-CFEAT-19, WP-UI-35): the draft look, the paging of the preview cards and the
// prices. No window and no network here; the module wires them. The paging is the original's (`CWndBeautyShop`, `CWndFaceShop`,
// `src:_Interface/WndShop.cpp`):
//   * the four hair cards are consecutive styles from a PAGE (`m_dwHairMesh`, starting at the current style) wrapping over all ten; the
//     arrows move the page, a click selects a style (`m_dwSelectHairMesh`), the two are independent;
//   * the friendly row of the face shop wraps over all eleven heads, the new row over the last six (heads 5..10: `MAX_HEAD`, 6 on the
//     1-based `m_dwNewFace`); the current head decides which row starts on it.

/** The hair meshes 0..9 and the head meshes 0..10 (`SetHairReq.hair` < 10, `ChangeFaceReq.face` < 11: VR-026). */
export const HAIR_MESHES = 10;
export const HEAD_MESHES = 11;
/** `CHANGE_FACE_COST`; the hair prices come from `rules.hairCost` (`HAIR_COST` 2 000 000, `HAIRCOLOR_COST` 4 000 000 as the fallback). */
export const CHANGE_FACE_COST = 1_000_000;
export const HAIR_COST = 2_000_000;
export const HAIRCOLOR_COST = 4_000_000;
/** The cards of a row. */
export const CARDS = 4;
/** The first head of the new-face row (`m_dwNewFace` starts at 6, 1-based). */
export const NEW_FACE_FIRST = 5;
/** The heads that a new-face row wraps over. */
export const NEW_FACE_COUNT = HEAD_MESHES - NEW_FACE_FIRST;

export type Rgb = readonly [number, number, number];

export interface HairDraft {
  /** The selected style (`m_dwSelectHairMesh - 1`). */
  readonly mesh: number;
  /** The first style of the four cards (`m_dwHairMesh - 1`). */
  readonly page: number;
  readonly rgb: Rgb;
}

export interface FaceDraft {
  /** The selected face (`m_nSelectedFace - 1`). */
  readonly face: number;
  /** The first face of each row of cards (they page independently): `m_dwFriendshipFace - 1` and `m_dwNewFace - 1`. */
  readonly friendlyStart: number;
  readonly freshStart: number;
}

/** `n` wrapped into `0..count - 1`. */
export const wrap = (n: number, count: number): number => ((n % count) + count) % count;

export const clampChannel = (v: number): number => Math.min(255, Math.max(0, Math.round(v)));

/** The four cards of a page: `CARDS` consecutive styles from `page` (`DrawHairKind`: `HairNum > MAX_HAIR ? 1 : HairNum`). */
export function hairCards(page: number, count = HAIR_MESHES): readonly number[] {
  return Array.from({ length: CARDS }, (_, i) => wrap(page + i, count));
}

/** The friendly row: `CARDS` consecutive heads from `start`, wrapping over all heads (`FaceNum > MAX_DEFAULT_HEAD ? 1 : FaceNum`). */
export function faceRow(start: number, count = HEAD_MESHES): readonly number[] {
  return Array.from({ length: CARDS }, (_, i) => wrap(start + i, count));
}

/** The new row: `CARDS` consecutive heads from `start`, wrapping over the heads 5..10 (`FaceNum > MAX_HEAD ? 6 : FaceNum`). */
export function newFaceRow(start: number): readonly number[] {
  return Array.from(
    { length: CARDS },
    (_, i) => NEW_FACE_FIRST + wrap(start - NEW_FACE_FIRST + i, NEW_FACE_COUNT),
  );
}

/** `0x00RRGGBB` of a draft colour, and back. */
export const rgbToNumber = (rgb: Rgb): number => (rgb[0] << 16) | (rgb[1] << 8) | rgb[2];
export const numberToRgb = (n: number): Rgb => [(n >> 16) & 255, (n >> 8) & 255, n & 255];

/** `Initialize` / `WIDC_BUTTON1`: the page starts on the current style and so does the selection. */
export function initialHairDraft(mesh: number, color: number): HairDraft {
  const m = wrap(mesh, HAIR_MESHES);
  return { mesh: m, page: m, rgb: numberToRgb(color) };
}

/** `WIDC_HAIRSTYLE_LEFT` / `RIGHT`: the cards page by one style, the selection stays. */
export function stepPage(d: HairDraft, delta: -1 | 1): HairDraft {
  return { ...d, page: wrap(d.page + delta, HAIR_MESHES) };
}

/**
 * `Initialize` / `WIDC_BTN_RESET`: a head below 6 starts the friendly row on it and the new row on 6; a head from 6 on starts the new row on it
 * and the friendly row on 1 (1-based numbers).
 */
export function initialFaceDraft(face: number): FaceDraft {
  const f = wrap(face, HEAD_MESHES);
  return f < NEW_FACE_FIRST + 1
    ? { face: f, friendlyStart: f, freshStart: NEW_FACE_FIRST }
    : { face: f, friendlyStart: 0, freshStart: f };
}

export function stepFriendly(d: FaceDraft, delta: -1 | 1): FaceDraft {
  return { ...d, friendlyStart: wrap(d.friendlyStart + delta, HEAD_MESHES) };
}

export function stepFresh(d: FaceDraft, delta: -1 | 1): FaceDraft {
  return {
    ...d,
    freshStart: NEW_FACE_FIRST + wrap(d.freshStart - NEW_FACE_FIRST + delta, NEW_FACE_COUNT),
  };
}

/** Whether the draft differs from the current look. */
export const hairChanged = (d: HairDraft, mesh: number, color: number): boolean =>
  d.mesh !== wrap(mesh, HAIR_MESHES) || rgbToNumber(d.rgb) !== (color & 0xffffff);
