// The checks of the PIN windows (`CWndConfirmBank::OnChildNotify`, `CWndBankPassword::OnChildNotify`, WndBank.cpp:476-700): pure functions over the
// typed texts, so that the windows and their tests agree on the order of the refusals.

import { TID_DIAG_0022, TID_DIAG_0025 } from '../../../gen/defs/text';
import { PIN_LENGTH } from './ports';

/** `isdigit2` on every character and `strlen == 4`: the PIN the server accepts. */
export function isPin(text: string): boolean {
  return text.length === PIN_LENGTH && /^[0-9]+$/.test(text);
}

/** `CWndConfirmBank` OK: `0` = send, else the text id of the message box ("Password should have 4 digits"). */
export function checkConfirmPin(text: string): number {
  return isPin(text) ? 0 : TID_DIAG_0025;
}

/** What `CWndBankPassword` does with its three fields on OK. */
export type ChangePinCheck =
  /** All good: send `ChangeBankPass(old, new)`. */
  | { readonly ok: true }
  | {
      readonly ok: false;
      /** The text id of the message box. */
      readonly tid: number;
      /** Which fields are emptied: the old one is reset (to "0000" in the first-PIN mode), the new one and the confirmation always. */
      readonly resetOld: boolean;
    };

/**
 * The order of the original: all three must be four characters long (else `TID_DIAG_0025`, everything reset); the new PIN must equal the
 * confirmation (else `TID_DIAG_0022`, the new PIN and the confirmation are emptied, the old one stays); all twelve characters must be digits
 * (else `TID_DIAG_0025`, everything reset).
 */
export function checkChangePin(oldPin: string, newPin: string, confirm: string): ChangePinCheck {
  if (
    oldPin.length !== PIN_LENGTH ||
    newPin.length !== PIN_LENGTH ||
    confirm.length !== PIN_LENGTH
  ) {
    return { ok: false, tid: TID_DIAG_0025, resetOld: true };
  }
  if (newPin !== confirm) return { ok: false, tid: TID_DIAG_0022, resetOld: false };
  if (!isPin(oldPin) || !isPin(newPin) || !isPin(confirm)) {
    return { ok: false, tid: TID_DIAG_0025, resetOld: true };
  }
  return { ok: true };
}
