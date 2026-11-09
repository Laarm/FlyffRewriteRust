// The cursor table (`Neuz.h:7-25`, `Neuz/WinMain.rc`, `CNeuzApp::InitCursor`; Plan/42 §15, ref/22 §6.10): cursor id -> the
// stem of the `.cur` file the asset pipeline turns into `/gen/cursor/<fp>/<stem>.png` (lower case). 43 imports `CUR` for
// its world cursor table (`OnSetCursor`, ref/21 §8.7).

/** `CUR_*` ids. */
export const CUR = {
  ATTACK: 0,
  BASE: 1,
  CAMERA: 2,
  CONTROL: 3,
  DELAY: 4,
  EDIT: 5,
  GETITEM: 6,
  HELP: 7,
  NO: 8,
  NPC: 9,
  SELECT: 10,
  RESIZE_HORI: 11,
  RESIZE_VERT: 12,
  RESIZE_HV1: 13,
  RESIZE_HV2: 14,
  DOOR: 15,
  GATE: 16,
  LEVER: 17,
  HAMMER: 18,
} as const;

export type CursorId = (typeof CUR)[keyof typeof CUR];

/** One row of the table: the constant's name, the file stem and a CSS keyword for when the file is missing. */
export interface CursorInfo {
  id: CursorId;
  name: string;
  /** The `.cur` file without extension, lower case. */
  stem: string;
}

/** The 19 cursors in id order (`CURSORS[id]`). */
export const CURSORS: readonly CursorInfo[] = [
  { id: 0, name: 'CUR_ATTACK', stem: 'curattack' },
  { id: 1, name: 'CUR_BASE', stem: 'curbase' },
  { id: 2, name: 'CUR_CAMERA', stem: 'curcamera' },
  { id: 3, name: 'CUR_CONTROL', stem: 'curcontrol' },
  { id: 4, name: 'CUR_DELAY', stem: 'curdelay' },
  { id: 5, name: 'CUR_EDIT', stem: 'edit' },
  { id: 6, name: 'CUR_GETITEM', stem: 'curgetitem' },
  { id: 7, name: 'CUR_HELP', stem: 'cursor2' },
  { id: 8, name: 'CUR_NO', stem: 'cursor3' },
  { id: 9, name: 'CUR_NPC', stem: 'curnpc' },
  { id: 10, name: 'CUR_SELECT', stem: 'curselect' },
  { id: 11, name: 'CUR_RESIZE_HORI', stem: 'hori' },
  { id: 12, name: 'CUR_RESIZE_VERT', stem: 'resize_h' },
  { id: 13, name: 'CUR_RESIZE_HV1', stem: 'cur00001' },
  { id: 14, name: 'CUR_RESIZE_HV2', stem: 'cur00002' },
  { id: 15, name: 'CUR_DOOR', stem: 'curdoor' },
  { id: 16, name: 'CUR_GATE', stem: 'curgate' },
  { id: 17, name: 'CUR_LEVER', stem: 'curlever' },
  { id: 18, name: 'CUR_HAMMER', stem: 'anvil' },
];

/** The stem of a cursor id, `null` for an unknown id. */
export function cursorStem(id: number): string | null {
  return CURSORS[id]?.stem ?? null;
}

/** The cursor of a resize zone (`GetResizeDir`: 1/2 top, bottom; 3/4 left, right; 5/8 TL, BR; 6/7 TR, BL). */
export function resizeCursor(dir: number): CursorId | null {
  switch (dir) {
    case 1:
    case 2:
      return CUR.RESIZE_VERT;
    case 3:
    case 4:
      return CUR.RESIZE_HORI;
    case 5:
    case 8:
      return CUR.RESIZE_HV1;
    case 6:
    case 7:
      return CUR.RESIZE_HV2;
    default:
      return null;
  }
}
