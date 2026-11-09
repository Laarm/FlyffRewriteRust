// The lens flare (Plan/41 section 14.5, WP-REND-28; SkyBox.cpp:850-996 `DrawLensFlare` / `CheckSun`, pass 15).
//
//   window    hours 7..17 only; before 12:00 `m_fSunAngle >= -84.6`, from 12:00 `m_fSunAngle <= -81.66` where
//             `m_fSunAngle = nSec * 0.00015f - 90` in float32: visible 10:00:00..11:59:59 and 12:00:00..15:26:40 game time
//   occlusion the ray from the camera towards `RotX(angle) * (0, 0, 300)` must hit neither the terrain nor the bounding box of an
//             object (`IsPickTerrain` / `PickObject_Fast`); evaluated at most every 6 ticks and cached
//   layout    in the sky view: `sunPos = RotX(a) * (0, 0, 30)`, `c = normalize(lookAt - eye) * 30`, `delta = sunPos - c`,
//             `alpha = |delta| < 20 ? 255 - floor(|delta| * 255 / 20) : 0`; sprite i (0..6) at
//             `p_i = p_(i-1) - sunPos / 10 - delta / 10` with `p_(-1) = sunPos`, texture `lenzflare_00(7 - i)`, half size
//             `{2, 2, 1, 1, 1, 3, 4}[i]`, vertex colour `{(255,235,200) x3, (50,50,100), (100,0,0), (200,255,200), (255,255,255)}`
//             with the common alpha; camera-facing, colour = texture x vertex, alpha = texture x vertex, SRCALPHA / INVSRCALPHA

import { BufferAttribute, BufferGeometry, Mesh, Scene, type Texture } from 'three';
import { Prio } from '../../workers/priority';
import type { Vec3LH } from '../types';
import { QUAD_INDICES, QUAD_UVS } from './geometry';
import { billboardQuad, bodyPosition, mirrorQuadInto } from './celestial';
import { createBodyMaterial, type BodyMaterial } from './skyShaders';
import type { SkyTextures } from './skyTextures';
import type { Mat4 } from '../d3dx';

/** Half sizes of the 7 sprites (`afRadius[1..7]`). */
export const FLARE_RADII: readonly number[] = [2, 2, 1, 1, 1, 3, 4];
/** Vertex colours of the 7 sprites (`adwColor[1..7]` without the alpha). */
export const FLARE_COLORS: readonly (readonly [number, number, number])[] = [
  [255, 235, 200],
  [255, 235, 200],
  [255, 235, 200],
  [50, 50, 100],
  [100, 0, 0],
  [200, 255, 200],
  [255, 255, 255],
];
/** The distance of the view-centre reference point and the body from the origin. */
export const FLARE_RADIUS = 30;
/** The flare fades out over this distance between the sun and the view centre. */
export const FLARE_ALPHA_RANGE = 20;
/** The occlusion test runs at most every 6 ticks. */
export const FLARE_EVAL_TICKS = 6;

/** `m_fSunAngle` of the sun branch with the float32 arithmetic of the original: `(float)nSec * 0.00015f + -90`. */
export function sunAngleF32(hour: number, min: number, sec: number): number {
  const f = Math.fround;
  const nSec = hour * 3600 + min * 60 + sec;
  return f(f(f(nSec) * f(0.00015)) + -90);
}

/** `true` while the flare may be drawn at all (before the occlusion test). */
export function flareWindow(hour: number, min: number, sec: number): boolean {
  if (hour < 7 || hour > 17) return false;
  const a = sunAngleF32(hour, min, sec);
  if (hour < 12) return !(a < Math.fround(-84.6));
  return !(a > Math.fround(-81.66));
}

/** One sprite of the flare. */
export interface FlareSprite {
  /** LH position in the sky view space. */
  readonly pos: readonly [number, number, number];
  readonly half: number;
  /** `0..255`. */
  readonly color: readonly [number, number, number];
  /** Index into the 7 flare textures in draw order (`lenzflare_007` first). */
  readonly texture: number;
}

export interface FlareLayout {
  /** `0..255`: the common vertex alpha (0 = nothing visible). */
  readonly alpha: number;
  readonly sunPos: readonly [number, number, number];
  readonly sprites: readonly FlareSprite[];
}

/** The layout of the 7 sprites for the sun angle `angle` (radians) and the view centre `centre` (`normalize(look) * 30`). */
export function flareLayout(angle: number, centre: readonly [number, number, number]): FlareLayout {
  const sun = bodyPosition(angle);
  const delta: [number, number, number] = [
    sun[0] - centre[0],
    sun[1] - centre[1],
    sun[2] - centre[2],
  ];
  const len = Math.hypot(delta[0], delta[1], delta[2]);
  // `(int)(fLength * 255 / 20.0f)` truncates
  const alpha = len < FLARE_ALPHA_RANGE ? 255 - Math.trunc((len * 255) / FLARE_ALPHA_RANGE) : 0;
  const sprites: FlareSprite[] = [];
  let p: [number, number, number] = [sun[0], sun[1], sun[2]];
  for (let i = 0; i < 7; i++) {
    p = [
      p[0] - sun[0] / 10 - delta[0] / 10,
      p[1] - sun[1] / 10 - delta[1] / 10,
      p[2] - sun[2] / 10 - delta[2] / 10,
    ];
    sprites.push({
      pos: p,
      half: FLARE_RADII[i] ?? 1,
      color: FLARE_COLORS[i] ?? [255, 255, 255],
      texture: i,
    });
  }
  return { alpha, sunPos: sun, sprites };
}

/** Visual-only occlusion queries of the sun (terrain heights and object boxes of the renderer). */
export interface SunOcclusion {
  /** `true` when the ray from `origin` along `dir` (LH, unit length) is blocked within the far plane. */
  blocked(origin: Vec3LH, dir: Vec3LH): boolean;
}

/** The state of the cached occlusion test. */
export class FlareVisibility {
  private ticksLeft = 0;
  private cached = false;
  /** Occlusion tests run so far (probe, tests). */
  evaluations = 0;

  /** Cosmetic ticks passed. */
  tick(steps: number): void {
    this.ticksLeft = Math.max(0, this.ticksLeft - Math.max(0, steps));
  }

  /** The cached answer, re-evaluated when the cache ran out (`true` = the sun is visible). */
  visible(eval1: () => boolean): boolean {
    if (this.ticksLeft <= 0) {
      this.cached = eval1();
      this.evaluations++;
      this.ticksLeft = FLARE_EVAL_TICKS;
    }
    return this.cached;
  }

  reset(): void {
    this.ticksLeft = 0;
    this.cached = false;
  }
}

/** The GPU side: 7 camera-facing quads in the sky view. */
export class LensFlare {
  readonly scene = new Scene();
  private readonly meshes: Mesh[] = [];
  private readonly materials: BodyMaterial[] = [];
  private readonly corners = new Float32Array(12);
  private readonly mirrored = new Float32Array(12);
  readonly visibility = new FlareVisibility();
  /** The layout of the last frame (probe, tests), `null` when the flare is off. */
  layout: FlareLayout | null = null;
  private drawn = 0;

  constructor(
    private readonly textures: SkyTextures,
    private readonly occlusion: () => SunOcclusion | null,
  ) {
    this.scene.matrixAutoUpdate = false;
    this.scene.matrixWorldAutoUpdate = false;
    for (let i = 0; i < 7; i++) {
      const g = new BufferGeometry();
      g.setAttribute('position', new BufferAttribute(new Float32Array(12), 3));
      g.setAttribute('uv', new BufferAttribute(QUAD_UVS, 2));
      g.setIndex(new BufferAttribute(QUAD_INDICES, 1));
      const m = createBodyMaterial(`lens-flare-${String(i)}`, 'alpha');
      const mesh = new Mesh(g, m);
      mesh.frustumCulled = false;
      mesh.matrixAutoUpdate = false;
      mesh.visible = false;
      this.scene.add(mesh);
      this.meshes.push(mesh);
      this.materials.push(m);
    }
  }

  /** `true` when a frame issued draws. */
  get drawing(): boolean {
    return this.drawn > 0;
  }

  /** No flare this frame. */
  hide(): void {
    this.drawn = 0;
    for (const m of this.meshes) m.visible = false;
    this.layout = null;
  }

  /**
   * Prepares the flare of this frame. `view` is the sky view (LH), `eye` the camera position, `angle` the sun angle in
   * radians, `centre` the view-centre reference point (`normalize(look) * 30`).
   */
  update(
    clock: { hour: number; min: number; sec: number },
    angle: number,
    view: Mat4,
    eye: Vec3LH,
    centre: readonly [number, number, number],
  ): void {
    this.hide();
    if (!flareWindow(clock.hour, clock.min, clock.sec)) return;
    // the textures are requested as soon as the window opens (the first time an hour of 7..17 is seen)
    const paths = this.textures.paths().flares;
    const tex: (Texture | null)[] = paths.map((p) => this.textures.texture(p, Prio.Fx));
    const layout = flareLayout(angle, centre);
    if (layout.alpha <= 0) return;
    const occ = this.occlusion();
    const dir = bodyPosition(angle);
    const dl = Math.hypot(dir[0], dir[1], dir[2]) || 1;
    const visible = this.visibility.visible(() => {
      if (!occ) return true;
      return !occ.blocked(eye, [dir[0] / dl, dir[1] / dl, dir[2] / dl]);
    });
    if (!visible) return;
    this.layout = layout;
    const out = this.mirrored;
    layout.sprites.forEach((s, i) => {
      const t = tex[i];
      const mesh = this.meshes[i];
      const mat = this.materials[i];
      if (!t || !mesh || !mat) return;
      billboardQuad(view, s.pos, s.half, this.corners);
      mirrorQuadInto(this.corners, out);
      const attr = mesh.geometry.getAttribute('position') as BufferAttribute;
      (attr.array as Float32Array).set(out);
      attr.needsUpdate = true;
      mat.uniforms.uMap.value = t;
      mat.uniforms.uColor.value.set(
        s.color[0] / 255,
        s.color[1] / 255,
        s.color[2] / 255,
        layout.alpha / 255,
      );
      mesh.visible = true;
      this.drawn++;
    });
  }

  tick(steps: number): void {
    this.visibility.tick(steps);
  }

  dispose(): void {
    for (const m of this.meshes) {
      m.geometry.dispose();
      this.scene.remove(m);
    }
    for (const m of this.materials) m.dispose();
  }
}
