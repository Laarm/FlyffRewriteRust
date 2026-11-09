// `AudioData` over the client's `ClientDataIndex` (Plan/44 §1.4): the item, mover, effect, NPC and world lookups the triggers read,
// plus `motionSounds`, the `MA_SOUND` ids of a model's clips from the `motions` bundle (the prefetch of §6.5). The index
// already satisfies the structural lookups; this adds the one derived table and memoises it per bundle.

import type { ClientDataIndex } from '../data/clientData';
import { decodeRows } from '../data/rowTable';
import type { MotionClipRow, MotionModelRow, MotionsBundle } from '../gen/bundles';
import type { AudioData } from './deps';

/** The `MA_SOUND` bit of a motion attribute's flags (`Bone.h`). */
export const MA_SOUND = 2;

/** `ot -> index -> sound ids` for the models of a `motions` bundle (deduplicated, ascending). */
export function motionSoundIndex(motions: MotionsBundle): Map<string, readonly number[]> {
  const clips: MotionClipRow[] = decodeRows(motions.clips);
  const out = new Map<string, readonly number[]>();
  for (const model of decodeRows<MotionModelRow>(motions.models)) {
    const ids = new Set<number>();
    for (const [, clip] of model.slots) {
      for (const attr of clips[clip]?.attrs ?? []) {
        const [, , flags, sound] = attr;
        if ((flags & MA_SOUND) !== 0 && sound > 0) ids.add(sound);
      }
    }
    // the empty-slot clip and the model's own built-in animation
    for (const clip of [model.empty, model.builtin]) {
      for (const attr of clip >= 0 ? (clips[clip]?.attrs ?? []) : []) {
        const [, , flags, sound] = attr;
        if ((flags & MA_SOUND) !== 0 && sound > 0) ids.add(sound);
      }
    }
    out.set(
      `${String(model.ot)}:${String(model.index)}`,
      [...ids].sort((a, b) => a - b),
    );
  }
  return out;
}

/** The audio view of a data index (`null` index = no data yet). */
export function audioDataOf(index: ClientDataIndex | null): AudioData | null {
  if (index === null) return null;
  let motionSource: unknown = null;
  let motionIds: Map<string, readonly number[]> | null = null;
  return {
    item: (id) => index.item(id),
    skill: (id) => index.skill(id),
    mover: (id) => index.mover(id),
    sfx: (id) => index.sfx(id),
    npc: (key) => index.npc(key),
    worldFolder: (folder) => index.worldFolder(folder),
    motionSounds: (ot, modelIndex) => {
      const motions = index.bundle('motions');
      if (motions === undefined) return [];
      if (motions !== motionSource || motionIds === null) {
        motionSource = motions;
        motionIds = motionSoundIndex(motions);
      }
      return motionIds.get(`${String(ot)}:${String(modelIndex)}`) ?? [];
    },
  };
}
