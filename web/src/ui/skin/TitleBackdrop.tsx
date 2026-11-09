// The title-screen wallpaper (Plan/42 §7.7, `CTheme::RenderWallpaper`): on the login and character states the root
// shows `Theme/Screen.bmp` stretched into a centred box of `min(vw, round(vh * 4 / 3))` x `vh` on black; in game it
// paints nothing. The box rule is `wallpaperBox` (pure, `skin/wallpaper.ts`); the image is an ordinary `<img>` so it
// is ramped, scaled and screenshotted like the rest of the root.

import { spriteCell } from '../controls/useSprite';
import { useSyncExternalStore } from 'react';
import type { WndEnv } from '../wnd/env';
import { WALLPAPER_FILE, wallpaperBox } from './wallpaper';

export interface TitleBackdropProps {
  env: WndEnv;
  /** The virtual viewport. */
  width: number;
  height: number;
}

export function TitleBackdrop({ env, width, height }: TitleBackdropProps) {
  const cell = spriteCell(env, WALLPAPER_FILE);
  const sprite = useSyncExternalStore(cell.subscribe, cell.get);
  const box = wallpaperBox(width, height);
  return (
    <div className="ui-backdrop" data-testid="ui-backdrop" style={{ width, height }}>
      {typeof sprite === 'object' && (
        <img
          className="ui-backdrop-img"
          data-testid="ui-backdrop-img"
          src={sprite.url}
          alt=""
          draggable={false}
          style={{ left: box.l, top: box.t, width: box.r - box.l, height: box.b - box.t }}
        />
      )}
    </div>
  );
}
