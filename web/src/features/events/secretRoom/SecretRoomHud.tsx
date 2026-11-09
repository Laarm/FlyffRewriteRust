// The DOM face of the war board of the secret room (Plan/45 §28.6; WP-CFEAT-52): a transparent layer between the world canvas and the windows, in the virtual pixels of 42's UI (`pointer-events:
// none`: it claims no pointer, BFIX1). It paints what `hudModel.ts` computed and plays the animations of `WndWorld.cpp:1874-2060` as CSS animations with the original's frame counts at 60 fps:
//
//   digit pop    when the own kills change, every digit from the most significant non-zero one on scales 2.5 -> 1.0 in 10 frames (-0.15 per frame) about its centre (the original drew it at
//                `x - 6 (s - 1)`, `y - 8 (s - 1)`) while it fades 255 -> 155 (-10 per frame; FIX: the original's alpha was reset at the creation of the world only, so later pops faded from ever lower values)
//   flash        a white 6 px outline round the own row, 14 frames in and out for EVERY change (FIX: the original's first flash only faded out, the later ones faded in and out)
//   lamp         the outline of a guild in MidBoss / Boss / Win pulses between transparent and the state colour at alpha 0xBE: 25 frames per half period (MidBoss, Boss), 100 (Win)
//
// The timer line itself is the shared `hudTimer` that `siege/SiegeHud.tsx` draws.

import { type CSSProperties, useSyncExternalStore } from 'react';
import { argbCss } from '../../../render/overlay/GlyphAtlas';
import { FONT, type FontSpec, fontStyle } from '../../../ui/text/fonts';
import { guildLogoSize } from '../../guild/logos';
import { type BoardRow, type BoardText, type HudBoard, KILL_CIPHERS } from './hudModel';

/** The font of the other guilds' rows: `gulim9_2`, 9 pt bold with an outline of 1 (a local spec; the table has no slot for it). */
export const OTHER_FONT: FontSpec = { px: 9, outlinePx: 1, outlineColor: 'rgb(0,0,0)', bold: true };

/** The model of the board as React state, with the animation counters. */
export class SecretRoomHudState {
  #model: HudBoard | null = null;
  #json = '';
  #version = 0;
  readonly #listeners = new Set<() => void>();
  /** The own kills as the digits show them (`m_stKillCountCiphers`, "000" at the start and at the entrance menu). */
  #shownKills = 0;
  /** Counters of the animations: a new value replays it (the React key). */
  #popVersion = 0;
  #flashVersion = 0;
  /** The first digit that pops (0 .. 2), -1 for none. */
  #popFrom = -1;
  /** Where a logo picture comes from: the module installs it. */
  logoUrl: (logo: number) => string | null = () => null;

  get model(): HudBoard | null {
    return this.#model;
  }
  get popVersion(): number {
    return this.#popVersion;
  }
  get flashVersion(): number {
    return this.#flashVersion;
  }
  get popFrom(): number {
    return this.#popFrom;
  }
  readonly subscribe = (fn: () => void): (() => void) => {
    this.#listeners.add(fn);
    return () => {
      this.#listeners.delete(fn);
    };
  };
  readonly getVersion = (): number => this.#version;

  /** Replaces the model; a model equal to the current one changes nothing (the logos are part of the check through `bump`). */
  set(model: HudBoard | null): void {
    const json = JSON.stringify(model);
    if (json === this.#json) return;
    this.#json = json;
    this.#model = model;
    const own = model?.rows.find((r) => r.own && r.count !== null);
    if (own && own.kills !== this.#shownKills) {
      this.#shownKills = own.kills;
      const digits = String(Math.max(0, Math.trunc(own.kills))).padStart(KILL_CIPHERS, '0');
      this.#popFrom = digits.split('').findIndex((c) => c > '0');
      this.#popVersion += 1;
      this.#flashVersion += 1;
    }
    this.#bump();
  }

  /** `MMI_SECRET_ENTRANCE`: the shown digits go back to "000". */
  resetDigits(): void {
    this.#shownKills = 0;
  }

  /** A re-render without a new model (a logo arrived). */
  bump(): void {
    this.#bump();
  }

  #bump(): void {
    this.#version += 1;
    for (const fn of [...this.#listeners]) fn();
  }
}

const STYLE = `
@keyframes sr-pop { from { transform: scale(2.5); opacity: 1; } to { transform: scale(1); opacity: 0.608; } }
@keyframes sr-flash { 0% { opacity: 0; } 50% { opacity: 1; } 100% { opacity: 0; } }
@keyframes sr-lamp { from { opacity: 1; } to { opacity: 0; } }
`;

const font = (own: boolean): CSSProperties => fontStyle(own ? FONT.secretRoom : OTHER_FONT);

function Text({
  t,
  own,
  extra,
}: {
  t: BoardText;
  own: boolean;
  extra?: CSSProperties | undefined;
}) {
  return (
    <div
      style={{
        position: 'absolute',
        left: t.x,
        top: t.y,
        whiteSpace: 'pre',
        color: argbCss(t.color),
        ...font(own),
        ...extra,
      }}
    >
      {t.text}
    </div>
  );
}

/** The four edges of an outline (`DrawOutLineLamp` / `DrawOutLineFlash`): gradients from transparent outside to the colour at the row. */
function Outline({
  x,
  y,
  w,
  h,
  thick,
  rgb,
  alpha,
  style,
  testId,
}: {
  x: number;
  y: number;
  w: number;
  h: number;
  thick: number;
  rgb: string;
  alpha: number;
  style: CSSProperties;
  testId: string;
}) {
  const c = `rgba(${rgb}, ${String(alpha)})`;
  const t0 = `rgba(${rgb}, 0)`;
  const half = Math.trunc(thick / 2);
  const edge = (css: CSSProperties): CSSProperties => ({ position: 'absolute', ...css });
  return (
    <div style={{ position: 'absolute', left: 0, top: 0, ...style }} data-testid={testId}>
      <div
        style={edge({
          left: x - thick - 1,
          top: y - half,
          width: thick + 1,
          height: h + 2 * half,
          background: `linear-gradient(to right, ${t0}, ${c})`,
        })}
      />
      <div
        style={edge({
          left: x,
          top: y - thick - 1,
          width: w,
          height: thick + 1,
          background: `linear-gradient(to bottom, ${t0}, ${c})`,
        })}
      />
      <div
        style={edge({
          left: x + w,
          top: y - half,
          width: thick,
          height: h + 2 * half,
          background: `linear-gradient(to left, ${t0}, ${c})`,
        })}
      />
      <div
        style={edge({
          left: x,
          top: y + h,
          width: w,
          height: thick,
          background: `linear-gradient(to top, ${t0}, ${c})`,
        })}
      />
    </div>
  );
}

const rgbOf = (rgb: number): string =>
  `${String((rgb >> 16) & 255)}, ${String((rgb >> 8) & 255)}, ${String(rgb & 255)}`;

function Row({ row, state }: { row: BoardRow; state: SecretRoomHudState }) {
  const r = row.rect;
  const logoUrl = row.logo ? state.logoUrl(row.logo.logo) : null;
  return (
    <div
      data-testid={`sr-hud-row-${String(row.index)}`}
      data-own={row.own ? '1' : '0'}
      data-war={row.war}
    >
      <div
        style={{
          position: 'absolute',
          left: r.x,
          top: r.y,
          width: r.w,
          height: r.h,
          background: argbCss(row.fill),
        }}
      />
      <Text t={row.rank} own={row.own} />
      {row.logo && logoUrl && (
        <img
          src={logoUrl}
          alt=""
          style={{
            position: 'absolute',
            left: row.logo.x,
            top: row.logo.y,
            width: guildLogoSize(row.logo.logo) * row.logo.scale,
            height: guildLogoSize(row.logo.logo) * row.logo.scale,
          }}
        />
      )}
      <Text t={row.name} own={row.own} />
      <Text t={row.sep} own={row.own} />
      {row.state && <Text t={row.state} own={row.own} />}
      {row.count && (
        <div data-testid={`sr-hud-count-${String(row.index)}`}>
          {row.count.digits.map((d, i) => {
            const pops =
              row.own &&
              state.popFrom >= 0 &&
              i >= state.popFrom &&
              row.count?.digits.length === KILL_CIPHERS;
            return (
              <Text
                key={pops ? `p${String(state.popVersion)}-${String(i)}` : `d${String(i)}`}
                t={d}
                own={row.own}
                extra={
                  pops
                    ? { transformOrigin: '6px 8px', animation: 'sr-pop 0.1667s linear 1' }
                    : undefined
                }
              />
            );
          })}
          <Text t={row.count.slash} own={row.own} />
          <Text t={row.count.total} own={row.own} />
        </div>
      )}
      {row.lamp && (
        <Outline
          x={row.lamp.x}
          y={row.lamp.y}
          w={row.lamp.w}
          h={row.lamp.h}
          thick={row.lamp.thick}
          rgb={rgbOf(row.lamp.rgb)}
          alpha={0.745}
          style={{
            animation: `sr-lamp ${String(row.lamp.halfFrames / 60)}s linear infinite alternate`,
          }}
          testId={`sr-hud-lamp-${String(row.index)}`}
        />
      )}
      {row.own && state.flashVersion > 0 && (
        <Outline
          key={`flash${String(state.flashVersion)}`}
          x={r.x}
          y={r.y}
          w={r.w}
          h={r.h}
          thick={6}
          rgb="255, 255, 255"
          alpha={0.745}
          style={{ opacity: 0, animation: 'sr-flash 0.2333s linear 1' }}
          testId="sr-hud-flash"
        />
      )}
    </div>
  );
}

export interface SecretRoomHudProps {
  state: SecretRoomHudState;
  /** The virtual viewport of the UI. */
  viewport: { width: number; height: number };
  /** The UI scale (`UiScaler`). */
  scale: number;
}

export function SecretRoomHud({ state, viewport, scale }: SecretRoomHudProps) {
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
    <div style={container} data-testid="sr-hud-board">
      <style>{STYLE}</style>
      {model.rows.map((row) => (
        <Row key={row.index} row={row} state={state} />
      ))}
    </div>
  );
}
