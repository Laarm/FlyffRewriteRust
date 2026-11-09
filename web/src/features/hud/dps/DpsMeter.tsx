// The DOM face of the DPS meter (Plan/45 §7.10; WP-CFEAT-36): the header bar with the title, the dark body and one row per player (gauge,
// name, value) at the left edge of the world view, painted under the windows in the virtual pixels of 42's UI. The numbers are the model's
// (`model.ts`); this file only places them (`CWndDPSMeter::DrawDPSMeter`).

import { useSyncExternalStore, type CSSProperties } from 'react';
import { argbCss } from '../../../render/overlay/GlyphAtlas';
import {
  DPS_BODY_ARGB,
  DPS_HEADER_ARGB,
  DPS_HEADER_HEIGHT,
  DPS_NAME_ARGB,
  DPS_POS_Y,
  DPS_TITLE_ARGB,
  DPS_VALUE_ARGB,
  DPS_WIDTH,
  type DpsModel,
} from './model';

const FAMILY = '"Flyff DejaVu Sans", Verdana, sans-serif';
const FONT_PX = 12;
/** The text cell of a row (`PosY + 4` for the baseline box of the 15 px font cell). */
const CELL = 15;

/** The model of the meter as React state: set by the module, read by the component. */
export class DpsState {
  #model: DpsModel | null = null;
  #version = 0;
  readonly #listeners = new Set<() => void>();

  get model(): DpsModel | null {
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
  set(model: DpsModel | null): void {
    if (this.#model === model || JSON.stringify(this.#model) === JSON.stringify(model)) return;
    this.#model = model;
    this.#version += 1;
    for (const fn of [...this.#listeners]) fn();
  }
}

const box = (
  left: number,
  top: number,
  width: number,
  height: number,
  argb: number,
): CSSProperties => ({
  position: 'absolute',
  left,
  top,
  width,
  height,
  background: argbCss(argb),
});

const text = (left: number, top: number, argb: number): CSSProperties => ({
  position: 'absolute',
  left,
  top,
  height: CELL,
  lineHeight: `${String(CELL)}px`,
  whiteSpace: 'pre',
  fontSize: FONT_PX,
  fontWeight: 500,
  color: argbCss(argb),
});

export interface DpsMeterProps {
  state: DpsState;
  /** The virtual viewport of the UI. */
  viewport: { width: number; height: number };
  /** The UI scale (`UiScaler`). */
  scale: number;
}

export function DpsMeter({ state, viewport, scale }: DpsMeterProps) {
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
    fontFamily: FAMILY,
  };
  return (
    <div style={container} data-testid="dps-meter">
      <div
        style={box(0, DPS_POS_Y - DPS_HEADER_HEIGHT, DPS_WIDTH, DPS_HEADER_HEIGHT, DPS_HEADER_ARGB)}
      />
      <div style={text(35, DPS_POS_Y - 19, DPS_TITLE_ARGB)} data-testid="dps-title">
        {model.title}
      </div>
      <div style={box(0, DPS_POS_Y, DPS_WIDTH, model.bodyHeight, DPS_BODY_ARGB)} />
      {model.rows.map((r) => (
        <div key={r.y} data-testid="dps-row">
          {r.gauge ? <div style={box(1, r.y + 2, r.gauge.width - 1, 20, r.gauge.color)} /> : null}
          <div style={text(5, r.y + 4, DPS_NAME_ARGB)} data-testid="dps-name">
            {r.name}
          </div>
          <div style={text(135, r.y + 4, DPS_VALUE_ARGB)} data-testid="dps-value">
            {r.value}
          </div>
        </div>
      ))}
    </div>
  );
}
