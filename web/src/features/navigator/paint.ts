// Draws a `NavScene` (the pixels of `CWndNavigator::OnEraseBkgnd`, WP-CFEAT-28): the land textures at alpha 200, the dots and icons, the
// focus marker, the fog of an indoor world, the marks and the player arrow in the centre (the 32 x 32 billboard scaled by 1.2).
// Pure drawing: no state, the canvas context and the pictures come in.

import { dotBox } from './dots';
import { type Frame } from './sprites';
import type { NavScene } from './scene';
import type { MinimapTiles } from './tiles';

/** `m_texMiniMap.Render( ..., 200 )`. */
export const TILE_ALPHA = 200 / 255;
/** The arrow billboard: 32 x 32 at the centre, scaled by `1.2`. */
export const ARROW_SIZE = 32;
export const ARROW_SCALE = 1.2;
/** `RenderMark`: the 32 x 32 sprite at half size, centred on the point. */
export const MARK_SIZE = 16;
/** The destination sprite: `RenderScal( ..., 0.4, 0.4 )` at the point minus 3 (`WndField.cpp:9916-9935`). */
export const DESTINATION_SCALE = 0.4;
export const DESTINATION_OFFSET = 3;

export interface PaintDeps {
  readonly tiles: Pick<MinimapTiles, 'tile'>;
  readonly object: (frame: number) => Frame | null;
  readonly mark: (frame: number) => Frame | null;
  readonly arrow: Frame | null;
  readonly fog: Frame | null;
  /** The destination sprite (`ButtDestination.bmp`), absent = none. */
  readonly destination?: Frame | null;
}

function blit(
  ctx: CanvasRenderingContext2D,
  f: Frame,
  x: number,
  y: number,
  w = f.w,
  h = f.h,
): void {
  ctx.drawImage(f.sheet, f.sx, f.sy, f.w, f.h, x, y, w, h);
}

export function paintScene(ctx: CanvasRenderingContext2D, scene: NavScene, d: PaintDeps): void {
  const { w, h } = scene.view;
  ctx.imageSmoothingEnabled = false;
  // the land textures
  ctx.globalAlpha = TILE_ALPHA;
  for (const t of scene.tiles) {
    const img = d.tiles.tile(t.lx, t.lz);
    if (img) ctx.drawImage(img as CanvasImageSource, t.x, t.y, t.size, t.size);
  }
  ctx.globalAlpha = 1;
  if (scene.fogged) {
    // `RenderTexture( ( 0, 0 ), &m_texDunFog )` at the size of the client
    if (d.fog) blit(ctx, d.fog, 0, 0, w, h);
  } else {
    for (const s of scene.dots) {
      const f = d.object(s.dot.frame);
      if (!f) continue;
      const b = dotBox(s.dot);
      blit(ctx, f, s.x + b.dx, s.y + b.dy);
    }
    for (const s of scene.quests) {
      const f = d.object(s.dot.frame);
      if (f) blit(ctx, f, s.x - 5, s.y - 5);
    }
    if (scene.focus) {
      const f = d.object(5);
      if (f) blit(ctx, f, scene.focus.x - 3, scene.focus.y - 3);
    }
    // the destination arrow, after the dots and the focus
    const dest = d.destination;
    if (scene.destination && dest) {
      blit(
        ctx,
        dest,
        scene.destination.x - DESTINATION_OFFSET,
        scene.destination.y - DESTINATION_OFFSET,
        dest.w * DESTINATION_SCALE,
        dest.h * DESTINATION_SCALE,
      );
    }
  }
  // the marks (own and party) are drawn in any world
  const mf = d.mark(scene.markFrame);
  if (mf) {
    for (const m of scene.marks)
      blit(ctx, mf, m.x - MARK_SIZE / 2, m.y - MARK_SIZE / 2, MARK_SIZE, MARK_SIZE);
  }
  // the arrow of the player, turned by the camera
  if (d.arrow) {
    const size = Math.round(ARROW_SIZE * ARROW_SCALE);
    ctx.save();
    ctx.translate(Math.trunc(w / 2), Math.trunc(h / 2));
    ctx.rotate((scene.heading * Math.PI) / 180);
    blit(ctx, d.arrow, -size / 2, -size / 2, size, size);
    ctx.restore();
  }
}
