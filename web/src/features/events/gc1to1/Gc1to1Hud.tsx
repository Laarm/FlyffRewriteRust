// The DOM face of the 1:1 guild combat HUD (Plan/45 §28.5; WP-CFEAT-45): a transparent layer between the world canvas and the windows, in the virtual pixels of 42's UI (`pointer-events: none`,
// like `SiegeHud`). It paints what `hudModel.ts` computed: the left board of the own line-up (`data-testid` `gc1to1-hud-players`) and the right board of the round wins (`gc1to1-hud-wins`). The
// timer line of the combat is the siege HUD's.

import { type CSSProperties, useSyncExternalStore } from 'react';
import { argbCss } from '../../../render/overlay/GlyphAtlas';
import { FONT, fontStyle } from '../../../ui/text/fonts';
import type { HudPanel, HudText } from '../siege/hudModel';
import type { Gc1to1HudModel } from './hudModel';

/** The model of the HUD as React state: set by the module, read by the component. */
export class Gc1to1HudState {
  #model: Gc1to1HudModel | null = null;
  #json = '';
  #version = 0;
  readonly #listeners = new Set<() => void>();

  get model(): Gc1to1HudModel | null {
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
  set(model: Gc1to1HudModel | null): void {
    const json = JSON.stringify(model);
    if (json === this.#json) return;
    this.#json = json;
    this.#model = model;
    this.#version += 1;
    for (const fn of [...this.#listeners]) fn();
  }
}

function Text({ t }: { t: HudText }) {
  const style: CSSProperties = {
    position: 'absolute',
    left: t.x,
    top: t.y,
    whiteSpace: 'pre',
    color: argbCss(t.color),
    textShadow: t.shadow === undefined ? undefined : `1px 1px 0 ${argbCss(t.shadow)}`,
    ...fontStyle(FONT.text),
  };
  return <div style={style}>{t.text}</div>;
}

function Panel({ panel, testId }: { panel: HudPanel; testId: string }) {
  const r = panel.rect;
  return (
    <div data-testid={testId}>
      <div
        style={{
          position: 'absolute',
          left: r.x,
          top: r.y,
          width: r.w,
          height: r.h,
          background: argbCss(r.color),
        }}
      />
      {panel.texts.map((t, i) => (
        <Text key={i} t={t} />
      ))}
    </div>
  );
}

export interface Gc1to1HudProps {
  state: Gc1to1HudState;
  /** The virtual viewport of the UI. */
  viewport: { width: number; height: number };
  /** The UI scale (`UiScaler`). */
  scale: number;
}

export function Gc1to1Hud({ state, viewport, scale }: Gc1to1HudProps) {
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
    <div style={container} data-testid="gc1to1-hud">
      <Panel panel={model.players} testId="gc1to1-hud-players" />
      <Panel panel={model.wins} testId="gc1to1-hud-wins" />
    </div>
  );
}
