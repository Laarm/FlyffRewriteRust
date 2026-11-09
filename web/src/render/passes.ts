// The pass framework (Plan/41 section 2.4): an ordered list of passes drawn once per frame into the off-screen
// target between the clear (pass 0) and the ramp (pass 16). Later work packages register their passes (sky,
// terrain, water, SFX, ...); the base renderer registers the three model queues (opaque, blended, translucent),
// which draw the shared scene through the `Layers` of `layers.ts`.

import type { Scene } from 'three';
import type { RenderCamera } from './camera';
import type { RenderDevice } from './device';
import type { EnvLight } from './gl/materials/env';
import type { RenderOptions } from './quality';
import type { PassName } from './stats';
import type { FrameInput } from './types';
import { LAYER } from './layers';

/** Numbers of the passes in section 2.4 (the order of the table). */
export const PASS_ORDER: Readonly<Record<PassName, number>> = {
  clear: 0,
  sky: 1,
  terrain: 2,
  opaque: 3,
  blended: 4,
  highlight: 5,
  partsFx: 6,
  particles: 7,
  blobShadows: 8,
  water: 9,
  ribbons: 10,
  underwater: 11,
  sfx: 12,
  translucent: 13,
  weather: 14,
  flare: 15,
  ramp: 16,
  overlay: 17,
};

/** What a pass sees. */
export interface PassContext {
  readonly device: RenderDevice;
  readonly camera: RenderCamera;
  /** The shared model scene (all queues; select a queue through `layers`). */
  readonly scene: Scene;
  readonly frame: FrameInput;
  readonly env: EnvLight;
  readonly options: RenderOptions;
}

export interface RenderPass {
  readonly name: PassName;
  enabled: boolean;
  render(ctx: PassContext): void;
}

/** A pass that renders the shared scene through one layer (`LAYER` key). */
export function scenePass(
  name: PassName,
  queue: keyof typeof LAYER,
  sortObjects = true,
): RenderPass {
  return {
    name,
    enabled: true,
    render(ctx) {
      ctx.device.renderLayer(ctx.scene, ctx.camera.camera, LAYER[queue], sortObjects);
    },
  };
}

/** The ordered pass list. */
export class PassPipeline {
  private list: RenderPass[] = [];

  /** Registers (or replaces) the pass of that name; the list stays ordered by `PASS_ORDER`. */
  register(pass: RenderPass): void {
    this.list = this.list.filter((p) => p.name !== pass.name);
    this.list.push(pass);
    this.list.sort((a, b) => PASS_ORDER[a.name] - PASS_ORDER[b.name]);
  }

  unregister(name: PassName): void {
    this.list = this.list.filter((p) => p.name !== name);
  }

  get(name: PassName): RenderPass | undefined {
    return this.list.find((p) => p.name === name);
  }

  names(): PassName[] {
    return this.list.map((p) => p.name);
  }

  /**
   * Runs the enabled passes in order and returns the draw calls per pass (differences of the device's counters)
   * and the triangles of the frame.
   */
  run(ctx: PassContext): { calls: Partial<Record<PassName, number>>; triangles: number } {
    const calls: Partial<Record<PassName, number>> = {};
    for (const p of this.list) {
      if (!p.enabled) continue;
      const before = ctx.device.callsSoFar;
      p.render(ctx);
      const n = ctx.device.callsSoFar - before;
      if (n > 0) calls[p.name] = (calls[p.name] ?? 0) + n;
    }
    return { calls, triangles: ctx.device.trianglesSoFar };
  }
}
