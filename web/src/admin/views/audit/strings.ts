// The texts of the audit pages (Plan/25 section 2.3: all strings in one place).
import type { Group } from './api';

export const GROUP_LABELS: Readonly<Record<Group, string>> = {
  account: 'Account',
  character: 'Character',
  item: 'Item',
  economy: 'Economy',
  guild: 'Guild',
  gm: 'GM',
  admin: 'Admin',
  server: 'Server',
};

export const T = {
  log: {
    title: 'Audit log',
    table: 'Audit records',
    filters: 'Audit filters',
    range: 'Time range',
    from: 'From',
    to: 'Until (optional)',
    groups: 'Event groups',
    actor: 'Actor',
    actorHint: 'staff:joe, console, or an account name',
    character: 'Character id',
    account: 'Account id',
    serial: 'Item serial',
    text: 'Search text',
    textHint: 'Words of the record, a name, a reason',
    search: 'Search',
    reset: 'Reset filters',
    export: 'Export',
    exportJsonl: 'Export JSONL',
    exportCsv: 'Export CSV',
    empty: 'No record matches these filters.',
    emptyHint: 'Widen the time range or remove a filter.',
    own: 'You see your own actions only (capability view.audit.own).',
    lookupRefused: 'A lookup by character or account needs the Items & inventories capability.',
    noRows: 'Select a record to see its raw JSON.',
  },
  mine: {
    title: 'My actions',
    table: 'My actions',
    intro:
      'Your last 50 admin actions. Undo runs the inverse as a new audited action; an action without an inverse has no button.',
    range: 'Look back',
    refresh: 'Refresh',
    empty: 'You have not done anything in this time range.',
    emptyHint: 'Longer ranges show older actions.',
  },
  detail: {
    title: 'Record',
    raw: 'Raw JSON',
    close: 'Close',
    copy: 'Copy JSON',
    copied: 'Copied to the clipboard.',
    copyFailed: 'The browser did not allow copying.',
    requested: 'Request record',
    outcome: 'Outcome record',
    noOutcome: 'No outcome was written: the server stopped, or the action is still running.',
  },
  undo: {
    button: 'Undo',
    undone: 'Undone',
    unavailable: 'This action has no inverse.',
  },
} as const;
