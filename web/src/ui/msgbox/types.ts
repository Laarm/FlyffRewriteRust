// The constants of the message boxes (`MB_*`, `ID*` of Win32 as the original uses them; Plan/42 §14).

/** `MB_*`: which buttons a box has (`MB_CANCEL` is the project's own 6). */
export const MB = {
  OK: 0,
  OKCANCEL: 1,
  ABORTRETRYIGNORE: 2,
  YESNOCANCEL: 3,
  YESNO: 4,
  RETRYCANCEL: 5,
  CANCEL: 6,
} as const;
export type MbType = (typeof MB)[keyof typeof MB];

/** `IDOK` ... `IDNO`: the id of the button that closed the box. */
export const MBID = { OK: 1, CANCEL: 2, ABORT: 3, RETRY: 4, IGNORE: 5, YES: 6, NO: 7 } as const;
