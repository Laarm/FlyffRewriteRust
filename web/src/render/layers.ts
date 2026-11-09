// three.js layer ids of the pass groups (Plan/41 section 2.4): every drawable mesh sits in ONE scene and carries the
// layer of the pass that draws it; a pass renders the scene with `camera.layers.set(layer)`. Opaque and alpha-tested
// geometry (pass 3), blended material blocks of opaque models (pass 4), translucent models and night light objects
// (pass 13).

export const LAYER = {
  /** Pass 3: opaque and alpha-tested blocks. */
  opaque: 1,
  /** Pass 4: blocks with `amount < 255` of models that are not faded. */
  blended: 2,
  /** Pass 13: models with a blend factor < 255 and night light objects (additive sub-queue). */
  translucent: 3,
  /** Pass 5: the additive re-draw of the mouse-over mover (Plan/41 12.9). */
  highlight: 4,
  /** Pass 8: the blob shadows under the movers (Plan/41 13.1). */
  shadow: 5,
  /** No pass draws this layer: where a mesh goes while it must not be drawn (light objects by day). */
  hidden: 30,
} as const;

/** The pass queues (a `LAYER` entry that a pass draws). */
export type Queue = 'opaque' | 'blended' | 'translucent';
