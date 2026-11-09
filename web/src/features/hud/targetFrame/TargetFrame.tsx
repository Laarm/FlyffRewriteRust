// The DOM face of the target frame (Plan/45 §7.3; WP-CFEAT-09): the 200 px gauge at the top centre of the world view with the
// name, level and HP lines of `model.ts`, painted under the windows and over the world canvas in the virtual pixels of 42's UI.
// The gauge is the textured three-slice bar of the status window (`GauEmptyNormal.bmp`), the job and element icons are
// sprites of the `Icon_JobClassTextures.inc` / `Icon_MonElemantkind.inc` atlases, the monster tooltip opens over the 200 x 30
// strip above the gauge.

import { useEffect, useState, useSyncExternalStore, type CSSProperties } from 'react';
import { argbCss, outlineOffsets } from '../../../render/overlay/GlyphAtlas';
import { GaugeBar } from '../../../ui/controls/GaugeView';
import { useSprite } from '../../../ui/controls/useSprite';
import { MISSING } from '../../../ui/skin/images';
import { spriteRect } from '../../../ui/skin/atlas';
import type { UiApi } from '../../../ui';
import {
  GAUGE_BACKGROUND,
  GAUGE_FILL,
  GAUGE_WIDTH,
  COLOR_TOOLTIP_NAME,
  type FrameText,
  type TargetFrameModel,
} from './model';

const FAMILY = '"Flyff DejaVu Sans", Verdana, sans-serif';
/** `m_pFontWorld`: 12 px with a 1 px outline. */
const FONT_PX = 12;
export const JOB_ICON_ATLAS = 'icon/icon_jobclasstextures.inc';
export const ELEMENT_ICON_ATLAS = 'icon/icon_monelemantkind.inc';
const GAUGE_TEXTURE = 'GauEmptyNormal.bmp';

/** The model of the frame as React state: set by the module, read by the component. */
export class TargetFrameState {
  #model: TargetFrameModel | null = null;
  #version = 0;
  readonly #listeners = new Set<() => void>();

  get model(): TargetFrameModel | null {
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
  set(model: TargetFrameModel | null): void {
    if (sameModel(this.#model, model)) return;
    this.#model = model;
    this.#version += 1;
    for (const fn of [...this.#listeners]) fn();
  }
}

/** Deep equality of two models (they are small plain records). */
export function sameModel(a: TargetFrameModel | null, b: TargetFrameModel | null): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return JSON.stringify(a) === JSON.stringify(b);
}

function outline(rgb: string): string {
  return outlineOffsets(1)
    .map(([dx, dy]) => `${String(dx)}px ${String(dy)}px 0 ${rgb}`)
    .join(',');
}

function textStyle(t: FrameText): CSSProperties {
  return {
    position: 'absolute',
    left: t.x,
    top: t.y,
    transform: t.centered === true ? 'translateX(-50%)' : undefined,
    whiteSpace: 'pre',
    fontSize: FONT_PX,
    lineHeight: `${String(FONT_PX + 4)}px`,
    color: argbCss(t.color),
    textShadow: outline('rgba(0,0,0,0.85)'),
  };
}

interface AtlasCss {
  readonly url: string;
  readonly texW: number;
  readonly texH: number;
  readonly rect: readonly [number, number, number, number];
}

/** The texture URL and pixel rectangle of an atlas sprite (`undefined` while loading or when it does not exist). */
function useAtlasSprite(ui: UiApi | null, inc: string, index: number | null): AtlasCss | undefined {
  const [res, setRes] = useState<{ key: string; css: AtlasCss | undefined }>({
    key: '',
    css: undefined,
  });
  const key = ui && index !== null ? `${inc}:${String(index)}` : '';
  useEffect(() => {
    if (!ui || index === null) return undefined;
    const dto = ui.atlases.info(inc);
    const sprite = dto?.sprites[index];
    if (!dto || !sprite || dto.texture === '') return undefined;
    let live = true;
    void ui.images.get(dto.texture, 'exact').then((img) => {
      if (!live) return;
      const url = img === MISSING ? null : ui.images.url(dto.texture, 'exact');
      setRes({
        key,
        css:
          img === MISSING || url === null
            ? undefined
            : {
                url,
                texW: img.width,
                texH: img.height,
                rect: spriteRect(sprite, img.width, img.height),
              },
      });
    });
    return () => {
      live = false;
    };
  }, [ui, inc, index, key]);
  return res.key === key ? res.css : undefined;
}

function AtlasIcon({ css, x, y, scale }: { css: AtlasCss; x: number; y: number; scale: number }) {
  const [sx, sy, w, h] = css.rect;
  const style: CSSProperties = {
    position: 'absolute',
    left: x,
    top: y,
    width: w * scale,
    height: h * scale,
    backgroundImage: `url("${css.url}")`,
    backgroundRepeat: 'no-repeat',
    backgroundSize: `${String(css.texW * scale)}px ${String(css.texH * scale)}px`,
    backgroundPosition: `${String(-sx * scale)}px ${String(-sy * scale)}px`,
  };
  return <div style={style} data-testid="target-icon" />;
}

export interface TargetFrameProps {
  readonly state: TargetFrameState;
  /** The virtual viewport of the UI. */
  readonly viewport: { width: number; height: number };
  readonly scale: number;
  /** The UI (images, atlases); `null` renders the gauge with plain boxes. */
  readonly ui: UiApi | null;
}

export function TargetFrame({ state, viewport, scale, ui }: TargetFrameProps) {
  useSyncExternalStore(state.subscribe, state.getVersion);
  const m = state.model;
  const [hover, setHover] = useState(false);
  const bar = useSprite(ui?.wm.env ?? NO_ENV, ui ? GAUGE_TEXTURE : null);
  const job = useAtlasSprite(ui, JOB_ICON_ATLAS, m?.jobIcon ?? null);
  const elem = useAtlasSprite(ui, ELEMENT_ICON_ATLAS, m?.elementIcon ?? null);
  if (!m) return null;
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
  const g = m.gauge;
  const gaugeBox: CSSProperties = {
    position: 'absolute',
    left: g.x,
    top: g.y,
    width: GAUGE_WIDTH,
    height: 16,
  };
  const sprite = typeof bar === 'object' ? bar : null;
  return (
    <div style={container} data-testid="target-frame">
      <div style={{ ...gaugeBox, height: sprite?.height ?? 16 }} data-testid="target-gauge">
        {sprite ? (
          <>
            <GaugeBar sprite={sprite} width={GAUGE_WIDTH} alpha={(GAUGE_BACKGROUND >>> 24) / 255} />
            <GaugeBar
              sprite={sprite}
              width={g.fill}
              alpha={(GAUGE_FILL >>> 24) / 255}
              tint="rgb(255, 15, 15)"
            />
          </>
        ) : (
          <>
            <div style={plainBar(GAUGE_WIDTH, argbCss(GAUGE_BACKGROUND))} />
            <div style={plainBar(g.fill, argbCss(GAUGE_FILL))} />
          </>
        )}
      </div>
      {m.jobIcon !== null && job && <AtlasIcon css={job} x={g.x} y={g.y - 22} scale={1.2} />}
      {m.elementIcon !== null && elem && <AtlasIcon css={elem} x={g.x} y={g.y - 25} scale={1.5} />}
      <div style={textStyle(m.name)} data-testid="target-name">
        {m.name.text}
      </div>
      {m.level.text !== '' && (
        <div style={textStyle(m.level)} data-testid="target-level">
          {m.level.text}
        </div>
      )}
      {m.hp && (
        <div style={textStyle(m.hp)} data-testid="target-hp">
          {m.hp.text}
        </div>
      )}
      {m.gmLine && (
        <div
          style={textStyle({ text: m.gmLine, x: g.x, y: g.y + 12, color: 0xffffffff })}
          data-testid="target-gm"
        >
          {m.gmLine}
        </div>
      )}
      {m.tooltip && (
        <div
          style={{
            position: 'absolute',
            left: g.x,
            top: 0,
            width: GAUGE_WIDTH,
            height: 30,
            pointerEvents: 'auto',
          }}
          data-testid="target-strip"
          onMouseEnter={() => {
            setHover(true);
          }}
          onMouseLeave={() => {
            setHover(false);
          }}
        >
          {hover && (
            <div
              style={{
                position: 'absolute',
                left: 0,
                top: g.y + 24,
                background: 'rgba(0,0,0,0.78)',
                border: '1px solid rgba(255,255,255,0.55)',
                padding: '3px 6px',
                fontSize: FONT_PX,
                lineHeight: `${String(FONT_PX + 4)}px`,
                color: '#fff',
                whiteSpace: 'pre',
              }}
              data-testid="target-tooltip"
            >
              <div style={{ color: argbCss(COLOR_TOOLTIP_NAME), fontWeight: 'bold' }}>
                {m.tooltip.name}
              </div>
              {m.tooltip.lines.map((l) => (
                <div key={l}>{l}</div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

const plainBar = (w: number, color: string): CSSProperties => ({
  position: 'absolute',
  left: 0,
  top: 0,
  width: w,
  height: 16,
  borderRadius: 4,
  background: color,
});

/** `useSprite` needs an environment; without a UI there is no sprite to load (`null` path). */
const NO_ENV = {} as never;
