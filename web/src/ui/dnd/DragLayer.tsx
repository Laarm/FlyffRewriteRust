// The drag layer (Plan/42 §10.2, ref/22 §16): the dragged icon (32 x 32 at the pointer minus 16 px, or the text when the
// shortcut has no icon, e.g. a chat macro) above every window and below the tooltips, and the forbid icon at the pointer
// above everything (`Neuz.cpp` draws: the shortcut icon, the tooltips, the forbid icon). The layer is transparent to the
// pointer, so the manager's hit test (never the DOM) decides about drop targets.

import { useSyncExternalStore } from 'react';
import { useSprite } from '../controls/useSprite';
import type { WndEnv } from '../wnd/env';
import { FONT, fontStyle, lineHeight } from '../text/fonts';
import { DRAG_ICON_OFFSET, FORBID_ICON, type DragController } from './DragController';

function Icon({
  env,
  path,
  x,
  y,
  cls = 'dnd-icon',
}: {
  env: WndEnv;
  path: string;
  x: number;
  y: number;
  cls?: string;
}) {
  const sprite = useSprite(env, path);
  if (typeof sprite !== 'object') return null;
  return (
    <div
      className={cls}
      data-testid={cls === 'dnd-icon' ? 'dnd-icon' : 'dnd-forbid'}
      style={{
        left: x,
        top: y,
        width: 32,
        height: 32,
        backgroundImage: `url("${sprite.url}")`,
        backgroundSize: '32px 32px',
        backgroundRepeat: 'no-repeat',
      }}
    />
  );
}

export function DragLayer({ controller, env }: { controller: DragController; env: WndEnv }) {
  const v = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
  const sc = v.shortcut;
  const x = v.pt.x + DRAG_ICON_OFFSET;
  const y = v.pt.y + DRAG_ICON_OFFSET;
  return (
    <>
      {sc &&
        (sc.icon ? (
          <Icon env={env} path={sc.icon} x={x} y={y} />
        ) : (
          <div
            className="dnd-text"
            data-testid="dnd-text"
            // `TextOut(point - GetMaxHeight() / 2)` in the default (white) colour, no texture
            style={{
              left: v.pt.x - Math.trunc(lineHeight(FONT.text) / 2),
              top: v.pt.y - Math.trunc(lineHeight(FONT.text) / 2),
              ...fontStyle(FONT.text),
            }}
          >
            {sc.text ?? ''}
          </div>
        ))}
      {v.forbid && (
        <Icon
          env={env}
          cls="dnd-forbid"
          path={FORBID_ICON}
          x={v.forbid.pt.x + DRAG_ICON_OFFSET}
          y={v.forbid.pt.y + DRAG_ICON_OFFSET}
        />
      )}
    </>
  );
}
