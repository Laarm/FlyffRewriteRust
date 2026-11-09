// The browser-side commands (Plan/45 §8.6, 23 §3.10): a registry per context so that the chat input pipeline can run the
// commands of the generated table (`gen/gm.ts`) whose `side` is `client` without knowing what they do. The implementations
// live with the feature that owns the behaviour (`features/texts/clientCommands.ts` registers the ones of this task).

import type { ClientContext } from '../../app/context';

export interface LocalCommandCall {
  /** The tokens after the command name (whitespace separated). */
  readonly args: readonly string[];
  /** The whole line as typed (after the `/r` rewrite). */
  readonly line: string;
}

export type LocalCommand = (call: LocalCommandCall) => void;

export interface LocalCommands {
  /** Registers the handler of a command by its legacy table row (`GmCommand.row`). */
  set(row: number, fn: LocalCommand): void;
  get(row: number): LocalCommand | undefined;
  readonly rows: readonly number[];
  /**
   * Registers the handler of a command by its canonical name (lower-cased): the commands that have no legacy row (`row` 0, every
   * command the rewrite added) cannot be keyed by row. A named handler also takes over a server command in the browser
   * (BFIX1 #006: `/help`), see `input.ts` `routeLine`.
   */
  setNamed(name: string, fn: LocalCommand): void;
  getNamed(name: string): LocalCommand | undefined;
}

interface Registry {
  readonly rows: Map<number, LocalCommand>;
  readonly named: Map<string, LocalCommand>;
}

const registries = new WeakMap<ClientContext, Registry>();

/** The local-command registry of a context. */
export function localCommandsOf(ctx: ClientContext): LocalCommands {
  let r = registries.get(ctx);
  if (!r) {
    r = { rows: new Map(), named: new Map() };
    registries.set(ctx, r);
  }
  const { rows, named } = r;
  return {
    set: (row, fn) => {
      rows.set(row, fn);
    },
    get: (row) => rows.get(row),
    get rows() {
      return [...rows.keys()];
    },
    setNamed: (name, fn) => {
      named.set(name.toLowerCase(), fn);
    },
    getNamed: (name) => named.get(name.toLowerCase()),
  };
}
