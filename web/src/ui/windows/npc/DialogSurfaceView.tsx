// The pixels of the dialog surface: the portrait, the page text, the key buttons, the answer links, the clickable words and the
// quest lists, all at the positions the window computed (client coordinates of the dialog window).

import { useSyncExternalStore, type CSSProperties } from 'react';
import { useSprite } from '../../controls/useSprite';
import { rectHeight, rectWidth, type Rect } from '../../layout/geometry';
import { FONT, fontStyle, lineHeight } from '../../text/fonts';
import { cssColor } from '../../text/markup';
import { RichText } from '../../text/RichText';
import type { DialogSurface, SurfaceWord } from './DialogSurface';

/** The colours of `CWndDialog::OnDraw`: idle `0xff505050`, hovered `0xff1010ff`, selected key `255, 128, 0, 255`. */
export const WORD_COLOR = 0xff505050;
export const WORD_HOVER_COLOR = 0xff1010ff;
export const KEY_SELECTED_COLOR = 0xff8000ff;
export const QUEST_SELECT_COLOR = 0xffff0000;

/** A word drawn twice (x and x + 1: faux bold), underlined while hovered. */
function Word({ w, color, testid }: { w: SurfaceWord; color: number; testid: string }) {
  const cell = lineHeight(FONT.text);
  const style: CSSProperties = {
    position: 'absolute',
    left: w.rect.l,
    top: w.rect.t,
    color: cssColor(color),
    whiteSpace: 'pre',
    textShadow: '1px 0 0 currentColor',
    ...fontStyle(FONT.text),
  };
  if (w.hover) {
    // `RenderLine` under the word, one cell below its top
    style.backgroundImage = 'linear-gradient(currentColor, currentColor)';
    style.backgroundRepeat = 'no-repeat';
    style.backgroundSize = '100% 1px';
    style.backgroundPosition = `0 ${String(cell)}px`;
  }
  return (
    <div
      data-testid={testid}
      data-hover={w.hover ? 'true' : 'false'}
      data-selected={w.selected === true ? 'true' : 'false'}
      style={style}
    >
      {w.word}
    </div>
  );
}

function Icon({ control, path, rect }: { control: DialogSurface; path: string; rect: Rect }) {
  const sprite = useSprite(control.env, path);
  if (typeof sprite !== 'object') return null;
  return (
    <div
      style={{
        position: 'absolute',
        left: rect.l,
        top: rect.t,
        width: sprite.width,
        height: sprite.height,
        backgroundImage: `url("${sprite.url}")`,
        backgroundRepeat: 'no-repeat',
      }}
    />
  );
}

function Portrait({
  control,
  path,
  at,
}: {
  control: DialogSurface;
  path: string;
  at: { x: number; y: number };
}) {
  const sprite = useSprite(control.env, path);
  if (typeof sprite !== 'object') return null;
  return (
    <div
      data-testid="dialog-portrait"
      style={{
        position: 'absolute',
        left: at.x,
        top: at.y,
        width: sprite.width,
        height: sprite.height,
        backgroundImage: `url("${sprite.url}")`,
        backgroundRepeat: 'no-repeat',
      }}
    />
  );
}

export function DialogSurfaceView({ control }: { control: DialogSurface }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const w = control.rectWindow;
  return (
    <div
      data-testid="dialog-surface"
      style={{ position: 'relative', width: rectWidth(w), height: rectHeight(w) }}
    >
      {s.portrait && <Portrait control={control} path={s.portrait.path} at={s.portrait.at} />}
      {s.body && (
        <div
          data-testid="dialog-text"
          style={{
            position: 'absolute',
            left: s.body.clip.l,
            top: s.body.clip.t,
            width: rectWidth(s.body.clip),
            height: rectHeight(s.body.clip),
            overflow: 'hidden',
          }}
        >
          <div
            style={{
              position: 'absolute',
              left: s.body.at.x - s.body.clip.l,
              top: s.body.at.y - s.body.clip.t,
            }}
          >
            <RichText lines={s.body.lines} font={FONT.text} lineSpace={6} />
          </div>
        </div>
      )}
      {s.empty && (
        <div
          data-testid="dialog-empty"
          style={{
            position: 'absolute',
            left: s.empty.at.x,
            top: s.empty.at.y,
            color: '#000',
            whiteSpace: 'pre',
            ...fontStyle(FONT.text),
          }}
        >
          {s.empty.text}
        </div>
      )}
      {s.links.map((l, i) =>
        l.hover ? (
          <Word key={`l${String(i)}`} w={l} color={WORD_HOVER_COLOR} testid="dialog-link" />
        ) : null,
      )}
      {s.quests && (
        <div data-testid="dialog-quests">
          {s.quests.rows.map((r, i) => (
            <div key={i}>
              <Icon control={control} path={r.icon} rect={r.rect} />
              <div
                data-hover={r.hover ? 'true' : 'false'}
                style={{
                  position: 'absolute',
                  left: r.rect.l + 16,
                  top: r.rect.t,
                  ...(r.hover ? { color: cssColor(QUEST_SELECT_COLOR) } : {}),
                }}
              >
                <RichText lines={r.lines} font={FONT.text} lineSpace={3} />
              </div>
            </div>
          ))}
        </div>
      )}
      {s.answers.map((a, i) => (
        <Word
          key={`a${String(i)}`}
          w={a}
          color={a.hover ? WORD_HOVER_COLOR : WORD_COLOR}
          testid="dialog-answer"
        />
      ))}
      {s.keys.map((k, i) => (
        <Word
          key={`k${String(i)}`}
          w={k}
          color={k.hover ? WORD_HOVER_COLOR : k.selected === true ? KEY_SELECTED_COLOR : WORD_COLOR}
          testid="dialog-key"
        />
      ))}
    </div>
  );
}
