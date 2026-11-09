// The read model of the character window (`CharacterView` of `ui/windows/char/ports.ts`; Plan/45 §20.1-§20.2, WP-CFEAT-11): the
// base data of `selfStore`, the derived numbers of `rules.derivedStats` (kept in `selfStore.derived` by the rules runtime, at
// most 10 Hz) and the preview of a pending stat allocation (`rules.statPreview`).

import type { ClientContext } from '../../../app/context';
import { JTYPE_BASE, JTYPE_HERO, JTYPE_MASTER } from '../../../gen/defs/job';
import {
  SM_ATTACK_UP,
  SM_ATTACK_UP1,
  atkExtrasOf,
  derivedForWindow,
  unflattenDerived,
} from './derived';
import { peekRulesRuntime } from '../../../game/rules';
import { derivedMatches, statInputOf } from '../../../game/rules/selfStats';
import type { DerivedStatsJs } from '../../../rules';
import type { SelfState } from '../../../stores/self';
import type { CharacterView, DerivedStats, StatDelta } from '../../../ui/windows/char/ports';
import { derivedView, type ViewSource } from '../../common/view';

const EMPTY_DERIVED: DerivedStats = { atk: 0, def: 0, critical: 0, attackSpeed: 0 };

/**
 * How long the window keeps showing the previous numbers while the derived record is behind the base stats it would show
 * (ms). The rules call of a base-stat change runs one loop iteration later (R slot, every 100 ms at most): without the hold
 * the window showed the new base stats and points next to the old derived numbers and a total that did not match its base
 * for that moment (the screenshot of the M3 demo after the stat allocation: ATK 28 and STR 15 with 17 base points).
 */
export const DERIVED_HOLD_MS = 600;

/** The "stat pair": `GetStr()` with buffs and equipment, and the base `m_nStr`. */
const pair = (total: number | undefined, base: number): { total: number; base: number } => ({
  total: total ?? base,
  base,
});

/** The sm-mode timer of a kind is positive (`IsSMMode`). */
const smActive = (s: SelfState, kind: number): boolean =>
  s.smTimers.some((t) => t.kind === kind && t.endUnix > 0);

/** What decides the identity of a view: everything the window prints (the `preview` closure is stable). */
function keyOf(v: CharacterView): string {
  return JSON.stringify({ ...v, preview: undefined });
}

export function createCharacterView(ctx: ClientContext): ViewSource<CharacterView> {
  const { selfStore, sessionStore } = ctx.stores;
  const dataSource = {
    subscribe: (fn: () => void) =>
      ctx.data?.onBundle(() => {
        fn();
      }) ?? (() => undefined),
  };
  /** The own replica's DST adjustments (`dst_view`), read once per derived record. */
  let extrasFor: { derived: unknown; adj: Int32Array | undefined } | null = null;
  const adjOf = (s: SelfState): Int32Array | undefined => {
    if (extrasFor?.derived === s.derived) return extrasFor.adj;
    const id = ctx.entities.self()?.objId ?? 0;
    let adj: Int32Array | undefined;
    try {
      adj = peekRulesRuntime(ctx)?.rules()?.dstView(id);
    } catch {
      adj = undefined;
    }
    extrasFor = { derived: s.derived, adj };
    return adj;
  };
  const attackUp = (s: SelfState): boolean =>
    smActive(s, SM_ATTACK_UP) || smActive(s, SM_ATTACK_UP1);
  const maxCast = (): number => ctx.features.gameplay.set_max_cast;
  const statistics = (): boolean => ctx.features.features.statistics;

  /** The numbers of one rules result as the window prints them. */
  const shown = (s: SelfState, d: DerivedStatsJs): DerivedStats =>
    derivedForWindow(d, atkExtrasOf(adjOf(s), attackUp(s)), statistics(), maxCast());

  let previewKey = '';
  let previewValue: DerivedStats | null = null;
  const preview = (p: StatDelta): DerivedStats | null => {
    if (p.str === 0 && p.sta === 0 && p.dex === 0 && p.int === 0) return null;
    const s = selfStore.getState();
    const key = `${String(s.derived === null ? 0 : 1)}:${String(s.level)}:${JSON.stringify(s.base)}:${JSON.stringify(p)}:${String(adjOf(s)?.length ?? 0)}`;
    if (key === previewKey) return previewValue;
    previewKey = key;
    previewValue = null;
    const rec = ctx.entities.self();
    const rules = peekRulesRuntime(ctx)?.rules();
    if (!rec || !rules) return null;
    try {
      const d = rules.statPreview(rec.objId, statInputOf(s, rec.visual), [
        p.str,
        p.sta,
        p.dex,
        p.int,
      ]);
      previewValue = shown(s, d);
    } catch {
      previewValue = null;
    }
    return previewValue;
  };

  /** `ctx.now()` since which the derived record has been behind the base stats (`-1`: it is not). */
  let staleSince = -1;
  /** The record of `s` matches its base stats, or the window has waited long enough for it. */
  const settled = (s: SelfState): boolean => {
    if (s.derived === null || derivedMatches(s.derived, s)) {
      staleSince = -1;
      return true;
    }
    const now = ctx.now();
    if (staleSince < 0) staleSince = now;
    return now - staleSince >= DERIVED_HOLD_MS;
  };

  const build = (): CharacterView => {
    const s = selfStore.getState();
    const job = ctx.data?.job(s.job);
    const flat = unflattenDerived(s.derived);
    return {
      name: s.name,
      job: job?.name ?? '',
      level: s.level,
      baseJob: (job?.type ?? JTYPE_BASE) === JTYPE_BASE,
      legend: job?.type === JTYPE_MASTER ? 1 : job?.type === JTYPE_HERO ? 2 : 0,
      server: sessionStore.getState().server.name,
      channel: '',
      str: pair(flat?.str, s.base.str),
      sta: pair(flat?.sta, s.base.sta),
      dex: pair(flat?.dex, s.base.dex),
      int: pair(flat?.int, s.base.int),
      remainGp: s.statPoints,
      derived: flat ? shown(s, flat) : EMPTY_DERIVED,
      preview,
      statistics: statistics(),
      // the honor title name needs the title table (WP-CFEAT-40); the fame itself is shown
      fameName: '',
      fame: s.fame,
      pkValue: s.pk.value,
      pkPropensity: s.pk.propensity,
    };
  };

  let last: CharacterView | null = null;
  let lastKey = '';
  return derivedView(
    () => {
      // the base stats changed but their derived numbers are not there yet: nothing new is shown until both are
      if (last && !settled(selfStore.getState())) return last;
      const next = build();
      const key = keyOf(next);
      if (last && key === lastKey) return last;
      last = next;
      lastKey = key;
      return next;
    },
    [selfStore, sessionStore, dataSource],
    (a, b) => a === b,
  );
}
