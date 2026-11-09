// The pixels of the world-map overlay (`CWorldMap::RenderWorldMap`, ref/99 §4.5, Plan/45 §7.7): a full-viewport layer UNDER the windows
// (the original draws it in the world window, the other windows stay above it). The background `0xff3B3D2F`, or black beside a letter-boxed
// 4:3 box; the map texture over the box; the nine continent buttons; the teleporter icons (16 x 16, 20 x 20 under the pointer); the arrows
// of the own character and of the party with their names. The layer takes no pointer events: the click arrives through the input hook
// (`InputHooks.worldMap.click`), the canvas below it receives the event.

import { useCallback, useSyncExternalStore } from 'react';
import { useSprite } from '../../controls/useSprite';
import { FONT, fontStyle } from '../../text/fonts';
import type { WndEnv } from '../../wnd/env';
import type { ViewSource, WorldMapView } from './ports';

const BG = '#3b3d2f';
/** The destination sprite: 16 x 16, `RenderScal( 0.6 )`; the alpha runs 0 -> 255 -> 0 at 800 per second (one cycle = 2 * 255 / 800 s). */
const DESTINATION_SIZE = 16;
const DESTINATION_SCALE = 0.6;
const DESTINATION_PERIOD_MS = Math.round((2 * 255 * 1000) / 800);
const DESTINATION_KEYFRAMES =
  '@keyframes wm-destination { 0% { opacity: 0 } 50% { opacity: 1 } 100% { opacity: 0 } }';

function Image({
  env,
  path,
  style,
  testId,
}: {
  env: WndEnv;
  path: string | null;
  style: React.CSSProperties;
  testId: string;
}) {
  const sprite = useSprite(env, path);
  const url = typeof sprite === 'object' ? sprite.url : null;
  return (
    <div
      data-testid={testId}
      data-path={path ?? undefined}
      style={{
        position: 'absolute',
        ...style,
        ...(url
          ? {
              backgroundImage: `url("${url}")`,
              backgroundSize: '100% 100%',
              backgroundRepeat: 'no-repeat',
            }
          : {}),
      }}
    />
  );
}

function ButtonStrip({
  env,
  path,
  cut,
  rect,
  index,
}: {
  env: WndEnv;
  path: string;
  cut: { x: number; y: number; w: number; h: number };
  rect: { l: number; t: number; r: number; b: number };
  index: number;
}) {
  const sprite = useSprite(env, path);
  const w = rect.r - rect.l;
  const h = rect.b - rect.t;
  const k = cut.w > 0 ? w / cut.w : 1;
  const style: React.CSSProperties = {
    position: 'absolute',
    left: rect.l,
    top: rect.t,
    width: w,
    height: h,
    backgroundRepeat: 'no-repeat',
  };
  if (typeof sprite === 'object') {
    style.backgroundImage = `url("${sprite.url}")`;
    style.backgroundSize = `${String(sprite.width * k)}px ${String(sprite.height * k)}px`;
    style.backgroundPosition = `${String(-cut.x * k)}px ${String(-cut.y * k)}px`;
  }
  return <div data-testid="worldmap-button" data-index={index} style={style} />;
}

export function WorldMapLayer({ view, env }: { view: ViewSource<WorldMapView>; env: WndEnv }) {
  const subscribe = useCallback((fn: () => void) => view.subscribe(fn), [view]);
  const get = useCallback(() => view.get(), [view]);
  const v = useSyncExternalStore(subscribe, get);
  if (!v.open) return null;
  const box = v.box;
  const wide = box.l > 0 || box.t > 0;
  const arrow = Math.max(16, Math.round(v.viewH * 0.07));
  return (
    <div
      data-testid="worldmap"
      data-map={v.mapNo}
      data-teleporter={v.teleporter ? '1' : '0'}
      style={{
        position: 'absolute',
        inset: 0,
        background: wide ? '#000' : BG,
        pointerEvents: 'none',
        zIndex: 0,
      }}
    >
      <Image
        env={env}
        path={v.texture}
        testId="worldmap-texture"
        style={{ left: box.l, top: box.t, width: box.r - box.l, height: box.b - box.t }}
      />
      {v.icons.map((i) => (
        <Image
          key={`icon:${String(i.index)}`}
          env={env}
          path="theme/buttteleport.bmp"
          testId="worldmap-icon"
          style={{ left: i.x, top: i.y, width: i.w, height: i.h }}
        />
      ))}
      {v.destination && (
        <>
          <style>{DESTINATION_KEYFRAMES}</style>
          <Image
            env={env}
            path="theme/buttdestination.bmp"
            testId="worldmap-destination"
            style={{
              left: v.destination.x,
              top: v.destination.y,
              width: DESTINATION_SIZE * DESTINATION_SCALE,
              height: DESTINATION_SIZE * DESTINATION_SCALE,
              animation: `wm-destination ${String(DESTINATION_PERIOD_MS)}ms linear infinite`,
            }}
          />
        </>
      )}
      {v.arrows.map((a, i) => (
        <div key={`arrow:${String(i)}`} data-testid="worldmap-arrow" data-self={a.self ? '1' : '0'}>
          <Image
            env={env}
            path={a.self ? 'theme/imgmaparrow.bmp' : 'theme/imgmaparrowparty.bmp'}
            testId="worldmap-arrow-img"
            style={{
              left: a.x - arrow / 2,
              top: a.y - arrow / 2,
              width: arrow,
              height: arrow,
              transform: `rotate(${String(a.heading)}deg)`,
            }}
          />
          <div
            style={{
              position: 'absolute',
              left: a.x,
              top: a.y - arrow * 0.9,
              transform: 'translate(-50%, -100%)',
              whiteSpace: 'nowrap',
              color: a.self ? '#7fff7f' : '#ffff7f',
              ...fontStyle(FONT.title),
            }}
          >
            {a.name}
          </div>
        </div>
      ))}
      {v.buttons.map((b, i) =>
        b.sprite ? (
          <ButtonStrip
            key={`button:${String(i)}`}
            env={env}
            path={b.sprite.path}
            cut={b.sprite}
            rect={b.rect}
            index={i}
          />
        ) : null,
      )}
    </div>
  );
}
