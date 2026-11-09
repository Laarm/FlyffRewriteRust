// The pixels of the minimap: one `<canvas>` over the whole control, sized for the device pixel ratio. The window draws into it (`NavMapCtrl.redraw`).

import { useCallback, useEffect, useSyncExternalStore } from 'react';
import type { NavMapCtrl } from './NavMapCtrl';

export function NavMapView({ control }: { control: NavMapCtrl }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const dpr = typeof devicePixelRatio === 'number' && devicePixelRatio > 0 ? devicePixelRatio : 1;
  const ref = useCallback(
    (el: HTMLCanvasElement | null) => {
      control.attachCanvas(el);
    },
    [control],
  );
  useEffect(() => {
    // a resized canvas is cleared by the browser: draw it again at once
    control.redraw(control.env.host.now());
  }, [control, s.w, s.h, dpr]);
  return (
    <canvas
      ref={ref}
      data-testid="nav-map"
      width={Math.max(1, Math.round(s.w * dpr))}
      height={Math.max(1, Math.round(s.h * dpr))}
      style={{
        position: 'absolute',
        left: 0,
        top: 0,
        width: s.w,
        height: s.h,
        pointerEvents: 'none',
      }}
    />
  );
}
