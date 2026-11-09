// The inverse-action catalogue (Plan/25 section 2.5; WP-ADMIN-29): for an audited admin action, the action that takes it back, built
// from the record's own data and run as a new, audited action through the same routes (so the capabilities, the rank rules, the
// reasons and the typed confirmations of the server apply to an undo exactly as to the action).
//
//   Mute -> Unmute                         Login block -> Lift                   IP block -> Remove
//   Ban -> Unban                           Grant (pending) -> Cancel pending     Grant (direct) -> remove the serials still present
//   Grant (mail) -> none                   Removal (pending) -> Cancel           Penya adjustment -> the opposite change
//   Teleport -> back to the `from`         Mode flag set -> Clear the flag       Spawn -> Despawn the ids still alive
//   Guild member added -> Remove member     Kill, despawn, live removal, disband, rename -> none
//
// `inverseOf(entry)` is pure: it reads the record and answers the plan or `null`. `Inverse.prepare()` looks at the world where the
// plan depends on it (the grant scan, the IP block's id) and answers what the dialog says and what to run. `runInverse` opens the
// confirmation and runs it.
import { isApiError, mutation, rest } from '../../api/rest';
import type { ConfirmFacts } from '../../app/confirm';
import { runConfirmed } from '../../app/mutate';
import type { ConfirmResult } from '../../components/ConfirmDialog';
import type { Cap } from '../../gen/caps';
import type {
  ActionResult,
  CancelResult,
  ContainerLoc,
  ContainersView,
  IpBlocksView,
  ModeFlagName,
  Reason,
} from '../../gen/dto';
import { toast } from '../../stores/toasts';
import { dataOf, isRecord, num, numbers, str } from './api';
import type { ActionEntry } from './collapse';

export type InverseId =
  | 'unmute'
  | 'unban'
  | 'lift_login_block'
  | 'remove_ip_block'
  | 'cancel_grant'
  | 'undo_grant'
  | 'cancel_removal'
  | 'adjust_gold'
  | 'teleport_back'
  | 'revert_flags'
  | 'despawn'
  | 'remove_member';

/** What the dialog says and what confirming runs. */
export interface Prepared {
  readonly message: string;
  /** Set when there is nothing to take back (everything is gone already): shown as a note, no dialog opens. */
  readonly nothing?: string;
  readonly facts?: ConfirmFacts;
  /** Runs the inverse; answers the text of the success toast. */
  readonly run: (given: ConfirmResult) => Promise<string>;
}

export interface Inverse {
  readonly id: InverseId;
  /** The text of the confirm button. */
  readonly label: string;
  /** Every capability the inverse needs (the button is left out without them). */
  readonly caps: readonly Cap[];
  readonly title: string;
  readonly reason: 'required' | 'optional';
  readonly danger?: boolean;
  readonly prepare: () => Promise<Prepared>;
}

const ready =
  (p: Prepared): (() => Promise<Prepared>) =>
  () =>
    Promise.resolve(p);

/** The reason of the undo: what the operator gave, else a line that says what it undoes. */
function reasonFor(given: ConfirmResult, entry: ActionEntry): Reason {
  const r = given.reason;
  if (r && ((r.text ?? '').trim() !== '' || (r.preset ?? '') !== '')) return r;
  return { text: `Undo of ${entry.ev}` };
}

function plural(n: number, one: string, many: string): string {
  return `${String(n)} ${n === 1 ? one : many}`;
}

// ---- mode flags ------------------------------------------------------------------------------------------------------------

/** The `m_dwMode` bits of the names the console can set (`flyff_common::gm::ModeFlags`; the composite names come last). */
const FLAG_BITS: readonly (readonly [ModeFlagName, number])[] = [
  ['matchless', 0x1],
  ['transparent', 0x2],
  ['onekill', 0x4],
  ['donmove', 0x8],
  ['saytalk', 0x10],
  ['matchless2', 0x20],
  ['no_attack', 0x40],
  ['item', 0x80],
  ['community', 0x100],
  ['talk', 0x200],
  ['shouttalk', 0x400],
  ['expup_stop', 0x40000],
  ['dontalk', 0x610],
  ['observe', 0xc0],
];

function popcount(n: number): number {
  let c = 0;
  for (let v = n >>> 0; v !== 0; v >>>= 1) c += v & 1;
  return c;
}

/** The flag names whose bits make up `mask` (the widest names first: `0x610` is `dontalk`, not three names). */
export function flagNames(mask: number): ModeFlagName[] {
  let rest = mask >>> 0;
  const out: ModeFlagName[] = [];
  const widest = [...FLAG_BITS].sort((a, b) => popcount(b[1]) - popcount(a[1]));
  for (const [name, bits] of widest) {
    if ((rest & bits) === bits) {
      out.push(name);
      rest &= ~bits;
    }
  }
  return out;
}

// ---- containers ------------------------------------------------------------------------------------------------------------

/** The `ContainerLoc` of a container of `GET /characters/{id}/items` (the mailbox and the guild banks are not scanned). */
export function locOf(kind: string, index: number): ContainerLoc | null {
  switch (kind) {
    case 'equipment':
      return 'equipment';
    case 'inventory':
      return 'inventory';
    case 'pocket':
      return { pocket: index };
    case 'bank':
      return { bank: { slot: index } };
    default:
      return null;
  }
}

/** `inventory`, `pocket2`, `bank0` (the names the audit records use) as a `ContainerLoc`. */
export function locOfName(name: string): ContainerLoc | null {
  if (name === 'inventory' || name === 'equipment') return name;
  const m = /^(pocket|bank)(\d+)$/.exec(name);
  if (!m) return null;
  const n = Number(m[2]);
  return m[1] === 'pocket' ? { pocket: n } : { bank: { slot: n } };
}

function tuple3(v: unknown): [number, number, number] | null {
  const n = numbers(v);
  const [x, y, z] = n;
  return n.length === 3 && x !== undefined && y !== undefined && z !== undefined ? [x, y, z] : null;
}

// ---- the grant ---------------------------------------------------------------------------------------------------------------

export interface GrantFacts {
  /** `pending_grants` ids still to cancel. */
  readonly pending: readonly number[];
  /** The serials created by the direct deliveries, per character. */
  readonly delivered: readonly {
    readonly character: number;
    readonly serials: readonly number[];
  }[];
  readonly mails: number;
  readonly failed: number;
  /** The pieces of each item line, in the order of the serials. */
  readonly counts: readonly number[];
  readonly gold: number;
}

/** What the `admin.grant` record says happened (tolerant: a record of an older shape gives an empty answer). */
export function grantFacts(d: Readonly<Record<string, unknown>>): GrantFacts {
  const pending: number[] = [];
  const delivered: { character: number; serials: number[] }[] = [];
  let mails = 0;
  let failed = 0;
  const report = d['report'];
  const results =
    isRecord(report) && Array.isArray(report['results']) ? (report['results'] as unknown[]) : [];
  for (const pair of results) {
    if (!Array.isArray(pair)) continue;
    const [who, outcome] = pair as unknown[];
    const character = num(who);
    if (character === null || !isRecord(outcome)) continue;
    const dv = outcome['delivered'];
    const ml = outcome['mailed'];
    const pd = outcome['pending'];
    if (isRecord(dv)) delivered.push({ character, serials: numbers(dv['serials']) });
    else if (isRecord(ml)) mails += Math.max(1, numbers(ml['mail_ids']).length);
    else if (isRecord(pd)) pending.push(...numbers(pd['grant_ids']));
    else if (isRecord(outcome['failed'])) failed += 1;
  }
  const items = Array.isArray(d['items']) ? (d['items'] as unknown[]) : [];
  return {
    pending,
    delivered,
    mails,
    failed,
    counts: items.map((i) => (isRecord(i) ? (num(i['count']) ?? 0) : 0)),
    gold: num(d['gold']) ?? 0,
  };
}

interface FoundCell {
  readonly character: number;
  readonly loc: ContainerLoc;
  readonly slot: number;
  readonly serial: number;
  readonly count: number;
}

/** Looks for the delivered serials in the containers of their recipients (they may have moved to a pocket or a bank since). */
async function findGranted(
  facts: GrantFacts,
): Promise<{ found: FoundCell[]; total: number; unreadable: number }> {
  const found: FoundCell[] = [];
  let total = 0;
  let unreadable = 0;
  for (const d of facts.delivered) {
    total += d.serials.length;
    if (d.serials.length === 0) continue;
    const wanted = new Map(d.serials.map((s, i) => [s, facts.counts[i] ?? 0] as const));
    let view: ContainersView;
    try {
      view = await rest.get<ContainersView>(`/admin/api/characters/${String(d.character)}/items`);
    } catch (error) {
      if (isApiError(error, 'not_found')) {
        unreadable += d.serials.length;
        continue;
      }
      throw error;
    }
    for (const c of view.containers) {
      const loc = locOf(c.kind, c.index);
      if (loc === null) continue;
      for (const cell of c.items) {
        const granted = wanted.get(cell.serial);
        if (granted === undefined) continue;
        found.push({
          character: d.character,
          loc,
          slot: cell.slot,
          serial: cell.serial,
          // what is left of the stack, at most what the grant created
          count: granted > 0 ? Math.min(cell.count, granted) : cell.count,
        });
        wanted.delete(cell.serial);
      }
    }
  }
  return { found, total, unreadable };
}

function grantInverse(entry: ActionEntry, d: Readonly<Record<string, unknown>>): Inverse | null {
  const facts = grantFacts(d);
  const direct = facts.delivered.some((x) => x.serials.length > 0);
  if (!direct && facts.pending.length === 0) return null; // mailed or failed everywhere: none (use Item search)
  const characters = facts.delivered.length;
  const caps: Cap[] = direct ? ['act.items', 'view.inventory'] : ['act.items'];
  return {
    id: direct ? 'undo_grant' : 'cancel_grant',
    label: direct ? 'Remove granted items' : 'Cancel pending grants',
    caps,
    title: direct ? 'Take back a grant' : 'Cancel pending grants',
    reason: 'required',
    danger: direct,
    prepare: async (): Promise<Prepared> => {
      const { found, total, unreadable } = direct
        ? await findGranted(facts)
        : { found: [], total: 0, unreadable: 0 };
      const notes: string[] = [];
      if (direct) {
        notes.push(
          `Found ${String(found.length)} of ${plural(total, 'granted item', 'granted items')} still with ${plural(characters, 'recipient', 'recipients')}; they will be removed.`,
        );
        if (unreadable > 0)
          notes.push(
            `${plural(unreadable, 'item', 'items')} could not be looked for (the character is gone).`,
          );
      }
      if (facts.pending.length > 0) {
        notes.push(
          `${plural(facts.pending.length, 'pending grant', 'pending grants')} will be cancelled; one that was delivered meanwhile is refused.`,
        );
      }
      if (facts.mails > 0)
        notes.push(
          `${plural(facts.mails, 'mail', 'mails')} cannot be taken back: use Item search.`,
        );
      if (facts.gold > 0 && direct)
        notes.push('The penya of the grant stays: use a penya adjustment.');
      const nothing =
        found.length === 0 && facts.pending.length === 0
          ? 'None of the granted items is still there.'
          : undefined;
      return {
        message: notes.join(' '),
        ...(nothing === undefined ? {} : { nothing }),
        facts: { items: found.length, targets: Math.max(1, characters) },
        run: async (given) => {
          const reason = reasonFor(given, entry);
          let cancelled = 0;
          let late = 0;
          let removed = 0;
          let gone = 0;
          for (const id of facts.pending) {
            try {
              await rest.post<CancelResult>(
                `/admin/api/grants/${String(id)}/cancel`,
                mutation({}, reason),
              );
              cancelled += 1;
            } catch (error) {
              if (isApiError(error, 'already_delivered')) late += 1;
              else throw error;
            }
          }
          for (const cell of found) {
            try {
              await rest.post(
                `/admin/api/characters/${String(cell.character)}/items/remove`,
                mutation(
                  { container: cell.loc, slot: cell.slot, serial: cell.serial, count: cell.count },
                  reason,
                  given.confirm,
                ),
                { idempotent: true },
              );
              removed += 1;
            } catch (error) {
              // the item left the slot since the scan: it is gone, which is what the undo wants
              if (isApiError(error, 'stale_target')) gone += 1;
              else throw error;
            }
          }
          const parts: string[] = [];
          if (removed > 0 || found.length > 0)
            parts.push(`removed ${plural(removed, 'item', 'items')}`);
          if (cancelled > 0)
            parts.push(`cancelled ${plural(cancelled, 'pending grant', 'pending grants')}`);
          if (gone > 0) parts.push(`${String(gone)} already gone`);
          if (late > 0)
            parts.push(
              `${plural(late, 'grant was', 'grants were')} delivered meanwhile (use Item search)`,
            );
          const text = parts.join(', ');
          return `Undo done: ${text === '' ? 'nothing to do' : text}.`;
        },
      };
    },
  };
}

// ---- the catalogue -------------------------------------------------------------------------------------------------------

/** The text after the last `(+500)` of a gold summary (`"100 -> 600 (+500)"`): what the game applied, which a clamp may cut. */
export function appliedOf(summary: string | null): number | null {
  const m = /\(([+-]?\d+)\)\s*$/.exec(summary ?? '');
  return m?.[1] === undefined ? null : Number(m[1]);
}

/**
 * The inverse of an action, or `null` when it has none (or this record is not a completed action). Pure.
 */
export function inverseOf(entry: ActionEntry): Inverse | null {
  if (entry.state !== 'done' && entry.state !== 'recorded') return null;
  const d = dataOf(entry.row);
  const character = num(d['character'] ?? d['target']);
  switch (entry.ev) {
    case 'admin.mute': {
      if (str(d['action']) !== 'mute' || character === null) return null;
      return {
        id: 'unmute',
        label: 'Unmute',
        caps: ['act.mute'],
        title: `Unmute character ${String(character)}`,
        reason: 'optional',
        prepare: ready({
          message: `Lift the mute of character ${String(character)}.`,
          run: async (given) => {
            await rest.post<ActionResult>(
              '/admin/api/actions/unmute',
              mutation({ character }, reasonFor(given, entry)),
              { idempotent: true },
            );
            return `Character ${String(character)} is unmuted.`;
          },
        }),
      };
    }
    case 'admin.teleport': {
      const from = d['from'];
      if (character === null || !isRecord(from)) return null;
      const world = num(from['world']);
      const pos = tuple3(from['pos']);
      if (world === null || pos === null) return null;
      const layer = num(from['layer']) ?? 0;
      return {
        id: 'teleport_back',
        label: 'Teleport back',
        caps: ['act.teleport.others'],
        title: `Teleport character ${String(character)} back`,
        reason: 'optional',
        prepare: ready({
          message: `Move character ${String(character)} back to world ${String(world)} at ${pos.map((v) => String(Math.round(v))).join(', ')}. The character must be online.`,
          run: async (given) => {
            await rest.post<ActionResult>(
              '/admin/api/actions/teleport',
              mutation(
                { characters: [character], world, layer, pos, label: 'point' },
                reasonFor(given, entry),
              ),
              { idempotent: true },
            );
            return `Character ${String(character)} is back.`;
          },
        }),
      };
    }
    case 'admin.character_edit':
      return characterEditInverse(entry, d, character);
    case 'account.ban': {
      const account = str(d['account']);
      return account === null ? null : unban(entry, account);
    }
    case 'admin.ip_block': {
      const cidr = str(d['cidr']);
      if (str(d['action']) !== 'add' || cidr === null) return null;
      return {
        id: 'remove_ip_block',
        label: 'Unblock',
        caps: ['act.ip_block', 'view.pii'],
        title: `Remove the block of ${cidr}`,
        reason: 'optional',
        prepare: async (): Promise<Prepared> => {
          const view = await rest.get<IpBlocksView>('/admin/api/ip-blocks');
          const row = view.table.find((b) => b.cidr === cidr);
          if (!row)
            return {
              message: '',
              nothing: `${cidr} is not in the block table any more.`,
              run: () => Promise.resolve(''),
            };
          return {
            message: `Remove ${cidr} from the block table.`,
            run: async () => {
              await rest.delete(`/admin/api/ip-blocks/${String(row.id)}`);
              return `${cidr} is unblocked.`;
            },
          };
        },
      };
    }
    case 'admin.grant':
      return grantInverse(entry, d);
    case 'admin.removal':
    case 'admin.gold':
      return pendingOrGold(entry, d, character);
    case 'admin.spawn': {
      const objs = numbers(d['spawned']);
      if (objs.length === 0) return null;
      const world = num(d['world']);
      const layer = num(d['layer']);
      return {
        id: 'despawn',
        label: 'Despawn',
        caps: ['act.combat'],
        title: `Despawn ${plural(objs.length, 'spawned monster', 'spawned monsters')}`,
        reason: 'optional',
        prepare: ready({
          message: `Remove the ${plural(objs.length, 'monster', 'monsters')} this spawn created; the ones that died or left already are skipped.`,
          run: async (given) => {
            const r = await rest.post<ActionResult>(
              '/admin/api/actions/despawn',
              mutation(
                {
                  objs,
                  ...(world === null ? {} : { world }),
                  ...(layer === null ? {} : { layer }),
                },
                reasonFor(given, entry),
              ),
              { idempotent: true },
            );
            return `Despawned ${String(r.ok)} of ${String(objs.length)}; the others were gone.`;
          },
        }),
      };
    }
    case 'admin.guild': {
      const guild = num(d['guild']);
      const member = num(d['character'] ?? d['member']);
      // "member" with nothing before and something after is an addition; the record of a removal has a `before`
      const added =
        str(d['action']) === 'member' &&
        (d['before'] ?? null) === null &&
        (d['after'] ?? null) !== null;
      if (!added || guild === null || member === null) return null;
      return {
        id: 'remove_member',
        label: 'Remove member',
        caps: ['act.guild'],
        title: `Remove character ${String(member)} from guild ${String(guild)}`,
        reason: 'required',
        prepare: ready({
          message: `Take character ${String(member)} out of guild ${String(guild)} again.`,
          run: async (given) => {
            await rest.delete(
              `/admin/api/guilds/${String(guild)}/members/${String(member)}`,
              mutation({}, reasonFor(given, entry)),
            );
            return `Character ${String(member)} left guild ${String(guild)}.`;
          },
        }),
      };
    }
    default:
      return null;
  }
}

function unban(entry: ActionEntry, account: string): Inverse {
  return {
    id: 'unban',
    label: 'Unban',
    caps: ['act.ban'],
    title: `Unban account ${account}`,
    reason: 'optional',
    prepare: ready({
      message: `Lift the ban of account ${account}.`,
      run: async (given) => {
        await rest.delete(
          `/admin/api/accounts/${encodeURIComponent(account)}/ban`,
          mutation({}, reasonFor(given, entry)),
        );
        return `Account ${account} is unbanned.`;
      },
    }),
  };
}

function characterEditInverse(
  entry: ActionEntry,
  d: Readonly<Record<string, unknown>>,
  character: number | null,
): Inverse | null {
  const field = str(d['field']);
  if (field === null) {
    // the character ban of the players tabs: `{character, account, until}` and no field
    const account = str(d['account']);
    return account !== null && 'until' in d ? unban(entry, account) : null;
  }
  if (character === null) return null;
  if (field === 'login_block') {
    if (d['lift'] === true) return null;
    return {
      id: 'lift_login_block',
      label: 'Lift block',
      caps: ['act.ban'],
      title: `Lift the login block of character ${String(character)}`,
      reason: 'required',
      prepare: ready({
        message: `Let character ${String(character)} log in again.`,
        run: async (given) => {
          await rest.put(
            `/admin/api/characters/${String(character)}/login-block`,
            mutation({ lift: true }, reasonFor(given, entry)),
          );
          return `The login block of character ${String(character)} is lifted.`;
        },
      }),
    };
  }
  if (field === 'mode_flags') {
    const set = num(d['set']) ?? 0;
    const clear = num(d['clear']) ?? 0;
    const undoSet = flagNames(clear);
    const undoClear = flagNames(set);
    if (undoSet.length === 0 && undoClear.length === 0) return null;
    return {
      id: 'revert_flags',
      label: set !== 0 && clear === 0 ? 'Clear flag' : 'Revert flags',
      caps: ['act.character.edit'],
      title: `Revert the mode flags of character ${String(character)}`,
      reason: 'optional',
      prepare: ready({
        message:
          `${undoClear.length > 0 ? `Clear ${undoClear.join(', ')}. ` : ''}${undoSet.length > 0 ? `Set ${undoSet.join(', ')} again.` : ''}`.trim(),
        run: async (given) => {
          await rest.post<ActionResult>(
            '/admin/api/actions/mode-flags',
            mutation({ character, set: undoSet, clear: undoClear }, reasonFor(given, entry)),
            { idempotent: true },
          );
          return `The mode flags of character ${String(character)} are reverted.`;
        },
      }),
    };
  }
  return null;
}

/** The penya adjustment and the removal: a pending row is cancelled; a live penya change is made again in the opposite direction. */
function pendingOrGold(
  entry: ActionEntry,
  d: Readonly<Record<string, unknown>>,
  character: number | null,
): Inverse | null {
  const mode = str(d['mode']);
  const pendingId = num(d['pending_id']);
  const delta = entry.ev === 'admin.gold' ? num(d['delta']) : null;
  if (mode === 'pending' && pendingId !== null) {
    // a positive penya adjustment waits as a pending grant, everything else as a pending removal
    const grant = entry.ev === 'admin.gold' && (delta ?? 0) > 0;
    const path = `/admin/api/${grant ? 'grants' : 'removals'}/${String(pendingId)}/cancel`;
    const what = grant ? 'grant' : 'removal';
    return {
      id: 'cancel_removal',
      label: `Cancel pending ${what}`,
      caps: ['act.items'],
      title: `Cancel pending ${what} #${String(pendingId)}`,
      reason: 'optional',
      prepare: ready({
        message: `The ${what} waits for the next login of the character; cancelling stops it. One that was applied meanwhile is refused.`,
        run: async (given) => {
          await rest.post<CancelResult>(path, mutation({}, reasonFor(given, entry)));
          return `Pending ${what} #${String(pendingId)} is cancelled.`;
        },
      }),
    };
  }
  if (entry.ev !== 'admin.gold' || mode !== 'live' || character === null || delta === null)
    return null;
  const container = locOfName(str(d['container']) ?? 'inventory');
  // penya lives in the inventory and in the banks
  if (
    container === null ||
    container === 'equipment' ||
    (typeof container === 'object' && 'pocket' in container)
  )
    return null;
  const applied = appliedOf(str(d['summary'])) ?? delta;
  if (applied === 0) return null;
  const back = -applied;
  return {
    id: 'adjust_gold',
    label: 'Adjust back',
    caps: ['act.items'],
    title: `Change the penya of character ${String(character)} by ${back > 0 ? '+' : ''}${String(back)}`,
    reason: 'required',
    prepare: ready({
      message: `The adjustment of ${applied > 0 ? '+' : ''}${String(applied)} penya is made in the opposite direction. The result is clamped like any adjustment.`,
      run: async (given) => {
        await rest.post(
          `/admin/api/characters/${String(character)}/gold`,
          mutation({ container, delta: back }, reasonFor(given, entry), given.confirm),
          { idempotent: true },
        );
        return `The penya of character ${String(character)} changed by ${back > 0 ? '+' : ''}${String(back)}.`;
      },
    }),
  };
}

/**
 * Opens the confirmation of an inverse and runs it. `true` when the inverse ran.
 */
export async function runInverse(inverse: Inverse): Promise<boolean> {
  let prepared: Prepared;
  try {
    prepared = await inverse.prepare();
  } catch (error) {
    toast.error(error, inverse.title);
    return false;
  }
  if (prepared.nothing !== undefined) {
    toast.info(prepared.nothing);
    return false;
  }
  const outcome = await runConfirmed({
    title: inverse.title,
    message: prepared.message,
    confirmLabel: inverse.label,
    ...(inverse.danger === true ? { danger: true } : {}),
    ...(prepared.facts === undefined ? {} : { facts: prepared.facts }),
    reason: inverse.reason,
    run: prepared.run,
    success: (text) => text,
  });
  return outcome.ok;
}
