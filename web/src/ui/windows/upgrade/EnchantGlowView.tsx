// The pixels of the enchant glow: one `.flyff-enchant` box (the animation of `icons/anim.ts`) per spot.

import { useSyncExternalStore } from 'react';
import type { EnchantGlow } from './EnchantGlow';

export function EnchantGlowView({ control }: { control: EnchantGlow }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  return (
    <div
      data-testid="enchant-glow"
      style={{ position: 'absolute', inset: 0, pointerEvents: 'none' }}
    >
      {s.spots.map((p) => (
        <div
          key={`${String(p.x)}:${String(p.y)}`}
          className="flyff-enchant"
          data-testid="enchant"
          style={{ left: p.x, top: p.y }}
        />
      ))}
    </div>
  );
}
