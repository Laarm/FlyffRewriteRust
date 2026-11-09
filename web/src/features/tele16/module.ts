// The `tele16` feature (Plan/45 §26, 15 §10.7; WP-CFEAT-29): the teleporter window `APP_TELE16` (hotkey V, `features.teleportation_system`).
// The list, the texts and the boss panel come from `tables.teleports16` and the item / mover bundles (`model.ts`); Teleport sends `Tele16{index}`
// with the ROW index of the data and closes the window. Refusals (level, class, cooldown, fight state, pets: VR-063) arrive as `Text` lines.

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { Tele16Schema } from '../../gen/proto/flyff/v1/npc_pb';
import type { Tele16Ports, Tele16View, ViewSource } from '../../ui/windows/nav/ports';
import { type FeatureContext, type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { buildEntries } from './model';

export interface Tele16Feature {
  readonly ports: Tele16Ports;
  /** The entries of the window now (the bundles may not be in yet: empty). */
  entries(): Tele16View['entries'];
  /** `SendTeleport( index )`. */
  teleport(index: number): void;
}

const features = new WeakMap<ClientContext, Tele16Feature>();

/** The TELE16 feature of a context; `undefined` before the module registered. */
export function tele16Of(ctx: ClientContext): Tele16Feature | undefined {
  return features.get(ctx);
}

export const tele16Module: FeatureModule = {
  id: 'map',
  milestone: 4,
  register(fc) {
    registerTele16(fc);
  },
};

function registerTele16(fc: FeatureContext): Tele16Feature {
  const ctx = fc.app;
  let cached: { version: number; view: Tele16View } | null = null;

  const get = (): Tele16View => {
    const version = ctx.data?.version ?? -1;
    const hit = cached;
    if (hit?.version === version) return hit.view;
    const rows = ctx.data?.bundle('tables')?.teleports16 ?? [];
    const data = ctx.data;
    const view: Tele16View = {
      entries: data
        ? buildEntries(rows, { mover: (id) => data.mover(id), item: (id) => data.item(id) })
        : [],
    };
    cached = { version, view };
    return view;
  };
  const view: ViewSource<Tele16View> = {
    get,
    subscribe: (fn) =>
      ctx.data
        ? ctx.data.onBundle(() => {
            fn();
          })
        : () => undefined,
  };

  const teleport = (index: number): void => {
    // legacy-send: SendTeleport
    ctx.net.sender.send({ case: 'tele16', value: create(Tele16Schema, { index }) });
  };
  const ports: Tele16Ports = { view, actions: { teleport } };

  const installed = new WeakSet<object>();
  const ensureUi = (): void => {
    const ui = fullUi(ctx);
    if (!ui || installed.has(ui)) return;
    installed.add(ui);
    ui.nav.setPorts({ tele16: ports });
  };
  ctx.tickSystems.register('tele16.ui', ensureUi);
  ctx.machine.subscribe(ensureUi);
  ensureUi();

  addProbeMember(ctx, 'tele16', () => {
    const ui = fullUi(ctx);
    const w = ui?.nav.tele16() ?? null;
    const scale = ui?.scaler.scale ?? 1;
    const css = (r: { l: number; t: number; r: number; b: number } | null) =>
      r
        ? { x: Math.round(((r.l + r.r) / 2) * scale), y: Math.round(((r.t + r.b) / 2) * scale) }
        : null;
    const list = w?.list ?? null;
    const origin = list?.getScreenRect() ?? null;
    const entries = get().entries;
    return {
      open: w !== null,
      entries: entries.map((e) => ({ index: e.index, name: e.name, boss: e.boss?.name ?? null })),
      selected: w?.selected ?? -1,
      selectedIndex: w?.selectedIndex ?? -1,
      width: w ? w.rectWindow.r - w.rectWindow.l : 0,
      texts: w ? w.textLayer.items.map((i) => i.text) : [],
      // the CSS pixel centres the e2e clicks: the list rows and the Teleport button
      rows: entries.map((e, pos) => {
        const r = list?.itemRect(pos);
        return {
          pos,
          index: e.index,
          ...(origin && r
            ? css({ l: origin.l + r.l, t: origin.t + r.t, r: origin.l + r.r, b: origin.t + r.b })
            : { x: 0, y: 0 }),
        };
      }),
      teleport: css(w?.getDlgItem(2003)?.getScreenRect() ?? null),
    };
  });

  const feature: Tele16Feature = { ports, entries: () => get().entries, teleport };
  features.set(ctx, feature);
  return feature;
}
