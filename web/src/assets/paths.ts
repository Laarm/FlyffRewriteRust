// Resource paths and asset URLs (Plan/24 section 5.1 and 6.5, Plan/40 section 9.1, WP-CLIENT-09).
//
// `normalizeResourcePath` is the TypeScript port of `flyff_common::normalize_resource_path` (Plan/14 section 2.2); both
// sides run the shared golden file `crates/flyff-common/tests/golden/normalize_paths.json`. `assetUrl` is the
// transcription of the 24 section 6.5 resolver (it must pass `crates/flyff-assets/tests/golden/asset-urls.json`);
// `genUrl` builds the `/gen` URLs of the keys that are not VFS files.

import type { ClientManifest } from '../gen/data/assetManifest';

/** `true` for the ASCII whitespace of Rust's `u8::is_ascii_whitespace` (space, \t, \n, \f, \r; NOT \v). */
function isAsciiWhitespace(code: number): boolean {
  return code === 0x20 || code === 0x09 || code === 0x0a || code === 0x0c || code === 0x0d;
}

/** `to_ascii_lowercase`: only `A-Z` change (non-ASCII text stays as it is, unlike `toLowerCase`). */
function asciiLower(s: string): string {
  return s.replace(/[A-Z]/g, (c) => String.fromCharCode(c.charCodeAt(0) + 32));
}

/**
 * Normalises a resource path (the rules of Plan/14 section 2.2): trim ASCII whitespace, `\` to `/`, drop empty and
 * `.` segments, `..` pops the previous segment, ASCII lower case, join with `/`. Returns `null` for the empty
 * path and for a `..` that would leave the Resource root (the two `PathError`s of the Rust function).
 */
export function normalizeResourcePath(p: string): string | null {
  let start = 0;
  let end = p.length;
  while (start < end && isAsciiWhitespace(p.charCodeAt(start))) start += 1;
  while (end > start && isAsciiWhitespace(p.charCodeAt(end - 1))) end -= 1;
  const segments: string[] = [];
  for (const seg of p.slice(start, end).replaceAll('\\', '/').split('/')) {
    if (seg === '' || seg === '.') continue;
    if (seg === '..') {
      if (segments.pop() === undefined) return null;
      continue;
    }
    segments.push(asciiLower(seg));
  }
  return segments.length === 0 ? null : segments.join('/');
}

/** `encodeURIComponent` of every segment of a normalised path (`Obj_ Azria.o3d` -> `obj_%20azria.o3d`). */
export function encodePath(path: string): string {
  return path.split('/').map(encodeURIComponent).join('/');
}

/** The directory part of a normalised path (`''` for a root file). */
export function dirOf(path: string): string {
  const slash = path.lastIndexOf('/');
  return slash < 0 ? '' : path.slice(0, slash);
}

/** The file name of a normalised path. */
export function fileOf(path: string): string {
  const slash = path.lastIndexOf('/');
  return slash < 0 ? path : path.slice(slash + 1);
}

/** The lower-case extension without the dot (`''` when there is none). */
export function extOf(file: string): string {
  const dot = file.lastIndexOf('.');
  return dot < 0 ? '' : file.slice(dot + 1);
}

/** The `gen` entry of a kind that has one fingerprint per partition (`png`, `dds`, `collision`, ...). */
export function genPartitions(
  m: ClientManifest,
  kind: string,
): Readonly<Record<string, string>> | undefined {
  const entry = m.gen[kind];
  return typeof entry === 'object' ? entry : undefined;
}

/** The fingerprint of a kind that has a single one (`cursor`, `motion-table`, `model-bounds`, `ctrl-models`). */
export function genFingerprint(m: ClientManifest, kind: string): string | undefined {
  const entry = m.gen[kind];
  return typeof entry === 'string' ? entry : undefined;
}

/** A manifest with its `dds_fix` lists turned into sets (done once; the URL resolver runs on every request). */
export interface ResolvedManifest {
  readonly manifest: ClientManifest;
  readonly ddsFix: ReadonlyMap<string, ReadonlySet<string>>;
}

export function resolveManifest(manifest: ClientManifest): ResolvedManifest {
  const ddsFix = new Map<string, ReadonlySet<string>>();
  for (const [dir, files] of Object.entries(manifest.dds_fix)) ddsFix.set(dir, new Set(files));
  return { manifest, ddsFix };
}

/**
 * The URL the client fetches `path` (a normalised VFS path) from, or `null` when the path is not client-visible
 * (Plan/24 section 6.5): raw files `/res/<groupfp>/<path>`, PNG-converted forms `/gen/png/<fp>/<path>.png`, fixed DDS
 * `/gen/dds/<fp>/<path>`. A `png`/`dds` rule whose partition is missing from `gen` falls back to the raw URL (the
 * server allows raw access to every whitelisted file).
 */
export function assetUrl(m: ResolvedManifest, path: string): string | null {
  const { manifest } = m;
  const dir = dirOf(path);
  const file = fileOf(path);
  const ext = extOf(file);
  const group = manifest.res.groups[dir];
  if (group === undefined) return null;
  const rule = manifest.rules.find((r) => r.dir === dir && r.exts.includes(ext));
  const enc = encodePath(path);
  if (rule?.form === 'png') {
    const fp = genPartitions(manifest, 'png')?.[dir];
    if (fp !== undefined) return `${manifest.gen.prefix}/png/${fp}/${enc}.png`;
  } else if (rule?.form === 'dds' && m.ddsFix.get(dir)?.has(file) === true) {
    const fp = genPartitions(manifest, 'dds')?.[dir];
    if (fp !== undefined) return `${manifest.gen.prefix}/dds/${fp}/${enc}`;
  }
  return `${manifest.res.prefix}/${group}/${enc}`;
}

/** The nibble-keyed skin image of a PNG-converted file: `/gen/png/<fp>/<path>.nk.png` (42's skin bake). */
export function nibbleUrl(m: ResolvedManifest, path: string): string | null {
  const { manifest } = m;
  const dir = dirOf(path);
  if (manifest.res.groups[dir] === undefined) return null;
  const fp = genPartitions(manifest, 'png')?.[dir];
  return fp === undefined ? null : `${manifest.gen.prefix}/png/${fp}/${encodePath(path)}.nk.png`;
}

/** `/gen/collision/<fp>/<folder>/<x>-<z>.bin` (x and z decimal without padding); `null` for an unknown world folder. */
export function collisionUrl(
  m: ClientManifest,
  folder: string,
  x: number,
  z: number,
): string | null {
  const fp = genPartitions(m, 'collision')?.[folder];
  if (fp === undefined || !Number.isInteger(x) || !Number.isInteger(z) || x < 0 || z < 0)
    return null;
  return `${m.gen.prefix}/collision/${fp}/${encodeURIComponent(folder)}/${String(x)}-${String(z)}.bin`;
}

/** `/gen/<kind>/<fp>/all.bin` of the session-wide sim inputs (`motion-table`, `model-bounds`, `ctrl-models`). */
export function allBinUrl(
  m: ClientManifest,
  kind: 'motion-table' | 'model-bounds' | 'ctrl-models',
): string | null {
  const fp = genFingerprint(m, kind);
  return fp === undefined ? null : `${m.gen.prefix}/${kind}/${fp}/all.bin`;
}

/** `/gen/cursor/<fp>/<stem>.png` (the hotspot comes from `manifest.cursors[stem]`). */
export function cursorUrl(m: ClientManifest, stem: string): string | null {
  const fp = genFingerprint(m, 'cursor');
  const lower = stem.toLowerCase();
  if (fp === undefined || m.cursors[lower] === undefined) return null;
  return `${m.gen.prefix}/cursor/${fp}/${encodeURIComponent(lower)}.png`;
}

/** `[x, y]` hotspot of a parsed cursor, `null` when unknown. */
export function cursorHotspot(m: ClientManifest, stem: string): [number, number] | null {
  return m.cursors[stem.toLowerCase()]?.hotspot ?? null;
}
