// Memory-growth-safe typed-array views onto the wasm linear memory (Plan/40 section 7.3, B4).
//
// `memory.buffer` is replaced (and the old `ArrayBuffer` detached) whenever the module grows its memory, which
// can happen during ANY call into wasm. A typed array created before such a call silently becomes length 0.
// `MemoryViews` therefore never hands out cached arrays across calls: every getter compares the current
// `memory.buffer` with the one the view was built on and rebuilds the view when it changed. Consumers
// (SimBridge: render view, command buffer, event buffer) read through `views.get(name)` after each call and
// never keep the returned array.

/** The typed array constructors a view may use. */
export interface TypedArrayCtor<T> {
  new (buffer: ArrayBufferLike, byteOffset: number, length: number): T;
  readonly BYTES_PER_ELEMENT: number;
}

/** Where a view lives in linear memory (re-evaluated on every rebuild, so the module may move buffers). */
export interface ViewLocation {
  /** Byte address in linear memory (must be a multiple of the element size). */
  readonly ptr: number;
  /** Number of ELEMENTS. */
  readonly length: number;
}

interface Entry<T extends ArrayBufferView> {
  readonly ctor: TypedArrayCtor<T>;
  readonly locate: () => ViewLocation;
  buffer: ArrayBufferLike | null;
  view: T | null;
  ptr: number;
  length: number;
}

/** Access to one registered view. */
export interface ViewHandle<T> {
  /**
   * The current view: rebuilt when the buffer changed (growth) or the section moved or resized. The result is
   * only valid until the next call into wasm: read it again after every call, never keep it.
   */
  get(): T;
}

/** A set of named views that follow memory growth. */
export class MemoryViews {
  private readonly entries = new Map<string, Entry<ArrayBufferView>>();
  private lastBuffer: ArrayBufferLike | null = null;
  private rebuildCount = 0;

  /** @param memory the memory of the instance (read lazily: `memory.buffer` is a getter). */
  constructor(private readonly memory: Pick<WebAssembly.Memory, 'buffer'>) {}

  /**
   * Registers a view and returns its handle. `locate` returns the CURRENT location (pointer and length of the
   * section, which may change between calls, e.g. when the module reallocates a buffer).
   */
  register<T extends ArrayBufferView>(
    name: string,
    ctor: TypedArrayCtor<T>,
    locate: () => ViewLocation,
  ): ViewHandle<T> {
    const entry: Entry<T> = { ctor, locate, buffer: null, view: null, ptr: -1, length: -1 };
    this.entries.set(name, entry);
    return { get: () => this.read(entry) };
  }

  /** `true` when the memory buffer is not the one the views were last built on (growth happened). */
  get stale(): boolean {
    return this.memory.buffer !== this.lastBuffer;
  }

  /** How often a view was rebuilt (diagnostics; the probe reports it). */
  get rebuilds(): number {
    return this.rebuildCount;
  }

  /** The current view `name` (see {@link ViewHandle.get}); throws for an unknown name. */
  get(name: string): ArrayBufferView {
    const entry = this.entries.get(name);
    if (!entry) throw new Error(`unknown memory view ${name}`);
    return this.read(entry);
  }

  private read<T extends ArrayBufferView>(entry: Entry<T>): T {
    const buffer = this.memory.buffer;
    const { ptr, length } = entry.locate();
    if (
      entry.view === null ||
      entry.buffer !== buffer ||
      entry.ptr !== ptr ||
      entry.length !== length
    ) {
      entry.view = new entry.ctor(buffer, ptr, length);
      entry.buffer = buffer;
      entry.ptr = ptr;
      entry.length = length;
      this.rebuildCount += 1;
    }
    this.lastBuffer = buffer;
    return entry.view;
  }

  /** Drops every cached view (they are rebuilt on the next `get`). */
  invalidate(): void {
    for (const e of this.entries.values()) {
      e.view = null;
      e.buffer = null;
    }
    this.lastBuffer = null;
  }
}
