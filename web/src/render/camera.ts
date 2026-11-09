// Projection and camera consumption (Plan/41 section 5, ref/21 section 6.4, WP-REND-01).
//
//   fovY = pi / fDiv  (4 normal = 45 degrees; 10/25/50 telescope; 2..4 flying), aspect = canvas CSS width / height,
//   near 0.49 (`m_fNearPlane 0.5 - 0.01`), far 512 (`m_fFarPlane`)
//   view: D3DXMatrixLookAtLH(pos, lookAt, up) built in LH (`d3dx.ts`), mapped once with `mirrorMat` -> the three.js
//   `matrixWorldInverse`; `matrixWorld` is its inverse. three.js never touches the camera matrices
//   (`matrixAutoUpdate = matrixWorldAutoUpdate = false`).
//
// Visible results are identical to D3D although the depth convention differs (z in [0, 1] there, [-1, 1] here):
// the mapped view space looks down -Z like every three.js camera and x/y are unchanged (S*V*S).

import { Frustum, Matrix4, PerspectiveCamera, Vector3 } from 'three';
import { inverse, lookAtLH } from './d3dx';
import { mirrorMatrix } from './space';
import type { CameraState, Ray3 } from './types';

/** `m_fNearPlane` (0.5) minus 0.01. */
export const NEAR_PLANE = 0.49;
/** `m_fFarPlane`. */
export const FAR_PLANE = 512;

export class RenderCamera {
  readonly camera = new PerspectiveCamera(45, 1, NEAR_PLANE, FAR_PLANE);
  readonly frustum = new Frustum();
  /** `projection * view` (three.js space), the frustum source. */
  readonly viewProjection = new Matrix4();
  /** The eye in three.js space. */
  readonly position = new Vector3();
  /** `g_matInvView` with the translation zeroed (billboards), LH row-major and three.js column-major. */
  invViewRotLH: number[] = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  readonly invViewRotThree = new Matrix4();
  /** The last applied state. */
  state: CameraState | null = null;
  private lastFov = Number.NaN;
  private lastAspect = Number.NaN;

  constructor() {
    this.camera.matrixAutoUpdate = false;
    this.camera.matrixWorldAutoUpdate = false;
    this.camera.layers.enableAll();
  }

  /** Applies a camera state for the current aspect ratio (CSS width / height of the canvas). */
  update(state: CameraState, aspect: number): void {
    this.state = state;
    const cam = this.camera;
    const fovDeg = 180 / state.fovDiv;
    if (fovDeg !== this.lastFov || aspect !== this.lastAspect) {
      cam.fov = fovDeg;
      cam.aspect = aspect;
      cam.near = NEAR_PLANE;
      cam.far = FAR_PLANE;
      cam.updateProjectionMatrix();
      this.lastFov = fovDeg;
      this.lastAspect = aspect;
    }
    const viewLH = lookAtLH(state.pos, state.lookAt, state.up);
    cam.matrixWorldInverse.fromArray(mirrorMatrix(viewLH));
    cam.matrixWorld.copy(cam.matrixWorldInverse).invert();
    this.position.setFromMatrixPosition(cam.matrixWorld);
    cam.position.copy(this.position);

    this.viewProjection.multiplyMatrices(cam.projectionMatrix, cam.matrixWorldInverse);
    this.frustum.setFromProjectionMatrix(this.viewProjection);

    const rot = inverse(viewLH);
    rot[12] = 0;
    rot[13] = 0;
    rot[14] = 0;
    this.invViewRotLH = rot;
    this.invViewRotThree.fromArray(mirrorMatrix(rot));
  }

  /** The pick ray through normalised device coordinates (x right, y up, both -1..1), three.js space. */
  pickRay(ndcX: number, ndcY: number): Ray3 {
    const cam = this.camera;
    const near = new Vector3(ndcX, ndcY, -1).unproject(cam);
    const far = new Vector3(ndcX, ndcY, 1).unproject(cam);
    const dir = far.sub(near).normalize();
    return {
      origin: [this.position.x, this.position.y, this.position.z],
      dir: [dir.x, dir.y, dir.z],
    };
  }

  /** CSS pixel position (origin top-left) to NDC. */
  static ndcOf(px: number, py: number, cssWidth: number, cssHeight: number): [number, number] {
    return [(px / cssWidth) * 2 - 1, 1 - (py / cssHeight) * 2];
  }
}
