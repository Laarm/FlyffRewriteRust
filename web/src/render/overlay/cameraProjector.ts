// The overlay's projector over the renderer's camera rig (`RenderCamera`): the matrices of the frame the world was drawn
// with, so names and balloons sit exactly on the heads of the movers.

import type { RenderCamera } from '../camera';
import { createProjector, type Projector } from './anchor';

/** A projector for the CSS-pixel canvas of `width x height` (call it after `RenderCamera.update` of the frame). */
export function projectorOfCamera(rig: RenderCamera, width: number, height: number): Projector {
  return createProjector(
    rig.viewProjection.elements,
    rig.camera.matrixWorldInverse.elements,
    width,
    height,
  );
}
