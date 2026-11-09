// Confirmation levels (Plan/25 section 2.3), enforced on the client AND recomputed on the server (`admin::guard`):
//   none   read-only actions (no dialog)
//   simple reversible, one target (a click confirms)
//   typed  irreversible actions, more than 10 targets, all players, penya above `admin.confirm_gold_threshold`, more than
//          `admin.confirm_item_threshold` items (the operator types the target name, or CONFIRM for several targets)
// The client uses the defaults of the thresholds; when the server's live thresholds differ it answers `400 confirm_required`
// (the message names what to type) and `runConfirmed` (app/mutate.tsx) asks again with the typed confirmation.

export type ConfirmLevel = 'none' | 'simple' | 'typed';

export interface ConfirmFacts {
  /** Reads never confirm. */
  readonly readOnly?: boolean;
  /** The action cannot be undone (disband, kill, removal, rename, ...). */
  readonly irreversible?: boolean;
  /** How many targets the action touches (default 1). */
  readonly targets?: number;
  /** The target's name when there is exactly one (what is typed). */
  readonly targetName?: string;
  /** The action addresses every player. */
  readonly allPlayers?: boolean;
  /** The absolute penya change. */
  readonly gold?: number;
  /** The number of items created or removed. */
  readonly items?: number;
}

export interface Thresholds {
  /** More targets than this need the typed confirmation. */
  readonly targets: number;
  readonly gold: number;
  readonly items: number;
}

/** `CONFIRM_TARGETS` of the server and the defaults of `admin.confirm_gold_threshold` / `admin.confirm_item_threshold`. */
export const DEFAULT_THRESHOLDS: Thresholds = { targets: 10, gold: 100_000_000, items: 50 };

export function confirmLevel(
  facts: ConfirmFacts,
  t: Thresholds = DEFAULT_THRESHOLDS,
): ConfirmLevel {
  if (facts.readOnly === true) return 'none';
  if (
    facts.irreversible === true ||
    facts.allPlayers === true ||
    (facts.targets ?? 1) > t.targets ||
    (facts.gold ?? 0) > t.gold ||
    (facts.items ?? 0) > t.items
  ) {
    return 'typed';
  }
  return 'simple';
}

/** What the operator must type: the target's name for one named target, else `CONFIRM`. */
export function expectedConfirmation(facts: ConfirmFacts): string {
  return (facts.targets ?? 1) === 1 && facts.allPlayers !== true && facts.targetName
    ? facts.targetName
    : 'CONFIRM';
}

/** Does `typed` satisfy the confirmation? `CONFIRM` is exact; a name matches ignoring ASCII case and surrounding spaces (the server's rule). */
export function confirmationMatches(typed: string, expected: string): boolean {
  const t = typed.trim();
  if (expected === 'CONFIRM') return t === 'CONFIRM';
  return t.length === expected.length && t.toLowerCase() === expected.toLowerCase() && t !== '';
}

/** The text a `400 confirm_required` message asks for ("type Bob to confirm"), or `null`. */
export function expectedFromMessage(message: string): string | null {
  const m = /^type (.+) to confirm$/.exec(message);
  return m?.[1] ?? null;
}

/** The reason presets of `[admin.reason_presets]` (Plan/25 section 2.3 defaults). */
export const DEFAULT_REASON_PRESETS: readonly string[] = [
  'Spam',
  'Abuse',
  'Botting',
  'RMT',
  'Exploit',
  'Impersonation',
  'Support',
  'Event',
  'Other',
];

export interface DurationChip {
  readonly id: string;
  readonly label: string;
  /** Seconds; `null` is permanent. */
  readonly seconds: number | null;
}

/** The duration chips of the mute and ban dialogs (Plan/25 section 2.3). */
export const DURATION_CHIPS: readonly DurationChip[] = [
  { id: '5m', label: '5 min', seconds: 5 * 60 },
  { id: '1h', label: '1 h', seconds: 3600 },
  { id: '1d', label: '1 d', seconds: 86_400 },
  { id: '7d', label: '7 d', seconds: 7 * 86_400 },
  { id: '30d', label: '30 d', seconds: 30 * 86_400 },
  { id: 'perm', label: 'Permanent', seconds: null },
];
