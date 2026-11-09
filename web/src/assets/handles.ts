// Asset handles (Plan/40 section 9.3): what a requester holds. A handle is a reference on one cache entry;
// `release()` is idempotent, the last release of an entry starts its idle clock (or cancels it while it is still
// loading). Handles are held by entities, by lands (`WorldStreamer`), by UI components and by the dev surfaces.

import type { AssetError } from './errors';

export type HandleState = 'loading' | 'ready' | 'failed';

export interface AssetHandle<T> {
  /** `keyString` of the asset. */
  readonly key: string;
  /** The decoded value once `state === 'ready'`. */
  readonly value: T | undefined;
  readonly state: HandleState;
  /** The failure of a `failed` handle. */
  readonly error: AssetError | null;
  /** Resolves with the value, rejects with the `AssetError` (also after a release: the load is not awaited twice). */
  readonly ready: Promise<T>;
  /** The handles of the assets this one depends on (the textures of a mesh), known once the value is decoded. */
  readonly dependencies: readonly AssetHandle<unknown>[];
  /** The GPU resources of the asset were lost with the WebGL context and are not uploaded again yet (Plan/40 section 12.4). */
  readonly needsReupload: boolean;
  /** Gives the reference back (idempotent). */
  release(): void;
}

/** What a handle needs of its owner. */
export interface HandleOwner<E> {
  releaseEntry(entry: E): void;
}

/** The handle object; the cache entry holds the state, the handle only forwards. */
export class EntryHandle<T, E extends HandleView<T>> implements AssetHandle<T> {
  private released = false;

  constructor(
    private readonly entry: E,
    private readonly owner: HandleOwner<E>,
  ) {}

  get key(): string {
    return this.entry.keyStr;
  }
  get value(): T | undefined {
    return this.entry.value;
  }
  get state(): HandleState {
    return this.entry.state;
  }
  get error(): AssetError | null {
    return this.entry.error;
  }
  get ready(): Promise<T> {
    return this.entry.promise;
  }
  get dependencies(): readonly AssetHandle<unknown>[] {
    return this.entry.deps;
  }
  get needsReupload(): boolean {
    return this.entry.reupload === true;
  }

  release(): void {
    if (this.released) return;
    this.released = true;
    this.owner.releaseEntry(this.entry);
  }

  /** `true` once released (tests, diagnostics). */
  get isReleased(): boolean {
    return this.released;
  }
}

/** The fields of a cache entry a handle reads. */
export interface HandleView<T> {
  readonly keyStr: string;
  readonly value: T | undefined;
  readonly state: HandleState;
  readonly error: AssetError | null;
  readonly promise: Promise<T>;
  readonly deps: readonly AssetHandle<unknown>[];
  readonly reupload?: boolean | undefined;
}
