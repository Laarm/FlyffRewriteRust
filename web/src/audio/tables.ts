// Sound and music tables (Plan/44 §5, WP-AUDIO-02 table part): the `sounds` and `music` bundles as lookup tables with
// the path normalisation of §5.5. The buffer cache of §6 (`loader.ts`) is WP-AUDIO-02's second half and not built here.

import { normalizeResourcePath } from '../assets/paths';

export type SoundId = number;
export type MusicId = number;

/** `NULL_ID`: `0xFFFFFFFF` (also `-1` in a signed read) is never a music id. */
const NULL_MUSIC_ID = 0xffff_ffff;

export type SoundEntry =
  | { readonly kind: 'file'; readonly path: string }
  | { readonly kind: 'silent' }
  | { readonly kind: 'missing'; readonly path: string | null };

/** The result of a music lookup (§5.3): absent id, a missing file (`null` in the bundle) or a path. */
export type MusicEntry =
  | { readonly kind: 'absent' }
  | { readonly kind: 'missing'; readonly alias: string | null }
  | { readonly kind: 'file'; readonly path: string };

/** `music/bgm4dudarkon.ogg -> music/bgmdu4invisible.ogg` and the second fixable reference (KQ-30, `bgmAliasFix`). */
export const BGM_ALIASES: Readonly<Record<string, string>> = {
  'music/bgm4dudarkon.ogg': 'music/bgmdu4invisible.ogg',
  'music/bgmfidarkon.ogg': 'music/bgmfi3darkon1.ogg',
};

/**
 * The same two substitutions by music id: the `music` bundle carries `null` (no path) for a track whose file does not exist
 * (44 Open question 9a), so the path-keyed table above cannot see them. `BGM_DU_INVISIBLE 70` and `BGM_FI_DARKON3 124`;
 * `BGM_BA_SAINTMORNING 50` has no substitute and stays silent.
 */
export const BGM_ALIASES_BY_ID: Readonly<Record<number, string>> = {
  70: 'music/bgmdu4invisible.ogg',
  124: 'music/bgmfi3darkon1.ogg',
};

function normalize(value: string): string | null {
  return normalizeResourcePath(value);
}

/** `Map<SoundId, SoundEntry>` built from the `sounds` bundle (§5.1). */
export class SoundTable {
  readonly #entries = new Map<SoundId, SoundEntry>();

  constructor(sounds: Readonly<Record<string, unknown>> | null | undefined) {
    for (const [key, value] of Object.entries(sounds ?? {})) {
      const id = Number(key);
      if (!Number.isInteger(id)) continue;
      if (value === '') this.#entries.set(id, { kind: 'silent' });
      else if (value === null) this.#entries.set(id, { kind: 'missing', path: null });
      else if (typeof value === 'string') {
        const path = normalize(value);
        this.#entries.set(id, path === null ? { kind: 'silent' } : { kind: 'file', path });
      }
    }
  }

  get size(): number {
    return this.#entries.size;
  }

  /** `id <= 0` is rejected before the lookup (KEEP-QUIRK: `SND_NONE` maps to a file but never plays). */
  lookup(id: SoundId): SoundEntry | undefined {
    if (!Number.isInteger(id) || id <= 0) return undefined;
    return this.#entries.get(id);
  }
}

/**
 * A literal file of `Sound/` (§5.2): `normalizeResourcePath("Sound/" + file)`. A name that is empty or climbs out of
 * `Sound/` (`..`) is refused (`null`): the file of a server `PlaySound` is data, not a path to trust.
 */
export function soundFilePath(file: string): string | null {
  const path = normalize(`Sound/${file}`);
  return path !== null && path.startsWith('sound/') && path.length > 'sound/'.length ? path : null;
}

/** `Map<MusicId, MusicEntry>` from the `music` bundle (§5.3). */
export class MusicTable {
  readonly #entries = new Map<MusicId, string | null>();
  readonly #aliasFix: () => boolean;

  constructor(
    music: Readonly<Record<string, unknown>> | null | undefined,
    aliasFix: () => boolean = () => false,
  ) {
    this.#aliasFix = aliasFix;
    for (const [key, value] of Object.entries(music ?? {})) {
      const id = Number(key);
      if (!Number.isInteger(id)) continue;
      if (value === null) this.#entries.set(id, null);
      else if (typeof value === 'string') {
        const path = normalize(value);
        if (path !== null) this.#entries.set(id, path);
      }
    }
  }

  get size(): number {
    return this.#entries.size;
  }

  lookup(id: MusicId): MusicEntry {
    if (!Number.isFinite(id) || id === 0 || id === NULL_MUSIC_ID || id === -1) {
      return { kind: 'absent' };
    }
    const entry = this.#entries.get(id);
    if (entry === undefined) return { kind: 'absent' };
    if (entry === null) {
      const alias = this.#aliasFix() ? BGM_ALIASES_BY_ID[id] : undefined;
      return alias === undefined ? { kind: 'missing', alias: null } : { kind: 'file', path: alias };
    }
    if (this.#aliasFix()) {
      const alias = BGM_ALIASES[entry];
      if (alias !== undefined) return { kind: 'file', path: alias };
    }
    return { kind: 'file', path: entry };
  }
}
