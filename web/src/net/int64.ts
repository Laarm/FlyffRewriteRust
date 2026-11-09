// int64 handling on the wire (Plan/11 section 4.3). protobuf-es maps every 64-bit field to `bigint`.
// Fields marked `(flyff.js_safe_number)` stay within +-(2^53 - 1) and convert with `n64()` where they
// leave the protocol layer (codec, stores, handlers); outgoing values are built with `b64()`. The three
// bit-packed 64-bit values (`ItemElem.random_opt_item_id`, `ItemSelectAwakeningValueNtf.new_random_option`,
// `CallTheRoll.attendance_bits`) stay `bigint` end to end. The helpers are generated (`gen/proto/js_safe.ts`);
// this module re-exports them and adds the name lookup.

import { JS_SAFE_FIELDS, b64, n64 } from '../gen/proto/js_safe';

export { JS_SAFE_FIELDS, b64, n64 };

const SAFE = new Set<string>(JS_SAFE_FIELDS);

/** `true` when the field `flyff.v1.<Message>.<field_name>` is js-safe (converted with `n64`). */
export function isJsSafeField(fullName: string): boolean {
  return SAFE.has(fullName);
}

/** `n64` for a field that may be unset on the wire (`bigint | undefined` -> `number`, 0 when unset). */
export function n64or0(v: bigint | undefined): number {
  return v === undefined ? 0 : n64(v);
}
