// The three FREE lobby rule exports of the wasm module (Plan/45 §2.7, WP-CFEAT-04; `crates/flyff-wasm/src/lobby_rules.rs`):
// character-name, bank-PIN and default-hair-colour rules shared with the server (`flyff-rules`). They need neither the
// bundles nor `RulesContext`, so the character-create window of the title screens can call them. Feature-detected:
// `createWasmLobbyRules()` is `null` while the module is not instantiated in this realm or when the build has no such
// exports (the window then skips the pre-checks and shows the server's `LobbyError` text, 45 §5.2).

import * as pkg from './pkg/flyff_wasm';
import { wasmMemory } from './loader';

/** The result of the name rule: `name` (trimmed) on success, `tid` (the client text id) on failure. */
export interface NameCheck {
  readonly name?: string;
  readonly tid?: number;
}

/** The facade of 45 §2.7 (`LobbyRules`). */
export interface LobbyRules {
  /** `names`: the JSON of the name rules, the shape of the `names` bundle (`encodeNameRules`). */
  validateCharacterName(raw: string, names: Uint8Array, min: number, max: number): NameCheck;
  /** 0 when the PIN is acceptable, else the `TID_2ND_PASSWORD_INPUT_ERROR02` / `...03` text id (12 §9.4). */
  validateBankPin(pin: string): number;
  /** `0xAARRGGBB` of the table of 12 §9.2, `undefined` outside sex 0..1 / hair mesh 0..9. */
  defaultHairColor(sex: number, hairMesh: number): number | undefined;
}

/** The name rules of the `names` bundle as the bytes `validateCharacterName` takes. */
export function encodeNameRules(rules: {
  readonly invalid_substrings: readonly string[];
  readonly allowed_letters: string | null;
}): Uint8Array {
  return new TextEncoder().encode(
    JSON.stringify({
      invalid_substrings: rules.invalid_substrings,
      allowed_letters: rules.allowed_letters,
    }),
  );
}

interface Exports {
  validate_character_name: (raw: string, names: Uint8Array, min: number, max: number) => unknown;
  validate_bank_pin: (pin: string) => number;
  default_hair_color: (sex: number, hairMesh: number) => number | undefined;
}

function exportsOf(module: object): Exports | null {
  const m = module as Partial<Record<keyof Exports, unknown>>;
  if (
    typeof m.validate_character_name !== 'function' ||
    typeof m.validate_bank_pin !== 'function' ||
    typeof m.default_hair_color !== 'function'
  ) {
    return null;
  }
  return module as Exports;
}

/** `LobbyRules` over the instantiated module, or `null` (not instantiated, or the exports are missing). */
export function createWasmLobbyRules(module: object = pkg): LobbyRules | null {
  const e = exportsOf(module);
  if (e === null || (module === pkg && wasmMemory() === null)) return null;
  return {
    validateCharacterName(raw, names, min, max) {
      const r = e.validate_character_name(raw, names, min, max) as {
        name?: unknown;
        tid?: unknown;
      } | null;
      const out: { name?: string; tid?: number } = {};
      if (r && typeof r.name === 'string') out.name = r.name;
      if (r && typeof r.tid === 'number') out.tid = r.tid;
      return out;
    },
    validateBankPin: (pin) => e.validate_bank_pin(pin),
    defaultHairColor: (sex, hairMesh) => e.default_hair_color(sex, hairMesh) ?? undefined,
  };
}
