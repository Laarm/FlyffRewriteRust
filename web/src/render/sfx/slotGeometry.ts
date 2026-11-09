// Slot geometries of the SFX passes (`SfxRenderer` quad slots, `ParticleRenderer` slots): every slot draws the SAME unit quad
// (`position`, `uv` and the index are shared objects) with an interleaved instance buffer of its own. When the list outgrows that
// buffer the slots are released and rebound.
//
// A plain `geometry.dispose()` frees the GL buffers of EVERY attribute still attached, the shared quad included (three.js
// `WebGLGeometries.onGeometryDispose` removes each attribute from `WebGLAttributes`): it only worked because all the slots of a
// pass go at once. The shared attributes are detached first, so the dispose frees the instance buffer, the VAOs and the cached
// instance limit but not the quad (BFIX1 render-movers step 6b, the rule of `gl/instancing.ts`).

import type { BufferAttribute, InstancedBufferGeometry } from 'three';

/**
 * Releases the GL state of a quad slot geometry whose instance buffer is replaced and puts the shared unit quad back: the
 * geometry stays the slot's, and the caller rebinds the instance attributes (`iA*`, `iPos`, `iColor`) before the next draw.
 */
export function releaseSlotGeometry(
  g: InstancedBufferGeometry,
  corner: BufferAttribute,
  cornerUv: BufferAttribute,
  cornerIndex: BufferAttribute,
): void {
  g.deleteAttribute('position');
  g.deleteAttribute('uv');
  g.setIndex(null);
  g.dispose();
  g.setAttribute('position', corner);
  g.setAttribute('uv', cornerUv);
  g.setIndex(cornerIndex);
}
