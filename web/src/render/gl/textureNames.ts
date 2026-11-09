// Per-instance texture name rules (Plan/41 section 6.3, ref/18 section 8.5). The original mutated the SHARED
// `CObject3D`; here the rules are pure functions over texture file names and the caller (model instance) picks
// the resulting texture per instance (FIX, PL-137).
//
//   texture set   `X.ext`                         -> `X-etNN.ext`       (NN = two digits of the set number 1..7)
//   eye blink     `PartXHeadNN.dds`               -> `PartXHeadNN_Flash.dds`
//   skin set      `Part_{male,female}{Upper,Lower}[NN].ext` -> `...%02d.ext` with `skin_set + 1`
//
// A variant that does not exist (1 of 547 `-etNN` files, some skin sets) falls back to the base texture:
// `textureCandidates` returns the names best first, ending with the base name.

/** Directory of the model textures in the VFS. */
export const MODEL_TEXTURE_DIR = 'model/texture';

const pad2 = (n: number): string => String(Math.trunc(n)).padStart(2, '0');

/** `X.ext` -> `X-etNN.ext` (inserted before the last `.`; a name without a dot gets the suffix appended). */
export function withTextureSet(name: string, set: number): string {
  if (!(set > 0)) return name;
  const dot = name.lastIndexOf('.');
  const suffix = `-et${pad2(set)}`;
  return dot < 0 ? name + suffix : name.slice(0, dot) + suffix + name.slice(dot);
}

/** `name[..len-4] + "_Flash" + name[len-4..]` (WndWorld.cpp:11642-11671; assumes a 4 character extension). */
export function withEyeFlash(name: string): string {
  const len = name.length;
  if (len < 4) return name;
  return `${name.slice(0, len - 4)}_Flash${name.slice(len - 4)}`;
}

const SKIN_RE = /^(part_(?:male|female)(?:upper|lower))(\d{2})?(\.[a-z0-9]+)$/i;

/**
 * The body part texture of skin set `skinSet` (0 or 1): `...%02d.ext` with `skinSet + 1`. Returns `null` for a
 * texture that is not a body part texture. (The original macro `PARTSTEX_UPPER/LOWER` is not part of the source
 * tree we have; the form follows ref/18 section 8.5.)
 */
export function withSkinSet(name: string, skinSet: number): string | null {
  const m = SKIN_RE.exec(name);
  if (!m) return null;
  return `${m[1] ?? ''}${pad2(skinSet + 1)}${m[3] ?? ''}`;
}

const SKIN_LITERAL_RE = /^(part_(?:male|female)(?:upper|lower))01(\.[a-z0-9]+)$/i;

/**
 * The skin set rule exactly as `CMover::DoEquip`/`UpdateParts` apply it (MoverEquip.cpp:809-930):
 * `ChangeTexture(part, "Part_{sex}{Upper|Lower}01.dds", "Part_{sex}{Upper|Lower}%02d.dds" % (skinSet + 1))`
 * replaces a material whose texture is LITERALLY `...01.dds`. The shipped body models reference the un-numbered
 * `Part_maleUpper.dds`, and no `...02.dds` exists, so with this data the skin set changes nothing (KEEP: the rule
 * is reproduced, it just never fires). Returns `null` when the name is not the `...01` texture.
 */
export function withSkinSetLiteral(name: string, skinSet: number): string | null {
  const m = SKIN_LITERAL_RE.exec(name);
  if (!m) return null;
  return `${m[1] ?? ''}${pad2(skinSet + 1)}${m[2] ?? ''}`;
}

/** Rules of one model instance. */
export interface TextureNameRules {
  /** `ModelElem.texture_ex`, 0 = none. */
  readonly textureSet?: number;
  /** Player body part skin set 0 or 1; `undefined` = none. */
  readonly skinSet?: number;
  /** Apply the skin set like the C++ does (only the literal `...01` texture, `withSkinSetLiteral`). */
  readonly skinSetLiteral?: boolean;
  /** The closed-eyes head texture (`_Flash`). */
  readonly eyeFlash?: boolean;
  // ---- housing / guild house (ui-house-quiz)
  /** The texture IS this file name (`red.tga` of the placement ghost: the colliding furniture shows it on block 0 of every object); the other rules are not applied to it. */
  readonly literal?: string;
}

/** The name after the rules (the base name when none applies). */
export function applyTextureRules(name: string, rules: TextureNameRules): string {
  // ---- housing / guild house (ui-house-quiz): a literal replaces the whole name
  if (rules.literal !== undefined && rules.literal !== '') return rules.literal;
  let out = name;
  if (rules.skinSet !== undefined) {
    out =
      (rules.skinSetLiteral === true
        ? withSkinSetLiteral(out, rules.skinSet)
        : withSkinSet(out, rules.skinSet)) ?? out;
  }
  if (rules.eyeFlash === true) out = withEyeFlash(out);
  if (rules.textureSet !== undefined && rules.textureSet > 0)
    out = withTextureSet(out, rules.textureSet);
  return out;
}

/** Candidate texture names, best first, always ending with the base name (unique). */
export function textureCandidates(name: string, rules: TextureNameRules): string[] {
  const ruled = applyTextureRules(name, rules);
  return ruled === name ? [name] : [ruled, name];
}

/**
 * The canonical VFS path of a model texture: lower-case, `/` separators (`CTexture::LoadTexture` lowercases
 * names, ref/21 pitfalls). `name` may already contain a directory.
 */
export function modelTexturePath(name: string, dir: string = MODEL_TEXTURE_DIR): string {
  const n = name.replace(/\\/g, '/').toLowerCase();
  return n.includes('/') ? n : `${dir}/${n}`;
}
