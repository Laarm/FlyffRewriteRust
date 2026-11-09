// `useSprite(env, path)`: the sprite of a textured control as React state. One observable cell per (image cache,
// path) is shared by every control that shows the image, loaded once; a cell starts as `loading` (the view draws
// nothing, so a textured button never flashes its untextured look) and ends as the sprite or `missing`.

import { useSyncExternalStore } from 'react';
import { themePath } from '../skin/images';
import type { WndEnv } from '../wnd/env';
import { Observable } from '../wnd/observable';
import { loadSprite, peekSprite, type SpriteState } from './sprites';

const cells = new WeakMap<object, Map<string, Observable<SpriteState>>>();
const NONE = new Observable<SpriteState>('missing');

/** The cell of one image (created and started on first use). */
export function spriteCell(env: WndEnv, path: string): Observable<SpriteState> {
  let byPath = cells.get(env.images);
  if (!byPath) {
    byPath = new Map();
    cells.set(env.images, byPath);
  }
  const key = themePath(path);
  let cell = byPath.get(key);
  if (!cell) {
    const cached = peekSprite(env, path);
    cell = new Observable<SpriteState>(cached ?? 'loading');
    byPath.set(key, cell);
    if (cached === undefined) {
      const c = cell;
      void loadSprite(env, path).then(
        (sp) => {
          c.set(sp ?? 'missing');
        },
        () => {
          c.set('missing');
        },
      );
    }
  }
  return cell;
}

/** The sprite state of a theme image (`null` path: `missing`, i.e. "this control has no texture"). */
export function useSprite(env: WndEnv, path: string | null): SpriteState {
  const cell = path === null || path === '' ? NONE : spriteCell(env, path);
  return useSyncExternalStore(cell.subscribe, cell.get);
}
