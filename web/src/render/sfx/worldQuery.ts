// The SFX system's view of the world for one frame (`WorldQuery`): object positions and yaw from the sim render view (movers),
// the entity table for everything the sim does not simulate (ground items, ctrls), and the player test from the entity records.
// One object is reused for every frame (`set`).

import type { EntityViewSource, FrameInput } from '../types';
import {
  asAttachView,
  eventWorldPos,
  handWorldPos,
  moverRadius,
  moverScale,
  type AttachView,
} from './attachPos';
import type { BoneSource } from './bonePos';
import type { V3, WorldQuery } from './SfxSystem';

/** The part of `RenderViewReader` the SFX reads. */
interface PosView {
  readonly valid?: boolean;
  posOf(objId: number, out: V3): boolean;
  angleOf(objId: number): number;
  stateOf?(objId: number): number;
}

interface SpawnLike {
  readonly obj?: { readonly pos?: { readonly x: number; readonly y: number; readonly z: number } };
}

interface EntityLike {
  readonly isSelf?: boolean;
  readonly visual?: { readonly kind?: string };
  readonly spawn?: SpawnLike;
}

export class FrameWorld implements WorldQuery {
  private view: PosView | null = null;
  private attach: AttachView | null = null;
  /** The terrain height `(x, z) -> y` (`CWorld::GetLandHeight_Fast`); the renderer sets it. */
  height: ((x: number, z: number) => number | undefined) | null = null;
  /** The bone lookups over the movers' poses (render-audio-m7; the renderer sets it once it has movers). */
  bones: BoneSource | null = null;
  /** `MoverProp.dwClass` of a mover (the renderer reads the mover batcher's entry). */
  rank: ((id: number) => number | undefined) | null = null;
  private entities: EntityViewSource | null = null;
  private active: readonly [number, number, number] | null = null;

  set(frame: Pick<FrameInput, 'view' | 'entities' | 'activePos'>): this {
    this.active = frame.activePos ?? null;
    const v = frame.view as unknown as PosView | null;
    this.view = v && typeof v.posOf === 'function' && v.valid !== false ? v : null;
    this.attach = asAttachView(frame.view);
    this.entities = frame.entities;
    return this;
  }

  posOf(id: number, out: V3): boolean {
    if (this.view?.posOf(id, out) === true) return true;
    // ground items and ctrls are not simulated: the position of their spawn message
    const rec = this.entities?.get(id) as EntityLike | undefined;
    const p = rec?.spawn?.obj?.pos;
    if (!p) return false;
    out[0] = p.x;
    out[1] = p.y;
    out[2] = p.z;
    return true;
  }

  angleOf(id: number): number {
    return this.view ? this.view.angleOf(id) : Number.NaN;
  }

  stateOf(id: number): number {
    return this.view?.stateOf?.(id) ?? 0;
  }

  handPos(id: number, hand: 0 | 1, out: V3): boolean {
    return this.attach !== null && handWorldPos(this.attach, id, hand, out);
  }

  eventPos(id: number, index: number, out: V3): boolean {
    return this.attach !== null && eventWorldPos(this.attach, id, index, out);
  }

  radiusOf(id: number): number | undefined {
    return this.attach ? moverRadius(this.attach, id) : undefined;
  }

  bonePos(id: number, bone: number, local: readonly [number, number, number], out: V3): boolean {
    return this.bones?.pos(id, bone, local, out) === true;
  }

  bonePose(id: number, bone: number, world: number[], bone16: number[]): boolean {
    return this.bones?.pose(id, bone, world, bone16) === true;
  }

  rankOf(id: number): number | undefined {
    return this.rank?.(id);
  }

  scaleOf(id: number): number | undefined {
    return this.attach ? moverScale(this.attach, id) : undefined;
  }

  heightAt(x: number, z: number): number | undefined {
    return this.height?.(x, z);
  }

  /** The own (active) mover's position of the frame; `false` where the frame names none (dev pages). */
  activePos(out: V3): boolean {
    const a = this.active;
    if (!a) return false;
    out[0] = a[0];
    out[1] = a[1];
    out[2] = a[2];
    return true;
  }

  /** The object is the own character. */
  isActive(id: number): boolean {
    const rec = this.entities?.get(id) as EntityLike | undefined;
    return rec?.isSelf === true;
  }

  isPlayer(id: number): boolean {
    const rec = this.entities?.get(id) as EntityLike | undefined;
    return rec?.visual?.kind === 'player';
  }
}
