// The DOM face of the siege HUD (Plan/45 §28.4; WP-CFEAT-41): a transparent layer between the world canvas and the windows, in the virtual pixels of 42's UI (`pointer-events: none`). It paints
// what `hudModel.ts` computed: the timer, the boards, the roster and the "be ready" lines. Pattern of `features/hud/dps/DpsMeter.tsx`.

import { type CSSProperties, useSyncExternalStore } from 'react';
import { argbCss } from '../../../render/overlay/GlyphAtlas';
import { FONT, fontStyle } from '../../../ui/text/fonts';
import type { HudPanel, HudRect, HudText, SiegeHudModel } from './hudModel';

/** The model of the HUD as React state: set by the module, read by the component. */
export class SiegeHudState {
  #model: SiegeHudModel | null = null;
  #json = '';
  #version = 0;
  readonly #listeners = new Set<() => void>();

  get model(): SiegeHudModel | null {
    return this.#model;
  }
  readonly subscribe = (fn: () => void): (() => void) => {
    this.#listeners.add(fn);
    return () => {
      this.#listeners.delete(fn);
    };
  };
  readonly getVersion = (): number => this.#version;

  /** Replaces the model; a model equal to the current one changes nothing. */
  set(model: SiegeHudModel | null): void {
    const json = JSON.stringify(model);
    if (json === this.#json) return;
    this.#json = json;
    this.#model = model;
    this.#version += 1;
    for (const fn of [...this.#listeners]) fn();
  }
}

const rectStyle = (r: HudRect): CSSProperties => ({
  position: 'absolute',
  left: r.x,
  top: r.y,
  width: r.w,
  height: r.h,
  background: argbCss(r.color),
});

function Text({ t, testId }: { t: HudText; testId?: string }) {
  const style: CSSProperties = {
    position: 'absolute',
    left: t.x,
    top: t.y,
    whiteSpace: 'pre',
    color: argbCss(t.color),
    textShadow: t.shadow === undefined ? undefined : `1px 1px 0 ${argbCss(t.shadow)}`,
    transform: t.center === true ? 'translateX(-50%)' : undefined,
    ...fontStyle(t.big === true ? FONT.guildCombat : FONT.text),
  };
  return (
    <div style={style} data-testid={testId}>
      {t.text}
    </div>
  );
}

function Panel({ panel, testId }: { panel: HudPanel; testId: string }) {
  return (
    <div data-testid={testId}>
      <div style={rectStyle(panel.rect)} />
      {panel.texts.map((t, i) => (
        <Text key={i} t={t} />
      ))}
    </div>
  );
}

export interface SiegeHudProps {
  state: SiegeHudState;
  /** The virtual viewport of the UI. */
  viewport: { width: number; height: number };
  /** The UI scale (`UiScaler`). */
  scale: number;
}

export function SiegeHud({ state, viewport, scale }: SiegeHudProps) {
  useSyncExternalStore(state.subscribe, state.getVersion);
  const model = state.model;
  if (model === null) return null;
  const container: CSSProperties = {
    position: 'fixed',
    left: 0,
    top: 0,
    width: viewport.width,
    height: viewport.height,
    transform: scale === 1 ? undefined : `scale(${String(scale)})`,
    transformOrigin: '0 0',
    pointerEvents: 'none',
    zIndex: 0,
  };
  return (
    <div style={container} data-testid="siege-hud">
      {model.timer && <Text t={model.timer} testId="siege-hud-timer" />}
      {model.guildBoard && <Panel panel={model.guildBoard} testId="siege-hud-guild-board" />}
      {model.personBoard && <Panel panel={model.personBoard} testId="siege-hud-person-board" />}
      {model.roster && (
        <div data-testid="siege-hud-roster">
          <div style={rectStyle(model.roster.rect)} />
          {model.roster.texts.map((t, i) => (
            <Text key={i} t={t} />
          ))}
          {model.roster.bars.map((b, i) => (
            <div key={i} style={rectStyle(b)} />
          ))}
        </div>
      )}
      {model.joinReady.map((t, i) => (
        <Text key={i} t={t} testId="siege-hud-join-ready" />
      ))}
    </div>
  );
}
