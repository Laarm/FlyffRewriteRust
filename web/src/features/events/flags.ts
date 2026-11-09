// The pure part of the event flags (Plan/45 §28.0; WP-CFEAT-07): `FlyffEvent` replaces the whole array (padded or cut to 1024 bytes), `SetLocalEvent` sets one byte (an id outside 0..1023
// is ignored: the original wrote `g_eLocal.SetState( id, state )` with a 16-bit id and an array of `MAX_EVENT`).

import { EVENT_FLAGS } from './store';

/** `FlyffEvent{flags}`: the new array. */
export function replaceFlags(bytes: Uint8Array): Uint8Array {
  const out = new Uint8Array(EVENT_FLAGS);
  out.set(bytes.length > EVENT_FLAGS ? bytes.subarray(0, EVENT_FLAGS) : bytes);
  return out;
}

/** `SetLocalEvent{event_id, state}`: the array with one byte changed; the same array when the id is out of range or the byte is already that value. */
export function setFlag(flags: Uint8Array, id: number, state: number): Uint8Array {
  if (!Number.isInteger(id) || id < 0 || id >= EVENT_FLAGS) return flags;
  const value = state & 0xff;
  if (flags[id] === value) return flags;
  const out = flags.slice();
  out[id] = value;
  return out;
}

/** `g_eLocal.GetState( id ) != 0`. */
export function flagOn(flags: Uint8Array, id: number): boolean {
  return Number.isInteger(id) && id >= 0 && id < EVENT_FLAGS && flags[id] !== 0;
}
