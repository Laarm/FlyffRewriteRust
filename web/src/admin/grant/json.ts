// 64-bit numbers on the wire. The random-option word of an item is a `u64` (bits 61 to 63 hold the safe flag and the level-down marks),
// which a JavaScript number cannot hold. `StoredItemDto` therefore carries it as a decimal string; the one route that takes the stored
// item itself (`drop-item`, `flyff_db::StoredItem`) wants a JSON number, so the digits are written with `JSON.rawJSON` where the browser
// has it (Chrome 114, Safari 18, Node 22) and refused for the other browsers when they would lose precision.
import type { StoredItemDto } from '../gen/dto';

/** The `JSON.rawJSON` of this browser, or `null` (it is not in every `lib.d.ts`, hence the lookup by name). */
function rawJsonFn(): ((text: string) => unknown) | null {
  const fn: unknown = Reflect.get(JSON, 'rawJSON');
  if (typeof fn !== 'function') return null;
  return (text) => {
    const made: unknown = Reflect.apply(fn, JSON, [text]);
    return made;
  };
}

/** Can `digits` (a decimal `u64`) be sent exactly? */
export function canSendU64(digits: string): boolean {
  if (!/^\d{1,20}$/.test(digits)) return false;
  return Number.isSafeInteger(Number(digits)) || rawJsonFn() !== null;
}

/** `digits` as a value `JSON.stringify` writes as the exact number (`JSON.rawJSON`), or as a plain number when that is exact. */
export function u64Json(digits: string): unknown {
  const n = Number(digits);
  if (Number.isSafeInteger(n)) return n;
  const raw = rawJsonFn();
  if (raw === null) throw new Error('this browser cannot send a 64-bit number exactly');
  return raw(digits);
}

/** The `item` of a `drop-item` body: the validated item with its random-option word as a JSON number. */
export function dropItemJson(item: StoredItemDto): Record<string, unknown> {
  return { ...item, random_option: u64Json(item.random_option) };
}

/** The word of an item as a bigint (`0n` for a malformed string). */
export function wordOf(digits: string): bigint {
  return /^\d{1,20}$/.test(digits) ? BigInt(digits) : 0n;
}
