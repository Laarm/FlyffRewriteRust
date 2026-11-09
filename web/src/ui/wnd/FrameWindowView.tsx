// The React view of one frame window (Plan/42 §4.8): the baked skin canvas, the title, the title bar, and the
// client area with the controls in creation order (later controls paint and hit-test on top). The view only
// renders; the controller (`WndNeuz`) owns all state and every change goes through observables so a setter that
// writes an unchanged value never causes a commit.

import { memo, useEffect, useRef, useSyncExternalStore, type CSSProperties } from 'react';
import { rectHeight, rectWidth } from '../layout/geometry';
import { WTBID_CLOSE, WTBID_HELP, WTBID_MAX, WTBID_MIN } from '../layout/styles';
import { FONT, fontCell, fontStyle } from '../text/fonts';
import type { Control } from './Control';
import type { CustomSlot } from './WndNeuz';
import type { TitleBar, TitleButton } from './TitleBar';
import { TITLE_SPRITE } from './TitleBar';
import type { WndNeuz } from './WndNeuz';
import '../styles/ui.css';

export interface FrameWindowViewProps {
  frame: WndNeuz;
  /** `z-index` = index in the window order * 10. */
  z: number;
  /** `WindowAlpha` / 255 is applied to the skin canvas only (text and controls stay opaque). */
  alpha?: number;
  /** The frame has the focus (a `data-focused` attribute for tests and styling). */
  focused?: boolean;
}

export const FrameWindowView = memo(function FrameWindowView({
  frame,
  z,
  alpha = 255,
  focused = false,
}: FrameWindowViewProps) {
  const s = useSyncExternalStore(frame.view.subscribe, frame.view.get);
  const w = rectWidth(s.rect);
  const h = rectHeight(s.rect);
  const client = frame.rectClient;
  const cw = rectWidth(client);
  const ch = rectHeight(client);
  const titleTop = s.untextured ? 7 : 4;
  const titleLeft = s.untextured ? 17 : 10;
  return (
    <div
      className="wnd"
      data-app={frame.appId}
      data-uid={frame.uid}
      data-visible={s.visible}
      data-focused={focused}
      role="dialog"
      aria-label={s.title === '' ? undefined : s.title}
      style={{
        left: s.rect.l,
        top: s.rect.t,
        width: w,
        height: h,
        zIndex: z,
        ...(frame.domPassthrough ? { pointerEvents: 'none' as const } : {}),
      }}
    >
      <SkinCanvas skin={s.skin} version={s.skinVersion} alpha={s.skinAlpha ?? alpha} />
      {s.title !== '' && s.titleBar !== null && (
        <div
          className="wnd-title"
          style={{
            left: titleLeft,
            top: titleTop,
            ...fontStyle(FONT.title),
            lineHeight: `${String(fontCell(FONT.title.px))}px`,
            ...(s.skinAlpha != null
              ? { opacity: Math.max(0, Math.min(255, s.skinAlpha)) / 255 }
              : {}),
          }}
        >
          {s.title}
        </div>
      )}
      {s.titleBar && <TitleBarView bar={s.titleBar} />}
      <div
        className="wnd-client"
        style={{ left: s.clientOffset.x, top: s.clientOffset.y, width: cw, height: ch }}
      >
        {s.controls.map((c) => (
          <ControlHost key={c.uid} control={c} />
        ))}
        {s.customs.map((slot) => (
          <CustomHost key={slot.name} slot={slot} />
        ))}
      </div>
    </div>
  );
});

/** Paints the baked RGBA buffer into a canvas whenever a new skin was published. */
function SkinCanvas({
  skin,
  version,
  alpha,
}: {
  skin: { width: number; height: number; data: Uint8ClampedArray } | null;
  version: number;
  alpha: number;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = ref.current;
    if (!canvas || !skin || skin.width <= 0 || skin.height <= 0) return;
    canvas.width = skin.width;
    canvas.height = skin.height;
    const ctx = canvas.getContext('2d');
    if (!ctx) return; // no canvas support (tests): the baked buffer stays inspectable on the controller
    ctx.putImageData(
      new ImageData(skin.data as Uint8ClampedArray<ArrayBuffer>, skin.width, skin.height),
      0,
      0,
    );
  }, [skin, version]);
  if (!skin || skin.width <= 0 || skin.height <= 0) return null;
  const style: CSSProperties = {
    width: skin.width,
    height: skin.height,
    opacity: Math.max(0, Math.min(255, alpha)) / 255,
  };
  return (
    <canvas ref={ref} className="wnd-skin" width={skin.width} height={skin.height} style={style} />
  );
}

/**
 * The positioned host of one control; the kind's view component renders the interior. Exported for the controls
 * that own children (a text control's scroll bar, a tab control's pages, a menu's items): their views mount the
 * child's host inside their own, so a child's rect is relative to the owner (docking children: its window rect).
 */
export const ControlHost = memo(function ControlHost({ control }: { control: Control }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const View = control.view;
  return (
    <div
      className="ctrl"
      data-wtype={control.kind}
      data-name={control.name}
      data-id={control.id}
      data-visible={s.visible}
      style={{
        left: s.rect.l,
        top: s.rect.t,
        width: Math.max(0, rectWidth(s.rect)),
        height: Math.max(0, rectHeight(s.rect)),
      }}
    >
      {View ? <View control={control} /> : null}
    </div>
  );
});

/**
 * A window that lives inside a control (a tab page: a `WndNeuz` created with `WBS_CHILD`): the same client area as a
 * frame (controls, custom canvases) at its rect in the owner's client coordinates, without title or title bar. A page that
 * has a skin of its own (the character page sits on the picture `WndNewCharacter01.tga`: `SetTexture` of a child window)
 * draws it under its controls; pages that bake nothing (`WBS_NODRAWFRAME`) draw none.
 */
export const PageHost = memo(function PageHost({ page }: { page: WndNeuz }) {
  const s = useSyncExternalStore(page.view.subscribe, page.view.get);
  const client = page.rectClient;
  return (
    <div
      className="wnd-page"
      data-app={page.appId}
      data-visible={s.visible}
      style={{
        left: s.rect.l,
        top: s.rect.t,
        width: rectWidth(s.rect),
        height: rectHeight(s.rect),
      }}
    >
      <SkinCanvas skin={s.skin} version={s.skinVersion} alpha={255} />
      <div
        className="wnd-client"
        style={{
          left: s.clientOffset.x,
          top: s.clientOffset.y,
          width: rectWidth(client),
          height: rectHeight(client),
        }}
      >
        {s.controls.map((c) => (
          <ControlHost key={c.uid} control={c} />
        ))}
        {s.customs.map((slot) => (
          <CustomHost key={slot.name} slot={slot} />
        ))}
      </div>
    </div>
  );
});

/** Mounts the canvas of a `WTYPE_CUSTOM` placeholder at its rect. */
function CustomHost({ slot }: { slot: CustomSlot }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const host = ref.current;
    if (!host) return;
    host.appendChild(slot.canvas);
    return () => {
      if (slot.canvas.parentNode === host) host.removeChild(slot.canvas);
    };
  }, [slot]);
  return (
    <div
      ref={ref}
      className="wnd-custom"
      data-custom={slot.name}
      style={{
        left: slot.rect.l,
        top: slot.rect.t,
        width: rectWidth(slot.rect),
        height: rectHeight(slot.rect),
      }}
    />
  );
}

/** The title bar (§5.11): buttons at a 16 px pitch inside a bar right-aligned in the frame. */
function TitleBarView({ bar }: { bar: TitleBar }) {
  const rect = useSyncExternalStore(bar.rectState.subscribe, bar.rectState.get);
  const visible = useSyncExternalStore(bar.visibleState.subscribe, bar.visibleState.get);
  if (!visible) return null;
  return (
    <div
      className="wnd-titlebar"
      data-titlebar="true"
      style={{ left: rect.l, top: rect.t, width: rectWidth(rect), height: rectHeight(rect) }}
    >
      {bar.buttons.map((b) => (
        <TitleButtonView key={b.spec.id} button={b} />
      ))}
    </div>
  );
}

/** Accessible names of the title-bar buttons. */
const TITLE_BUTTON_LABELS: Readonly<Record<number, string>> = {
  [WTBID_CLOSE]: 'Close',
  [WTBID_HELP]: 'Help',
  [WTBID_MIN]: 'Minimize',
  [WTBID_MAX]: 'Maximize',
};

function TitleButtonView({ button }: { button: TitleButton }) {
  const s = useSyncExternalStore(button.state.subscribe, button.state.get);
  const { spec } = button;
  const r = s.rect;
  const style: CSSProperties = {
    left: r.l,
    top: r.t,
    width: rectWidth(r),
    height: rectHeight(r),
  };
  if (spec.sprite) {
    const url = button.env.images.url(`theme/${spec.sprite}`, 'exact');
    if (url) {
      // frames of the 60 x 14 strip: 0 hover, 1 normal, 2 pushed, 3 disabled
      const frame = !s.enabled ? 3 : s.pushed ? 2 : s.hover ? 0 : 1;
      style.backgroundImage = `url("${url}")`;
      style.backgroundSize = `${String(TITLE_SPRITE.w * TITLE_SPRITE.frames)}px ${String(TITLE_SPRITE.h)}px`;
      style.backgroundPosition = `${String(-frame * TITLE_SPRITE.w)}px 0`;
    }
  }
  return (
    <div
      className="wnd-tbtn"
      role="button"
      aria-label={TITLE_BUTTON_LABELS[spec.id] ?? spec.caption}
      aria-disabled={!s.enabled}
      data-id={spec.id}
      data-sprite={spec.sprite !== null}
      data-hover={s.hover}
      data-pushed={s.pushed}
      style={style}
    >
      {spec.sprite === null ? spec.caption : null}
    </div>
  );
}
