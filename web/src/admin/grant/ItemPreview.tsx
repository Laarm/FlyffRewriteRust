// The game tooltip of the item being built, drawn with the game's rich text (`ui/text`): the same lines, colours and wrapping.
import { useEffect, useMemo, useState } from 'react';
import { FONT } from '../../ui/text/fonts';
import { layoutRich } from '../../ui/text/layout';
import { RichText } from '../../ui/text/RichText';
import { TOOLTIP_WRAP } from '../../ui/tooltip/placement';
import { composePreview, numbersFn } from './preview';
import { useGrantData } from './data';
import type { StoredItemDto } from '../gen/dto';
import { T } from './strings';

export interface PreviewProps {
  /** The validated item; `null` while the server has not answered or the item is invalid. */
  readonly item: StoredItemDto | null;
}

/** The ultimate-weapon band: the gem sockets along the bottom, as small chips with the gem name (the game draws the icons). */
export function Preview({ item }: PreviewProps) {
  const data = useGrantData((s) => s.data);
  const rulesPhase = useGrantData((s) => s.rulesPhase);
  // the clock of the keep-time line: read once and refreshed every 30 s, never during a render
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => {
      setNow(Date.now());
    }, 30_000);
    return () => {
      clearInterval(timer);
    };
  }, []);
  const content = useMemo(() => {
    if (!data || !item) return null;
    // `rulesPhase` is a dependency: the numbers appear when the wasm rules are built
    return composePreview(
      data.env,
      numbersFn(rulesPhase === 'ready' ? data.rules() : null),
      item,
      now,
    );
  }, [data, item, rulesPhase, now]);
  const lines = useMemo(
    () => (content ? layoutRich(content.runs, TOOLTIP_WRAP, FONT.text, true) : []),
    [content],
  );
  if (!item) return <div className="grant-tip grant-tip--empty">{T.preview.none}</div>;
  if (!content) return <div className="grant-tip grant-tip--empty">{T.preview.unknown}</div>;
  return (
    <div className="grant-tip-wrap">
      <div className="grant-tip" data-testid="grant-tooltip" style={{ width: TOOLTIP_WRAP + 12 }}>
        <RichText lines={lines} font={FONT.text} lineSpace={2} />
        {content.ultimate && (
          <div className="grant-tip__sockets" aria-label={T.preview.sockets}>
            {content.ultimate.gems?.map((g, i) => (
              <span key={i} className={g ? 'grant-tip__socket is-full' : 'grant-tip__socket'} />
            ))}
          </div>
        )}
      </div>
      {rulesPhase === 'loading' && <div className="grant-note">{T.preview.rulesLoading}</div>}
      {(rulesPhase === 'error' || rulesPhase === 'missing') && (
        <div className="grant-note grant-note--warn">{T.preview.rulesMissing}</div>
      )}
    </div>
  );
}
