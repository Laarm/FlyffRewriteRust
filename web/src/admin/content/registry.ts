// The registry of the content panels (Plan/25 section 7.3; WP-ADMIN-30 builds the registry, WP-ADMIN-31 the panels): one entry per system of
// Phase C (Event.lua, lord, tax, secret room, guild siege, rainbow race, quiz, instance dungeons, housing), with what its read view and its
// actions are to show (the table of 7.3) and the work package it waits for. A panel shows "Not available yet (needs WP-...)" until its system
// exists: the server answers `501 needs_feature` for `GET /admin/api/content/{panel}` with the same work package (the table is pinned by
// `crates/flyff-server/tests/golden/admin/content_panels.json`).
//
// To land a panel (WP-ADMIN-31 and the systems it waits for): build its component, then `registerPanelView('lord', () => <LordPanel />)` from the module
// that imports it (the Content tab looks the view up here), and drop the entry's `wp` gate in the server's `PANELS`.
import type { ReactNode } from 'react';

export type PanelId =
  | 'events'
  | 'lord'
  | 'tax'
  | 'secret-room'
  | 'guild-siege'
  | 'rainbow-race'
  | 'quiz'
  | 'instances'
  | 'housing';

export interface ContentPanel {
  readonly id: PanelId;
  readonly title: string;
  /** The work package that has to land first (the server's answer names the same). */
  readonly wp: string;
  /** What the read view shows (7.3, column "Read"). */
  readonly reads: string;
  /** The actions (7.3, column "Actions", capability `act.world`); empty = read only. */
  readonly actions: readonly string[];
}

export const CONTENT_PANELS: readonly ContentPanel[] = [
  {
    id: 'events',
    title: 'Event.lua & scheduler',
    wp: 'WP-ADMIN-31',
    reads: 'Event.lua events, their state, the next fire of the wall jobs',
    actions: ['Reload the Lua scripts (/reload lua)', 'Set an event flag'],
  },
  {
    id: 'lord',
    title: 'Lord & election',
    wp: 'WP-EVENT-10',
    reads:
      'The current lord, the election phase, the candidates, the cool-downs of the lord skills',
    actions: [],
  },
  {
    id: 'tax',
    title: 'Tax',
    wp: 'WP-EVENT-12',
    reads: 'The tax rates of every continent and the amounts collected',
    actions: ['Change a rate (the original GM path)'],
  },
  {
    id: 'secret-room',
    title: 'Secret room',
    wp: 'WP-EVENT-13',
    reads: 'The state, the guilds that entered, the kill counts',
    actions: ['Open', 'Close'],
  },
  {
    id: 'guild-siege',
    title: 'Guild siege',
    wp: 'WP-SOCIAL-16',
    reads: 'The schedule, the applicants and the state of the siege',
    actions: ['Start', 'Stop'],
  },
  {
    id: 'rainbow-race',
    title: 'Rainbow race',
    wp: 'WP-EVENT-17',
    reads: 'The state, the applicants, the games',
    actions: ['Start', 'Next game', 'Finish'],
  },
  {
    id: 'quiz',
    title: 'Quiz',
    wp: 'WP-EVENT-19',
    reads: 'The state of the round and the question sets',
    actions: ['Open', 'Close', 'Import questions'],
  },
  {
    id: 'instances',
    title: 'Instance dungeons',
    wp: 'WP-EVENT-08',
    reads: 'The live instances (also on the map)',
    actions: ['Close an instance (typed confirmation)'],
  },
  {
    id: 'housing',
    title: 'Housing & guild houses',
    wp: 'WP-EVENT-14',
    reads: 'The houses, their upkeep and expiry',
    actions: [],
  },
];

export function panelById(id: string): ContentPanel | undefined {
  return CONTENT_PANELS.find((p) => p.id === id);
}

/** Draws a panel (`() => <LordPanel />`). */
export type PanelRenderer = () => ReactNode;

const views = new Map<PanelId, PanelRenderer>();

/** Registers how to draw a panel whose system exists (the Content tab then shows it instead of the "not available yet" state). */
export function registerPanelView(id: PanelId, view: PanelRenderer): void {
  views.set(id, view);
}

/** The renderer of a panel, if one is registered. */
export function panelView(id: PanelId): PanelRenderer | undefined {
  return views.get(id);
}

/** Tests: forgets the registered views. */
export function resetPanelViewsForTests(): void {
  views.clear();
}
