// The pixels of the weapon stage: the icon, enlarged, turning on the X axis; Start swaps the idle turn for the accelerating CSS animation and a
// pale quad that fades over the picture at the end (keyframes in `styles/ui.css`).

import { useSyncExternalStore } from 'react';
import { useSprite } from '../../controls/useSprite';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { SPIN_FPS } from './spin';
import type { WeaponStage } from './WeaponStage';

/** The icon is drawn at three times its natural size (the model fills the canvas in the original). */
export const STAGE_ICON = 96;

export function WeaponStageView({ control }: { control: WeaponStage }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const sprite = useSprite(control.env, s.icon);
  const url = typeof sprite === 'object' ? sprite.url : null;
  const w = rectWidth(s.rect);
  const h = rectHeight(s.rect);
  const spin = s.spinning ? `weapon-spin ${String(s.spin.ms)}ms ease-in forwards` : undefined;
  // the pale quad starts at `m_nDelay < 10` and is opaque (alpha 230) when the request goes out
  const flashAt = Math.round((s.spin.flashFrame * 1000) / SPIN_FPS);
  const flash = s.spinning
    ? `weapon-flash ${String(s.spin.ms - flashAt)}ms linear ${String(flashAt)}ms forwards`
    : undefined;
  return (
    <div className="weapon-stage" data-testid="weapon-stage" data-spinning={s.spinning}>
      {s.icon !== null && (
        <div
          className={s.spinning ? 'weapon-stage-icon' : 'weapon-stage-icon weapon-stage-idle'}
          data-testid="weapon-stage-icon"
          style={{
            position: 'absolute',
            left: Math.trunc((w - STAGE_ICON) / 2),
            top: Math.trunc((h - STAGE_ICON) / 2),
            width: STAGE_ICON,
            height: STAGE_ICON,
            backgroundRepeat: 'no-repeat',
            backgroundSize: `${String(STAGE_ICON)}px ${String(STAGE_ICON)}px`,
            ...(url ? { backgroundImage: `url("${url}")` } : {}),
            ...(spin
              ? ({
                  animation: spin,
                  '--spin-deg': `${String(s.spin.degrees)}deg`,
                } as React.CSSProperties)
              : {}),
          }}
        />
      )}
      {s.spinning && (
        <div
          className="weapon-stage-flash"
          data-testid="weapon-stage-flash"
          style={{ animation: flash }}
        />
      )}
    </div>
  );
}
