// The hover tooltip of the item cells (Plan/25 section 4.4): the original game tooltip, composed for THIS character as the viewer
// (`tooltip.ts`), drawn with the game's rich text, and an admin footer with the serial, the slot, the flags and the keep time.
import {
  createContext,
  useContext,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';
import type { ItemElem } from '../../../gen/proto/flyff/v1/item_types_pb';
import { FONT } from '../../../ui/text/fonts';
import { layoutRich } from '../../../ui/text/layout';
import { RichText } from '../../../ui/text/RichText';
import { TOOLTIP_WRAP } from '../../../ui/tooltip/placement';
import { useGrantData } from '../../grant';
import { FLAG_BINDS, FLAG_EXPIRED, FLAG_USING } from './containers';
import { formatDateTime } from './format';
import { usePlayerGame, useSetItemAvail } from './gamedata';
import { T } from './strings';
import { composeTip, type TipContent, type TipEquipment, type TipViewer } from './tooltip';

export interface TipTarget {
  readonly elem: ItemElem;
  /** The cell is a worn piece. */
  readonly worn: boolean;
  readonly slot: number;
  readonly anchor: DOMRect;
}

interface TipApi {
  show(target: TipTarget): void;
  hide(): void;
}

const TipContext = createContext<TipApi | null>(null);

/** The tooltip host of the enclosing provider (`null` outside one: cells then show no tooltip). */
export function useTip(): TipApi | null {
  return useContext(TipContext);
}

/** The flags of an item as the admin footer writes them. */
export function flagText(flag: number): string {
  const parts: string[] = [];
  if ((flag & FLAG_BINDS) !== 0) parts.push(T.items.footer.bound);
  if ((flag & FLAG_EXPIRED) !== 0) parts.push(T.items.footer.expired);
  if ((flag & FLAG_USING) !== 0) parts.push(T.items.footer.using);
  return parts.length === 0 ? '-' : parts.join(', ');
}

/** The footer lines of the admin: serial, slot, flags, expiry, guild. */
export function footerLines(elem: ItemElem, slot: number): string[] {
  const lines = [
    `${T.items.footer.serial} ${elem.serial.toString()}   ${T.items.footer.slot} ${String(slot)}`,
    `${T.items.footer.flags} ${flagText(elem.flag)}`,
  ];
  const keep = Number(elem.keepTimeAt);
  if (keep > 0) lines.push(`${T.items.footer.expires} ${formatDateTime(keep)}`);
  if (elem.idGuild !== 0) lines.push(`${T.items.footer.guild} ${String(elem.idGuild)}`);
  return lines;
}

export interface TipProviderProps {
  /** The character whose items these are: level, job and sex colour the requirement lines. */
  readonly viewer: TipViewer;
  readonly equipment: TipEquipment;
  readonly children: ReactNode;
}

export function TipProvider({ viewer, equipment, children }: TipProviderProps) {
  const [target, setTarget] = useState<(TipTarget & { at: number }) | null>(null);
  const api = useMemo<TipApi>(
    () => ({
      show: (t) => {
        setTarget({ ...t, at: Date.now() });
      },
      hide: () => {
        setTarget(null);
      },
    }),
    [],
  );
  return (
    <TipContext.Provider value={api}>
      {children}
      {target ? <TipBox target={target} viewer={viewer} equipment={equipment} /> : null}
    </TipContext.Provider>
  );
}

/** The composed tooltip of a cell as React state (the hover box and the item dialog share it). */
export function useTipContent(args: {
  readonly elem: ItemElem;
  readonly worn: boolean;
  readonly at: number;
  readonly viewer: TipViewer;
  readonly equipment: TipEquipment;
}): {
  readonly state: 'loading' | 'unknown' | 'ready';
  readonly content: TipContent | null;
  readonly rulesReady: boolean;
} {
  const game = usePlayerGame();
  const rulesPhase = useGrantData((s) => s.rulesPhase);
  const setItemAvail = useSetItemAvail();
  const { elem, worn, at, viewer, equipment } = args;
  const content = useMemo(() => {
    if (!game.data) return null;
    return composeTip({
      env: game.data.env,
      rules: rulesPhase === 'ready' ? game.data.rules() : null,
      setItemAvail,
      elem,
      viewer,
      equipment,
      worn,
      nowMs: at,
    });
  }, [game.data, rulesPhase, setItemAvail, elem, worn, at, viewer, equipment]);
  return {
    state: !game.data ? 'loading' : content ? 'ready' : 'unknown',
    content,
    rulesReady: rulesPhase === 'ready',
  };
}

/** The lines of a composed tooltip drawn with the game's rich text (the notes when the data or the rules are missing). */
export function TipBody({
  state,
  content,
  rulesReady,
}: {
  readonly state: 'loading' | 'unknown' | 'ready';
  readonly content: TipContent | null;
  readonly rulesReady: boolean;
}) {
  const lines = useMemo(
    () => (content ? layoutRich(content.runs, TOOLTIP_WRAP, FONT.text, true) : []),
    [content],
  );
  if (state === 'loading') return <div className="plr-tip__note">{T.items.tooltipLoading}</div>;
  if (!content) return <div className="plr-tip__note">{T.items.tooltipUnknown}</div>;
  return (
    <>
      <RichText lines={lines} font={FONT.text} lineSpace={2} />
      {content.ultimate?.gems ? (
        <div className="plr-tip__sockets">
          {content.ultimate.gems.map((g, i) => (
            <span key={i} className={g ? 'plr-tip__socket is-full' : 'plr-tip__socket'} />
          ))}
        </div>
      ) : null}
      {!rulesReady ? <div className="plr-tip__note">{T.items.tooltipNoRules}</div> : null}
    </>
  );
}

function TipBox({
  target,
  viewer,
  equipment,
}: {
  readonly target: TipTarget & { readonly at: number };
  readonly viewer: TipViewer;
  readonly equipment: TipEquipment;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const tip = useTipContent({
    elem: target.elem,
    worn: target.worn,
    at: target.at,
    viewer,
    equipment,
  });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const box = el.getBoundingClientRect();
    const a = target.anchor;
    const margin = 8;
    let left = a.right + margin;
    if (left + box.width > window.innerWidth - margin)
      left = Math.max(margin, a.left - box.width - margin);
    let top = a.top;
    if (top + box.height > window.innerHeight - margin)
      top = Math.max(margin, window.innerHeight - box.height - margin);
    setPos({ left, top });
  }, [target, tip.state, tip.content]);

  return createPortal(
    <div
      ref={ref}
      className="plr-tip"
      role="tooltip"
      data-testid="item-tooltip"
      style={{
        left: pos?.left ?? -9999,
        top: pos?.top ?? -9999,
        width: TOOLTIP_WRAP + 12,
      }}
    >
      <TipBody state={tip.state} content={tip.content} rulesReady={tip.rulesReady} />
      <div className="plr-tip__admin">
        {footerLines(target.elem, target.slot).map((l) => (
          <div key={l}>{l}</div>
        ))}
      </div>
    </div>,
    document.body,
  );
}
