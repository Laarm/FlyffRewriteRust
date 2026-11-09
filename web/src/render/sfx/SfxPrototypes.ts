// The prototype cache (`g_SfxMng.GetSfxBase`, Plan/41 section 16.1): `.sfx` file -> prepared prototype, loaded lazily through an
// `SfxSource` (the asset manager's `sfx` decode job in the game). A file that does not exist or cannot be decoded is remembered as
// missing: the effect is invisible (KQ-30), never retried.

import type { SfxMeta } from '../../gen/formats';
import { Prio } from '../../workers/priority';
import { prepareProto, type SfxProto } from './proto';

/** Where `.sfx` descriptions come from. Rejects when the file does not exist or cannot be decoded. */
export interface SfxSource {
  load(file: string, prio: Prio): Promise<SfxMeta>;
}

export type ProtoState = 'loading' | 'ready' | 'missing';

interface Entry {
  state: ProtoState;
  proto: SfxProto | null;
  promise: Promise<SfxProto | null>;
}

export class SfxPrototypes {
  private readonly entries = new Map<string, Entry>();
  /** Files that failed, with the reason (probe, tests). */
  readonly failures = new Map<string, string>();

  constructor(
    private readonly source: SfxSource,
    private readonly log?: (message: string) => void,
  ) {}

  /** The prepared prototype of `file`, or `null` while it loads or when it is missing. Starts the load on the first call. */
  get(file: string, prio: Prio = Prio.Fx): SfxProto | null {
    return this.entry(file, prio).proto;
  }

  state(file: string): ProtoState | 'unknown' {
    return this.entries.get(file)?.state ?? 'unknown';
  }

  /** Resolves with the prototype (or `null` when missing). */
  whenReady(file: string, prio: Prio = Prio.Fx): Promise<SfxProto | null> {
    return this.entry(file, prio).promise;
  }

  /** Requests a list of files at the `Fx` priority (the common effects after the world entry, Plan/41 16.1). */
  preload(files: Iterable<string>): void {
    for (const f of files) this.entry(f, Prio.Fx);
  }

  get size(): number {
    return this.entries.size;
  }

  private entry(file: string, prio: Prio): Entry {
    let e = this.entries.get(file);
    if (e) return e;
    const created: Entry = { state: 'loading', proto: null, promise: Promise.resolve(null) };
    created.promise = this.source.load(file, prio).then(
      (meta) => {
        try {
          created.proto = prepareProto(file, meta);
          created.state = 'ready';
        } catch (error) {
          this.fail(created, file, error);
        }
        return created.proto;
      },
      (error: unknown) => {
        this.fail(created, file, error);
        return null;
      },
    );
    e = created;
    this.entries.set(file, e);
    return e;
  }

  private fail(e: Entry, file: string, error: unknown): void {
    e.state = 'missing';
    e.proto = null;
    const reason = error instanceof Error ? error.message : String(error);
    this.failures.set(file, reason);
    this.log?.(`sfx ${file}: ${reason}`);
  }

  clear(): void {
    this.entries.clear();
    this.failures.clear();
  }
}
