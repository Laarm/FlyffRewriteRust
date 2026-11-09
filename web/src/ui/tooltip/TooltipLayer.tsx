// The DOM face of the tooltips (Plan/42 §9.1-§9.3): one layer above every window and the dragged icon that renders the
// boxes of the `TooltipService`: the 9-tile frame on a canvas 8 px around the text rect, the text at (+3, +3) with a line
// pitch of cell + 2, and the ultimate band (icon, gem slots) for ultimate weapons. The layer never takes the pointer.

import { memo, useEffect, useRef, useSyncExternalStore } from 'react';
import { paintTooltipFrame, tileCanvas, tooltipFrame } from '../skin/tooltipFrame';
import { MISSING, themePath, type UiImageCache } from '../skin/images';
import { FONT } from '../text/fonts';
import { RichText } from '../text/RichText';
import { Observable } from '../wnd/observable';
import type { TooltipBoxView, TooltipService } from './TooltipService';

type Tiles = readonly (CanvasImageSource | null)[];

const tileCells = new WeakMap<UiImageCache, Observable<Tiles>>();

/** The nine tile canvases of one image cache (loaded once, shared by every tooltip). */
function tilesOf(images: UiImageCache): Observable<Tiles> {
  let cell = tileCells.get(images);
  if (!cell) {
    const c = new Observable<Tiles>([]);
    cell = c;
    tileCells.set(images, c);
    void Promise.all(
      Array.from({ length: 9 }, (_v, i) =>
        images.get(themePath(`WndTooltipTile0${String(i)}.tga`), 'exact'),
      ),
    ).then((imgs) => {
      c.set(imgs.map((img) => (img === MISSING ? null : tileCanvas(img))));
    });
  }
  return cell;
}

function FrameCanvas({ box, tiles }: { box: TooltipBoxView; tiles: Tiles }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const w = box.w;
  const h = box.h;
  useEffect(() => {
    const ctx = ref.current?.getContext('2d');
    if (!ctx || tiles.length === 0) return;
    ctx.clearRect(0, 0, w + 16, h + 16);
    paintTooltipFrame(ctx, tiles, tooltipFrame(w, h), 8, 8);
  }, [tiles, w, h]);
  return (
    <canvas
      ref={ref}
      className="tt-frame"
      width={w + 16}
      height={h + 16}
      style={{ left: box.x - 8, top: box.y - 8 }}
    />
  );
}

const Box = memo(function Box({
  box,
  images,
  tiles,
}: {
  box: TooltipBoxView;
  images: UiImageCache;
  tiles: Tiles;
}) {
  const u = box.ultimate;
  const slotsLeft = box.x + 6 + Math.trunc((box.w - 194) / 2);
  const slotsTop = box.y + box.h - 32;
  const iconUrl = u ? images.url(u.icon, 'exact') : null;
  const slotBg = u ? images.url('theme/WndChgElemItem.bmp', 'exact') : null;
  return (
    <div className="tt-box" data-kind={box.kind} data-testid="tooltip">
      <FrameCanvas box={box} tiles={tiles} />
      <RichText
        lines={box.lines}
        font={FONT.text}
        lineSpace={2}
        className="tt-text"
        style={{ left: box.x + 3, top: box.y + 3 }}
      />
      {u && iconUrl && (
        <div
          className="tt-ultimate"
          style={{
            left: box.x + 2,
            top: box.y + (box.kind === 'main' ? 0 : 16),
            backgroundImage: `url("${iconUrl}")`,
          }}
        />
      )}
      {u &&
        Array.from({ length: u.slots }, (_v, i) => {
          const gem = u.gems?.[i] ?? null;
          const gemUrl = gem === null ? null : images.url(gem, 'exact');
          return (
            <div key={i} className="tt-slot" style={{ left: slotsLeft + 38 * i, top: slotsTop }}>
              {slotBg && (
                <div className="tt-slot-bg" style={{ backgroundImage: `url("${slotBg}")` }} />
              )}
              {gemUrl && <div className="tt-gem" style={{ backgroundImage: `url("${gemUrl}")` }} />}
            </div>
          );
        })}
    </div>
  );
});

export function TooltipLayer({
  service,
  images,
}: {
  service: TooltipService;
  images: UiImageCache;
}) {
  const view = useSyncExternalStore(service.subscribe, service.getSnapshot);
  const cell = tilesOf(images);
  const tiles = useSyncExternalStore(cell.subscribe, cell.get);
  if (!view) return null;
  return (
    <div className="tt-layer" role="tooltip" data-owner-tooltip="true">
      {view.boxes.map((b) => (
        <Box key={b.kind} box={b} images={images} tiles={tiles} />
      ))}
    </div>
  );
}
