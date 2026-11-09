// `lobbyRules` facade (Plan/45 §2.7): the free wasm lobby rules (name, bank PIN, default hair colour) plus the name
// rules bytes of the `names` bundle. `FeatureContext.lobbyRules` is `null` while the module does not export them; the
// create window then skips the name/format pre-checks (45 §5.2).

import type { BundleDataMap } from '../../gen/bundles';
import {
  createWasmLobbyRules,
  encodeNameRules,
  type LobbyRules,
  type NameCheck,
} from '../../wasm/lobbyRules';

export type { LobbyRules, NameCheck };

/** The name-rule bytes of the `names` bundle (cached per bundle object). */
const encoded = new WeakMap<object, Uint8Array>();
export function nameRulesBytes(names: BundleDataMap['names']): Uint8Array {
  let bytes = encoded.get(names);
  if (!bytes) {
    bytes = encodeNameRules({
      invalid_substrings: names.invalid_substrings,
      allowed_letters: names.allowed_letters,
    });
    encoded.set(names, bytes);
  }
  return bytes;
}

/** Feature-detects the exports once the wasm main instance exists; `null` otherwise (retried on the next call). */
let cached: LobbyRules | null = null;
export function detectLobbyRules(): LobbyRules | null {
  cached ??= createWasmLobbyRules();
  return cached;
}

/** Test seam: forget the detection result. */
export function resetLobbyRulesDetection(): void {
  cached = null;
}
