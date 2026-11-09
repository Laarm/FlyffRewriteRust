// The mouse-over highlight of a mover (Plan/41 section 12.9, pass 5; MoverRender.cpp:679-700): the mover under the cursor is
// drawn a SECOND time, additively (`ONE, ONE`), depth test LEQUAL, no depth write, same LOD and palette, so the model
// brightens (texture x light added to what is already there: `dst + src`, clamped). Never for a TRANSPARENT_MODE mover.
//
// Every drawn mesh of the visual gets a TWIN: a mesh with the same geometry in the sibling position (so it follows the same node
// matrices) and a material that shares the shader, the defines and - by reference - the uniforms of the original (palette base,
// textures, fade, hair), so nothing has to be kept in sync. Twins live in `LAYER.highlight`, which only pass 5 draws.

import {
  BackSide,
  CustomBlending,
  FrontSide,
  GLSL3,
  LessEqualDepth,
  Mesh,
  OneFactor,
  RawShaderMaterial,
  type Material,
  type Object3D,
} from 'three';
import { LAYER } from '../layers';
import { det3x3 } from '../gl/materials/MaterialFactory';
import type { MoverVisual } from './MoverVisual';

/** The additive twin of `source`: same program and uniforms, `ONE, ONE`, LEQUAL, no depth write. */
export function highlightMaterial(source: RawShaderMaterial): RawShaderMaterial {
  const twin = new RawShaderMaterial({
    name: `${source.name}:highlight`,
    glslVersion: GLSL3,
    vertexShader: source.vertexShader,
    fragmentShader: source.fragmentShader,
    defines: { ...source.defines },
    uniforms: source.uniforms, // by reference: the twin always shows what the original shows
    side: source.side,
    depthTest: true,
    depthWrite: false,
    depthFunc: LessEqualDepth,
    transparent: true,
    blending: CustomBlending,
    blendSrc: OneFactor,
    blendDst: OneFactor,
    blendSrcAlpha: OneFactor,
    blendDstAlpha: OneFactor,
    fog: false,
    lights: false,
  });
  if (source.side === FrontSide) {
    // the mirrored-node culling of the material system (a negative determinant flips the winding)
    twin.onBeforeRender = (_r, _s, _c, _g, object: Object3D) => {
      twin.side = det3x3(object.matrixWorld) < 0 ? BackSide : FrontSide;
    };
  }
  return twin;
}

interface Twin {
  readonly mesh: Mesh;
  readonly source: Material | Material[];
  readonly material: RawShaderMaterial;
}

export class HighlightTwins {
  private target: MoverVisual | null = null;
  private readonly twins = new Map<Mesh, Twin>();

  /** The mover to highlight (`null` clears). Only one mover is highlighted at a time. */
  set(visual: MoverVisual | null): void {
    if (visual === this.target) return;
    this.clear();
    this.target = visual;
  }

  get visual(): MoverVisual | null {
    return this.target;
  }

  /** The twin meshes alive now (tests, probe). */
  get count(): number {
    return this.twins.size;
  }

  /** Once per frame, after the visual's update: twins for the meshes drawn now, none for the others. */
  sync(): void {
    const v = this.target;
    if (!v?.root.visible) {
      this.clear();
      return;
    }
    const live = new Set<Mesh>();
    v.forEachDrawnMesh((mesh) => {
      live.add(mesh);
      const parent = mesh.parent;
      if (!parent || !(mesh.material instanceof RawShaderMaterial)) return;
      let t = this.twins.get(mesh);
      if (t && t.source !== mesh.material) {
        this.drop(t);
        this.twins.delete(mesh);
        t = undefined;
      }
      if (!t) {
        const material = highlightMaterial(mesh.material);
        const twin = new Mesh(mesh.geometry, material);
        twin.name = `${mesh.name}:highlight`;
        twin.matrixAutoUpdate = false;
        twin.frustumCulled = false;
        twin.layers.set(LAYER.highlight);
        twin.renderOrder = 5;
        parent.add(twin);
        twin.matrixWorld.copy(parent.matrixWorld); // this frame's world matrix (the visual updated before the sync)
        t = { mesh: twin, source: mesh.material, material };
        this.twins.set(mesh, t);
      }
      t.mesh.visible = true;
    });
    for (const [src, t] of [...this.twins]) {
      if (!live.has(src)) {
        this.drop(t);
        this.twins.delete(src);
      }
    }
  }

  private drop(t: Twin): void {
    t.mesh.removeFromParent();
    t.material.dispose();
  }

  clear(): void {
    for (const t of this.twins.values()) this.drop(t);
    this.twins.clear();
    this.target = null;
  }
}
