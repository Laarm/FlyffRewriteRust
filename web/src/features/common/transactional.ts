// `closeTransactional(reason)` (Plan/45 §0.10, §2.5): ONE helper closes every transactional window (NPC dialog, trade,
// shop, bank, vendor browse, upgrade windows, beauty shops, mail, ...) and sends the matching close/cancel message.
// Used by a duel accept, the own death, a world change (`Replace`), logout and kick.
//
// The windows are built by later work packages: each registers a row with the window registry below
// (`registerTransactional`); until then the registry is empty and the helper only closes the 42 windows it is
// told about through `closeApps`.

/** Why transactional windows are closed (45 §2.5). */
export type TransactionalReason = 'DuelAccepted' | 'OwnDeath' | 'WorldChange' | 'Logout' | 'Kicked';

export interface TransactionalRow {
  /** Stable id of the row (the feature that registered it). */
  readonly id: string;
  /** The reasons this row reacts to (45 §2.5: `WorldChange` is "all rows except the chat-room membership"). */
  readonly reasons: ReadonlySet<TransactionalReason> | 'all';
  /** Closes the window(s) of the row and sends the cancel message when a session of this kind exists. */
  close(reason: TransactionalReason): void;
}

export interface TransactionalRegistry {
  register(row: TransactionalRow): () => void;
  /** Runs `close(reason)` of every registered row that reacts to `reason`; returns the ids of the rows that ran. */
  closeTransactional(reason: TransactionalReason): string[];
  rows(): readonly string[];
}

export function createTransactionalRegistry(
  onError: (id: string, error: unknown) => void = () => undefined,
): TransactionalRegistry {
  const rows = new Map<string, TransactionalRow>();
  return {
    register(row) {
      rows.set(row.id, row);
      return () => {
        if (rows.get(row.id) === row) rows.delete(row.id);
      };
    },
    closeTransactional(reason) {
      const ran: string[] = [];
      for (const row of [...rows.values()]) {
        if (row.reasons !== 'all' && !row.reasons.has(reason)) continue;
        try {
          row.close(reason);
          ran.push(row.id);
        } catch (error) {
          onError(row.id, error);
        }
      }
      return ran;
    },
    rows: () => [...rows.keys()],
  };
}
