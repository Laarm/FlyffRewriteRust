// Typed access to the flyff-formats decoders of the wasm module (Plan/30 section 14, Plan/40 section 8.2).
//
// `runDecode(kind, bytes, opts)` calls `decode_<kind>` of the module that was initialised in the CURRENT
// realm (`initSync` in a worker or in a Node test, `init` on the main thread) and returns
// `{ meta, blob }`: `meta` is the plain object described by the generated `gen/formats/*.ts` types, `blob`
// a `Uint8Array` that owns a fresh, exactly sized `ArrayBuffer` (`byteOffset === 0`, so `blob.buffer` is a
// transferable) or `null` (`sfx`). The meta refers to the typed sections of the blob with `BlobView`s;
// `view()` turns one into a typed array.
//
// Layering (eslint/rules.js): decode.worker.ts may import only this file, `wasm/pkg` and `gen/formats`, so
// everything the worker needs from here is self-contained (no loader, no wasmCall).

import {
  decode_ani,
  decode_chr,
  decode_chunk_check,
  decode_dds,
  decode_dds_bc,
  decode_lnd,
  decode_o3d,
  decode_sfx,
  formats_info,
} from './pkg/flyff_wasm';
import type {
  AniMeta,
  BlobView,
  ChrMeta,
  ChrPackOptions,
  DdsMeta,
  DdsPackOptions,
  LndMeta,
  LndPackOptions,
  O3dMeta,
  O3dPackOptions,
  SfxMeta,
} from '../gen/formats';

export type {
  AniMeta,
  AttrMeta,
  BcMode,
  BlobView,
  ChrMeta,
  ChrPackOptions,
  DdsMeta,
  DdsPackOptions,
  ElemType,
  GlUpload,
  LevelMeta,
  LndMeta,
  LndPackOptions,
  O3dMeta,
  O3dPackOptions,
  ObjectMeta,
  SfxMeta,
  Space,
} from '../gen/formats';

/** The jobs answered by the `decode_*` exports of `flyff-wasm`: kind `x` calls `decode_x` (Plan/30 14.3). */
export type DecodeKind = 'o3d' | 'chr' | 'ani' | 'lnd' | 'sfx' | 'dds' | 'dds_bc';

/** Every decode kind, in the order of the plan's table. */
export const DECODE_KINDS: readonly DecodeKind[] = [
  'o3d',
  'chr',
  'ani',
  'lnd',
  'sfx',
  'dds',
  'dds_bc',
];

/** The meta type of each kind (what `meta` of a result is). */
export interface DecodeMetaOf {
  readonly o3d: O3dMeta;
  readonly chr: ChrMeta;
  readonly ani: AniMeta;
  readonly lnd: LndMeta;
  readonly sfx: SfxMeta;
  readonly dds: DdsMeta;
  readonly dds_bc: DdsMeta;
}

/** The options each kind accepts (all optional; `undefined` = defaults). */
export interface DecodeOptionsOf {
  readonly o3d: Partial<O3dPackOptions>;
  readonly chr: Partial<ChrPackOptions>;
  readonly ani: undefined;
  readonly lnd: Partial<LndPackOptions>;
  readonly sfx: undefined;
  readonly dds: Partial<DdsPackOptions>;
  readonly dds_bc: Partial<DdsPackOptions>;
}

/** Result of one decode: the meta and the (owned) blob, `null` for kinds without bulk data. */
export interface DecodeResult<M> {
  readonly meta: M;
  readonly blob: Uint8Array | null;
}

/** Request of the worker contract of Plan/30 14.3 (`bytes` transferred in). */
export interface DecodeRequest {
  readonly id: number;
  readonly kind: DecodeKind;
  readonly bytes: ArrayBuffer;
  readonly opts?: object;
}

/** Response of the worker contract of Plan/30 14.3. */
export type DecodeResponse<M> =
  | { readonly id: number; readonly ok: true; readonly meta: M; readonly blob: ArrayBuffer | null }
  | { readonly id: number; readonly ok: false; readonly error: string; readonly format?: string };

/** What `formats_info()` reports (Plan/30 14.1). */
export interface FormatsInfo {
  readonly parsers: number;
  readonly pack: number;
  readonly bcDecode: number;
  readonly bcEncode: number;
  readonly mips: number;
  readonly limits: {
    readonly maxInputBytes: number;
    readonly maxTexels: number;
    readonly maxDim: number;
  };
}

/** Largest input a decoder accepts (`FormatsInfo.limits.maxInputBytes`; checked again inside wasm). */
export const MAX_DECODER_INPUT = 32 * 1024 * 1024;

// ---- typed sections ---------------------------------------------------------------------------------------

/** The typed array class of each `ElemType`. */
export interface TypedViewOf {
  readonly u8: Uint8Array;
  readonly u16: Uint16Array;
  readonly u32: Uint32Array;
  readonly i32: Int32Array;
  readonly f32: Float32Array;
}

const ELEM_SIZE: Readonly<Record<BlobView['ty'], number>> = {
  u8: 1,
  u16: 2,
  u32: 4,
  i32: 4,
  f32: 4,
};

/**
 * A typed array over the section `v` of `blob` (a view, no copy). Offsets are multiples of 4, so every
 * element type is aligned. Throws a `RangeError` when the view does not fit the blob (a corrupt meta).
 */
export function view<T extends BlobView['ty']>(
  blob: ArrayBufferLike | Uint8Array,
  v: BlobView & { readonly ty: T },
): TypedViewOf[T];
export function view(blob: ArrayBufferLike | Uint8Array, v: BlobView): TypedViewOf[BlobView['ty']] {
  const buffer = blob instanceof Uint8Array ? blob.buffer : blob;
  const base = blob instanceof Uint8Array ? blob.byteOffset : 0;
  const size = ELEM_SIZE[v.ty];
  const end = v.off + v.len;
  if (end > buffer.byteLength - base || v.len % size !== 0 || (base + v.off) % size !== 0) {
    throw new RangeError(
      `blob view ${v.ty} off ${String(v.off)} len ${String(v.len)} does not fit a blob of ${String(buffer.byteLength - base)} bytes`,
    );
  }
  const offset = base + v.off;
  const count = v.len / size;
  switch (v.ty) {
    case 'u8':
      return new Uint8Array(buffer, offset, count);
    case 'u16':
      return new Uint16Array(buffer, offset, count);
    case 'u32':
      return new Uint32Array(buffer, offset, count);
    case 'i32':
      return new Int32Array(buffer, offset, count);
    case 'f32':
      return new Float32Array(buffer, offset, count);
  }
}

/** The raw bytes of a section (`ty` is ignored): for `FSK1`/`FMO1` sections handed to `SimWorld`. */
export function sectionBytes(blob: ArrayBufferLike | Uint8Array, v: BlobView): Uint8Array {
  return view(blob, { ...v, ty: 'u8', len: v.len });
}

// ---- running decoders -------------------------------------------------------------------------------------

function isDecodeResult(raw: unknown): raw is { meta: unknown; blob: Uint8Array | null } {
  if (typeof raw !== 'object' || raw === null) return false;
  const r = raw as { meta?: unknown; blob?: unknown };
  return (
    typeof r.meta === 'object' &&
    r.meta !== null &&
    (r.blob === null || r.blob instanceof Uint8Array)
  );
}

/**
 * Calls `decode_<kind>` of the wasm module of this realm. Throws the `Error` of the export (message
 * `"<format>: <kind> at offset <n>"`, see {@link errorFormat}) for malformed input; a
 * `WebAssembly.RuntimeError` means the instance trapped ({@link classifyWasmError}).
 */
export function runDecode<K extends DecodeKind>(
  kind: K,
  bytes: Uint8Array,
  opts?: DecodeOptionsOf[K],
): DecodeResult<DecodeMetaOf[K]> {
  let raw: unknown;
  switch (kind) {
    case 'o3d':
      raw = decode_o3d(bytes, opts);
      break;
    case 'chr':
      raw = decode_chr(bytes, opts);
      break;
    case 'ani':
      raw = decode_ani(bytes);
      break;
    case 'lnd':
      raw = decode_lnd(bytes, opts);
      break;
    case 'sfx':
      raw = decode_sfx(bytes);
      break;
    case 'dds':
      raw = decode_dds(bytes, opts);
      break;
    case 'dds_bc':
      raw = decode_dds_bc(bytes, opts);
      break;
    default:
      throw new TypeError(`unknown decode kind ${String(kind)}`);
  }
  if (!isDecodeResult(raw)) throw new TypeError(`decode_${kind} returned an unexpected value`);
  return raw as DecodeResult<DecodeMetaOf[K]>;
}

/** `decode_o3d` (Plan/30 13.2): mesh pack, `meta.lods[g].objects[o]` with sections in the blob. */
export const decodeO3d = (bytes: Uint8Array, opts?: Partial<O3dPackOptions>) =>
  runDecode('o3d', bytes, opts);
/** `decode_chr`: the whole blob is one `FSK1` (`SimWorld.add_skeleton`). */
export const decodeChr = (bytes: Uint8Array, opts?: Partial<ChrPackOptions>) =>
  runDecode('chr', bytes, opts);
/** `decode_ani`: the whole blob is one `FMO1` (`SimWorld.add_clip`). */
export const decodeAni = (bytes: Uint8Array) => runDecode('ani', bytes);
/** `decode_lnd`: land grids and placements. */
export const decodeLnd = (bytes: Uint8Array, opts?: Partial<LndPackOptions>) =>
  runDecode('lnd', bytes, opts);
/** `decode_sfx`: effect prototype, no blob. */
export const decodeSfx = (bytes: Uint8Array) => runDecode('sfx', bytes);
/** `decode_dds`: S3TC passthrough / GL layouts. */
export const decodeDds = (bytes: Uint8Array, opts?: Partial<DdsPackOptions>) =>
  runDecode('dds', bytes, opts);
/** `decode_dds_bc`: everything as RGBA8. */
export const decodeDdsBc = (bytes: Uint8Array, opts?: Partial<DdsPackOptions>) =>
  runDecode('dds_bc', bytes, opts);

/**
 * `decode_chunk_check` (the `chunk_check` job, Plan/31 17.2): validates one `FLYC` chunk exactly as fetched and returns a
 * fresh copy of the same bytes (`byteOffset 0`, so `.buffer` is a transferable); throws the named violated rule.
 */
export function checkChunk(bytes: Uint8Array): Uint8Array {
  return decode_chunk_check(bytes);
}

/** `formats_info()` of the module of this realm. */
export function formatsInfo(): FormatsInfo {
  const raw: unknown = formats_info();
  if (typeof raw !== 'object' || raw === null)
    throw new TypeError('formats_info returned no object');
  return raw as FormatsInfo;
}

// ---- errors -----------------------------------------------------------------------------------------------

/**
 * `'trap'` for a wasm trap (`WebAssembly.RuntimeError`, or the `RangeError` V8 throws for a wasm stack
 * overflow): the instance is unusable. `'error'` for everything else, i.e. the `Error` of a `Result::Err`
 * (malformed input): recoverable (Plan/40 7.8).
 */
export function classifyWasmError(e: unknown): 'trap' | 'error' {
  if (typeof WebAssembly === 'object' && e instanceof WebAssembly.RuntimeError) return 'trap';
  if (e instanceof RangeError && /call stack|stack overflow/i.test(e.message)) return 'trap';
  return 'error';
}

const FORMAT_NAMES = new Set(['o3d', 'chr', 'ani', 'lnd', 'sfx', 'dds', 'fsk1', 'fmo1']);

/** The `format` of a decoder error message (`"o3d: bad magic ..."` gives `"o3d"`), if it has one. */
export function errorFormat(message: string): string | undefined {
  const colon = message.indexOf(':');
  if (colon <= 0) return undefined;
  const head = message.slice(0, colon);
  return FORMAT_NAMES.has(head) ? head : undefined;
}
