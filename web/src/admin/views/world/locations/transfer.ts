// Export and import of the shared bookmarks as JSON (Plan/25 section 7.4): the file is a list of bookmark bodies (the same shape the bookmark routes take),
// so an export can be edited by hand and imported on another server. Pure parts here; the editor tab does the downloading and the calls.
import { bodyOf, type BookmarkBody, type LocationRow } from './api';

/** The JSON text of the bookmarks among `rows` (defaults and world cameras are data, not something to export). */
export function exportJson(rows: readonly LocationRow[]): string {
  const list = rows.filter((r) => r.kind === 'bookmark').map((r) => bodyOf(r));
  return `${JSON.stringify(list, null, 2)}\n`;
}

function num(v: unknown): number | null {
  return typeof v === 'number' && Number.isFinite(v) ? v : null;
}

/** One body from untrusted JSON, or an error text. The server checks everything again (worlds, ranges). */
export function parseBody(v: unknown, index: number): BookmarkBody | string {
  if (typeof v !== 'object' || v === null) return `#${String(index + 1)}: not an object`;
  const o = v as Record<string, unknown>;
  const name = typeof o['name'] === 'string' ? o['name'].trim() : '';
  const world = num(o['world']);
  const x = num(o['x']);
  const z = num(o['z']);
  if (name === '') return `#${String(index + 1)}: a name is needed`;
  if (world === null || !Number.isInteger(world) || world <= 0) return `${name}: a world is needed`;
  if (x === null || z === null) return `${name}: x and z are needed`;
  const group = typeof o['group'] === 'string' ? o['group'].trim() : '';
  const zoom = num(o['zoom']);
  const sort = num(o['sort']);
  const policy = o['layer_policy'];
  return {
    group,
    name,
    world,
    x,
    z,
    sort: sort !== null && Number.isInteger(sort) ? sort : 0,
    ...(policy === 'fixed0' || policy === 'pick_live' ? { layer_policy: policy } : {}),
    ...(zoom === null ? {} : { zoom }),
  };
}

export interface ParsedImport {
  readonly bodies: readonly BookmarkBody[];
  readonly errors: readonly string[];
}

/** Reads the text of an import: a list of bodies, or an object with a `bookmarks` list. */
export function parseImport(text: string): ParsedImport | null {
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch {
    return null;
  }
  const list = Array.isArray(v)
    ? v
    : typeof v === 'object' && v !== null && Array.isArray((v as { bookmarks?: unknown }).bookmarks)
      ? (v as { bookmarks: unknown[] }).bookmarks
      : null;
  if (list === null) return null;
  const bodies: BookmarkBody[] = [];
  const errors: string[] = [];
  list.slice(0, 500).forEach((item, i) => {
    const b = parseBody(item, i);
    if (typeof b === 'string') errors.push(b);
    else bodies.push(b);
  });
  return { bodies, errors };
}

/** Does a stored bookmark have the group and name of the body (the import updates it)? */
export function sameSlot(row: LocationRow, body: BookmarkBody): boolean {
  const group = body.group.trim() === '' ? 'Bookmarks' : body.group.trim();
  return (
    row.kind === 'bookmark' &&
    row.group.toLowerCase() === group.toLowerCase() &&
    row.name.toLowerCase() === body.name.trim().toLowerCase()
  );
}
