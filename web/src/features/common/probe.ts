// `window.__flyffDebug.features` (Plan/45 §2.6): read-only snapshots Playwright reads instead of pixels. ONE section object
// per probe; every feature adds its members with `addProbeMember` (the section is created by the first caller, `extend`
// is never called twice). Built-in members: `lastSends(n)` (the last `n` client -> server messages), `openWindows()`.

import type { ClientContext } from '../../app/context';
import { fullUi } from './module';

type ProbeMember = (...args: never[]) => unknown;

const sections = new WeakMap<object, Record<string, ProbeMember>>();

/** The shared `features` section of the context's probe (created on first use). */
export function featureProbe(ctx: ClientContext): Record<string, ProbeMember> {
  const probe = ctx.probe as unknown as Record<string, unknown>;
  const known = sections.get(ctx.probe);
  if (known) return known;
  const section: Record<string, ProbeMember> = {
    lastSends: (n?: number) => ctx.net.sender.recent(n ?? 10),
    openWindows: () =>
      (fullUi(ctx)?.wm.rects() ?? []).map((r) => ({
        app: r.app,
        key: r.key,
        visible: r.visible,
        focused: r.focused,
      })),
  };
  sections.set(ctx.probe, section);
  if (!('features' in probe)) ctx.probe.extend('features', section);
  return section;
}

/** Adds (or replaces) a read-only member of the `features` probe section. */
export function addProbeMember(ctx: ClientContext, name: string, fn: ProbeMember): void {
  featureProbe(ctx)[name] = fn;
}
