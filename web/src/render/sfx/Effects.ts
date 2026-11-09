// The cosmetic world around the SFX runtime (Plan/41 section 18.1, 18.2, WP-REND-21): the ribbons, the point-sprite particles, the
// melee sparks and the projectile visuals, with their draw passes 10 (ribbons) and 7 (particles). `WorldRenderer.attachSfx` builds one
// per renderer next to the `SfxSystem`; `tick(n)` runs the 60 Hz steps, `update` places the projectile visuals and fills the buffers.

import type { Texture } from 'three';
import { Prio } from '../../workers/priority';
import type { TextureHandle } from '../gl/textures';
import type { RenderPass } from '../passes';
import {
  MeleeSparks,
  ParticleMng,
  ParticleRenderer,
  particleSpec,
  type SparkWorld,
} from './ParticleMng';
import {
  ARROW_RIBBON_TEXTURE,
  ProjectileVisuals,
  type ProjectileView,
  type ProjectileWorld,
  type ProjectileStats,
} from './Projectiles';
import { HotAirVisuals, type HotAirStats, type HotAirWorld } from './hotAir';
import type { SfxSystem } from './SfxSystem';
import { TailMng, TailRenderer } from './Tails';
import { YoyoFlights, type YoyoHooks, type YoyoStats, type YoyoWorld } from './yoyo';

/** What the effects need of the texture cache. */
export interface EffectTextures {
  acquire(path: string, preset: 'world', options: { prio: Prio }): TextureHandle;
}

export interface EffectsDeps {
  readonly sys: SfxSystem;
  readonly textures: EffectTextures;
  /** The world of the frame (positions, the own character, the hands). */
  readonly world: () => SparkWorld & ProjectileWorld & YoyoWorld & HotAirWorld;
  /** The game's answers for the yo-yo flights (the weapon model, hiding the real yo-yo). */
  readonly yoyoHooks?: () => YoyoHooks;
  /** The audio cue of the Hot Air pain tick (44 T08h). */
  readonly hotAirPainTick?: () => HotAirPainCue | undefined;
  /** The sound hook of effects the projectile visuals make themselves (T41, `SkillFxHooks.sfxCreated`). */
  readonly created?: (xi: number, pos: readonly [number, number, number] | undefined) => void;
  /** `SFXRenderOff`: no melee sparks are made. */
  readonly sfxRenderOff: () => boolean;
  /** `[0, 1)` source of the sparks. */
  readonly rnd?: () => number;
}

/** `hotAirPainTick(pos, first, attacker, target)`. */
export type HotAirPainCue = (
  pos: readonly [number, number, number],
  first: boolean,
  attacker: number,
  target: number,
) => void;

/** What `update` needs of the frame. */
export interface EffectsFrame {
  readonly view: ProjectileView | null;
  /** The camera position (LH). */
  readonly cameraPos: readonly [number, number, number];
  /** `2 tan(fovY / 2)`. */
  readonly pointScale: number;
  /** The viewport height in pixels. */
  readonly viewportHeight: number;
}

export class SfxEffects {
  readonly tails = new TailMng();
  readonly particles = new ParticleMng();
  readonly sparks: MeleeSparks;
  readonly projectiles: ProjectileVisuals;
  /** The flying yo-yos of the yo-yo skills (WP-REND-24). */
  readonly yoyo: YoyoFlights;
  /** The Hot Air visuals (WP-REND-24). */
  readonly hotAir: HotAirVisuals;
  readonly tailGl: TailRenderer;
  readonly particleGl: ParticleRenderer;
  private readonly handles = new Map<string, TextureHandle>();

  constructor(private readonly deps: EffectsDeps) {
    const textureOf = (path: string): Texture | null => this.textureOf(path);
    this.tailGl = new TailRenderer({ textureOf });
    this.particleGl = new ParticleRenderer({ textureOf });
    this.sparks = new MeleeSparks(this.particles, deps.world, deps.rnd ?? Math.random);
    this.yoyo = new YoyoFlights({
      sfx: deps.sys,
      world: deps.world,
      ...(deps.yoyoHooks ? { hooks: deps.yoyoHooks } : {}),
    });
    this.hotAir = new HotAirVisuals({
      sfx: deps.sys,
      world: deps.world,
      painTick: () => deps.hotAirPainTick?.(),
    });
    this.projectiles = new ProjectileVisuals({
      sfx: deps.sys,
      tails: this.tails,
      particles: this.particles,
      world: deps.world,
      ...(deps.created ? { created: deps.created } : {}),
      step: () => {
        this.tails.step();
        this.particles.step();
        this.sparks.tick(deps.sfxRenderOff());
      },
    });
  }

  /**
   * Starts loading the textures of the common cases (the arrow ribbon, the wand dots and the hit sparks, types 0..9) so that the first
   * flight or hit already shows them (they are small).
   */
  preload(): void {
    this.textureOf(ARROW_RIBBON_TEXTURE);
    for (let t = 0; t <= 9; t++) this.textureOf(particleSpec(t).texture);
  }

  private textureOf(path: string): Texture | null {
    let h = this.handles.get(path);
    if (!h) {
      h = this.deps.textures.acquire(path, 'world', { prio: Prio.Fx });
      this.handles.set(path, h);
    }
    return h.state === 'ready' ? h.texture : null;
  }

  /** The passes to register: ribbons (10) and particles (7). */
  passes(): RenderPass[] {
    return [this.tailGl.pass(), this.particleGl.pass()];
  }

  /** `steps` 60 Hz ticks. */
  tick(steps: number): void {
    this.projectiles.tick(steps);
    this.yoyo.tick(steps);
    this.hotAir.tick(steps);
  }

  /** Places the projectile visuals and fills the draw buffers of the frame (before the passes run). */
  update(frame: EffectsFrame): void {
    this.projectiles.update(frame.view, frame.cameraPos);
    this.yoyo.update();
    this.hotAir.update();
    this.tailGl.render(this.tails);
    this.particleGl.render(this.particles, {
      k: frame.pointScale,
      viewportHeight: frame.viewportHeight,
    });
  }

  /** A world change: nothing flies, no ribbon or particle is left. */
  clear(): void {
    this.projectiles.clear();
    this.yoyo.clear();
    this.hotAir.clear();
    this.tails.clear();
    this.particles.clear();
    this.sparks.clear();
  }

  get texturesLoading(): number {
    let n = 0;
    for (const h of this.handles.values()) if (h.state === 'loading' || h.state === 'lost') n++;
    return n;
  }

  probe(): {
    projectiles: ProjectileStats;
    yoyo: YoyoStats;
    hotAir: HotAirStats;
    ribbons: number;
    ribbonNodes: number;
    ribbonDraws: number;
    particles: number;
    particleTypes: number;
    particleDraws: number;
    sparks: number;
    texturesLoading: number;
  } {
    return {
      projectiles: { ...this.projectiles.stats },
      yoyo: { ...this.yoyo.stats },
      hotAir: { ...this.hotAir.stats },
      ribbons: this.tails.ribbons,
      ribbonNodes: this.tails.nodes,
      ribbonDraws: this.tailGl.stats.draws,
      particles: this.particles.live,
      particleTypes: this.particles.typeCount,
      particleDraws: this.particleGl.stats.draws,
      sparks: this.sparks.active,
      texturesLoading: this.texturesLoading,
    };
  }

  dispose(): void {
    this.clear();
    this.tailGl.dispose();
    this.particleGl.dispose();
    for (const h of this.handles.values()) h.release();
    this.handles.clear();
  }
}
