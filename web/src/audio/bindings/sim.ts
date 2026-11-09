// The sim-event consumer (Plan/44 §8, WP-AUDIO-06): animation-frame sounds, footsteps and the hit-frame sounds, from the per-frame
// `SimEventRouter` of doc 40 (loop step 5). The motion clock is the shared Rust sim's (A12): TypeScript never re-implements it, so
// every sound arrives as an event with its position already computed.
//
//   AttrSound  [sound, x, y, z, flags]   T01-T04: `MA_SOUND` attributes of mover clips, animated statics, rides and ctrls. Flags bit 0
//                                        `positional`, bit 1 `decor` (an object id of the decor set: no mover lookup, §8.3).
//   Footstep   [0, x, y, z]              T05: `SND_WALK_LANDSOFT` at a walking player's `MA_HIT` frames, unless the mover is transparent
//   AttrHit    [kind, state, target, i]  T06/T10: the generic melee hit sound (kind 0 Melee, 6 FlyMelee); T11: the boss melee sounds of a non-player swing
//                                        (kind 0 only; `i` is the swing's `m_nMotionHitCount` before the handler's increment: the client sim counts it for the bosses)
//   CmdArrived [cmd, target]             T40: the local player's magic / range attack or skill use starts on arrival (combat music)
//
// The sim emits none of these in `SimRole::Server`, while the motion clock is paused (hit-stop, KQ-04) or during a `Silent`
// fast-forward (31 §3.6), so the consumer needs no rule of its own for them. Records are reused by the router: every field is copied
// synchronously.

import { bitsToFloat, type SimEventRecord } from '../../wasm/simTypes';
import { SND_WALK_LANDSOFT } from '../../gen/defs/sound';
import { type EventName, SOUND_FLAGS } from '../../gen/sim/wire';
import type { AudioStateReaders } from '../deps';
import type { AudioCues } from '../cues';
import type { SfxPlayer } from '../sfx';

/** `TRANSPARENT_MODE`: a transparent mover makes no footstep (`Mover.cpp:3289-3306`). */
export const TRANSPARENT_MODE = 0x0000_0002;
/** `HitKind` codes of the `AttrHit` payload word (`flyff-sim client/events.rs hit_kind_code`). */
export const HIT_KIND_MELEE = 0;
export const HIT_KIND_FLY_MELEE = 6;
/** `OBJACT_*` of the commands whose arrival starts an attack (`flyff-sim types.rs`). */
export const OBJACT_MAGIC_ATTACK = 2;
export const OBJACT_RANGE_ATTACK = 3;
export const OBJACT_USESKILL = 5;

/** The router surface the consumer needs (`SimEventRouter.on`). */
export interface SimEventSource {
  on(kinds: readonly EventName[], fn: (e: SimEventRecord) => void): () => void;
}

export interface SimBindingDeps {
  readonly sfx: Pick<SfxPlayer, 'playId'>;
  readonly cues: Pick<AudioCues, 'meleeHit' | 'bossMeleeSpecial' | 'localAttackStarted'>;
  readonly state: Pick<AudioStateReaders, 'mover' | 'self'>;
}

/** Subscribes the audio consumers to the router; returns the unsubscribe function. */
export function bindSimEvents(router: SimEventSource, deps: SimBindingDeps): () => void {
  const { sfx, cues, state } = deps;

  const onSound = (e: SimEventRecord): void => {
    const w = e.words;
    if (e.name === 'AttrSound') {
      const sound = w[0] | 0; // an i32 sound id
      if ((w[4] & SOUND_FLAGS.POSITIONAL) !== 0) {
        sfx.playId(sound, [bitsToFloat(w[1]), bitsToFloat(w[2]), bitsToFloat(w[3])]);
      } else {
        // ride / attachment / ctrl sounds are non-positional on purpose (KEEP-QUIRK, KQ-44)
        sfx.playId(sound);
      }
    } else {
      // Footstep
      const mover = state.mover(e.objId);
      if (((mover?.visual?.mode ?? 0) & TRANSPARENT_MODE) !== 0) return;
      sfx.playId(SND_WALK_LANDSOFT, [bitsToFloat(w[1]), bitsToFloat(w[2]), bitsToFloat(w[3])]);
    }
  };

  const onHit = (e: SimEventRecord): void => {
    const kind = e.words[0];
    if (kind === HIT_KIND_MELEE) {
      cues.meleeHit(e.objId, e.words[2], 'ground');
      // T11: words = [kind, state, target, hitCount]
      cues.bossMeleeSpecial(e.objId, e.words[2], e.words[1], e.words[3]);
    } else if (kind === HIT_KIND_FLY_MELEE) cues.meleeHit(e.objId, e.words[2], 'fly');
  };

  const onArrived = (e: SimEventRecord): void => {
    const cmd = e.words[0];
    if (cmd !== OBJACT_MAGIC_ATTACK && cmd !== OBJACT_RANGE_ATTACK && cmd !== OBJACT_USESKILL) {
      return;
    }
    // only the local player's own commands start the combat music
    if (e.objId === state.self().objId) cues.localAttackStarted();
  };

  // a kind absent from this build's `EVENT_KINDS` is logged once by the router and ignored
  const offs = [
    router.on(['AttrSound', 'Footstep'], onSound),
    router.on(['AttrHit'], onHit),
    router.on(['CmdArrived'], onArrived),
  ];
  return () => {
    for (const off of offs) off();
  };
}
