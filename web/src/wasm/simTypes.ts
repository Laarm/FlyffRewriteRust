// Typings and pure readers of the `SimWorld` exports (Plan/31 section 17, Plan/40 section 7).
//
// `SimWorldHandle` is the structural type of the wasm-bindgen class as `game/simBridge.ts` uses it (the generated
// `pkg/flyff_wasm.d.ts` is the contract; this file adds the documentation and the buffer layouts). The readers
// below turn the pointers returned by `render_view` and `take_events` into typed-array views over
// `memory.buffer`: they must be called again after EVERY call into the module, because any call can grow the
// memory and detach the old buffer (rule B4). All numbers come from the generated `gen/sim/wire.ts`.

import { EVENT_BUFFER, EVENT_KINDS, RENDER_VIEW, REPORT, type EventName } from '../gen/sim/wire';

/** The exports of the `SimWorld` class (Plan/31 section 17.3) that exist in every build. */
export interface SimWorldHandle {
  /** `[world_id, mpu, lands_w, lands_h, fly_allowed, housing]`; `world_id = 0` leaves the world. */
  set_world(desc: ArrayLike<number>): void;
  /** One `FLMT` file, bytes unchanged. */
  load_motion_table(bytes: Uint8Array): void;
  /** One `FLMB` file, bytes unchanged. */
  load_model_bounds(bytes: Uint8Array): void;
  /** One `FLYC` file (`Pending -> Loaded`). */
  load_chunk(lx: number, lz: number, bytes: Uint8Array): void;
  /** Back to `Pending`. */
  unload_chunk(lx: number, lz: number): void;
  /** One `FSK1` pack (the blob of `decode_chr`). */
  add_skeleton(skelId: number, fsk1: Uint8Array): void;
  /** One `FMO1` pack (the blob of `decode_ani`) under a `MotionTable` clip id. */
  add_clip(clipId: number, fmo1: Uint8Array): void;
  /** Applies a command buffer; returns the number of records accepted. Throws on a malformed buffer. */
  apply_commands(cmds: Uint8Array): number;
  /** `[buttons, mouse_dx, mouse_dy, level, req_flags]`. */
  local_input(input: ArrayLike<number>): void;
  /** `SendActMsg` on the active mover. */
  local_act(msg: number, p1: number, p2: number, p3: number, p4: number, p5: number): number;
  /** Steps up to 60 frames; returns the number stepped. */
  step(frames: number): number;
  /** Pointer to `[count, dropped, records...]`. */
  take_events(): number;
  /** Pointer to the render view (header first). */
  render_view(alpha: number): number;
  /** Terrain height. */
  land_height(x: number, z: number): number;
  /** Highest floor at or below `y`. */
  full_height(x: number, y: number, z: number): number;
  /** Restarts the render smoothing at the simulated state. */
  reset_render_smoothing(): void;
  /** `[chunks, movers, clips, bytes_static, bytes_clips, decor, ctrls_without_model]`. */
  memory_report(): Uint32Array;
  /** Releases the wasm object. */
  free(): void;
}

/** The parsed header of a render view (word offsets are in `f32` units from the header pointer). */
export interface RenderViewHeader {
  readonly frame: number;
  readonly moverCount: number;
  readonly moversOff: number;
  readonly paletteOff: number;
  readonly paletteFloats: number;
  readonly attachOff: number;
  readonly projCount: number;
  readonly projOff: number;
  readonly reportOff: number;
  readonly decorCount: number;
  readonly decorOff: number;
  readonly paletteRows: number;
  /** Total length of the view in `f32` words (the palette area is padded to whole rows). */
  readonly length: number;
}

/** A view that does not start with the `'SIMV'` magic or has another layout version: a programming error. */
export class RenderViewError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'RenderViewError';
  }
}

/** Parses and validates the 32-word header. Throws {@link RenderViewError} on a mismatch. */
export function parseRenderViewHeader(words: Uint32Array): RenderViewHeader {
  const h = RENDER_VIEW.header;
  const w = (i: number): number => words[i] ?? 0;
  if (w(h.magic) !== RENDER_VIEW.MAGIC) throw new RenderViewError('render view: bad magic');
  if (w(h.version) !== RENDER_VIEW.VERSION)
    throw new RenderViewError('render view: version mismatch');
  if (
    w(h.moverStride) !== RENDER_VIEW.MOVER_STRIDE ||
    w(h.attachStride) !== RENDER_VIEW.ATTACH_STRIDE ||
    w(h.projStride) !== RENDER_VIEW.PROJ_STRIDE ||
    w(h.decorStride) !== RENDER_VIEW.DECOR_STRIDE ||
    w(h.reportFloats) !== RENDER_VIEW.REPORT_FLOATS
  ) {
    throw new RenderViewError('render view: stride mismatch');
  }
  const paletteOff = w(h.paletteOff);
  const paletteRows = w(h.paletteRows);
  return {
    frame: w(h.frameLo) + w(h.frameHi) * 2 ** 32,
    moverCount: w(h.moverCount),
    moversOff: w(h.moversOff),
    paletteOff,
    paletteFloats: w(h.paletteFloats),
    attachOff: w(h.attachOff),
    projCount: w(h.projCount),
    projOff: w(h.projOff),
    reportOff: w(h.reportOff),
    decorCount: w(h.decorCount),
    decorOff: w(h.decorOff),
    paletteRows,
    length: paletteOff + paletteRows * RENDER_VIEW.PALETTE_ROW_FLOATS,
  };
}

/** Zero-copy views over one render view. Valid only until the next call into the module. */
export interface RenderViewViews {
  readonly header: RenderViewHeader;
  /** The whole view as `f32` words (three.js-space matrices, positions, ...). */
  readonly f32: Float32Array;
  /** The same bytes as `u32` words (ids, flags, indices, report bits). */
  readonly u32: Uint32Array;
}

/** Creates the views of the render view at `ptr` from the CURRENT `memory.buffer`. */
export function acquireRenderView(memory: WebAssembly.Memory, ptr: number): RenderViewViews {
  const header = parseRenderViewHeader(
    new Uint32Array(memory.buffer, ptr, RENDER_VIEW.HEADER_WORDS),
  );
  return {
    header,
    f32: new Float32Array(memory.buffer, ptr, header.length),
    u32: new Uint32Array(memory.buffer, ptr, header.length),
  };
}

/** One decoded event record (`[kind, obj_id, frame_lo, a, b, c, d, e]`). */
export interface SimEventRecord {
  kind: number;
  /** The event name of `gen/sim/wire.ts` (`'?'` for an unknown kind). */
  name: EventName | '?';
  objId: number;
  frame: number;
  /** The payload words `a..e` as raw `u32` bits (floats are bit-cast, `Number` via `bitsToFloat`). */
  words: [number, number, number, number, number];
}

const KIND_NAMES = new Map<number, EventName>(
  (Object.keys(EVENT_KINDS) as EventName[]).map((n) => [EVENT_KINDS[n].id, n]),
);

/** Reads the event buffer at `ptr` (`[count, dropped, records...]`); the record count is capped at 256. */
export function readEvents(
  memory: WebAssembly.Memory,
  ptr: number,
): { events: SimEventRecord[]; dropped: number } {
  const head = new Uint32Array(memory.buffer, ptr, EVENT_BUFFER.HEADER_WORDS);
  const count = Math.min(head[0] ?? 0, EVENT_BUFFER.CAPACITY);
  const dropped = head[1] ?? 0;
  const words = new Uint32Array(
    memory.buffer,
    ptr,
    EVENT_BUFFER.HEADER_WORDS + count * EVENT_BUFFER.RECORD_WORDS,
  );
  const events: SimEventRecord[] = [];
  for (let i = 0; i < count; i += 1) {
    const at = EVENT_BUFFER.HEADER_WORDS + i * EVENT_BUFFER.RECORD_WORDS;
    const kind = words[at] ?? 0;
    events.push({
      kind,
      name: KIND_NAMES.get(kind) ?? '?',
      objId: words[at + 1] ?? 0,
      frame: words[at + 2] ?? 0,
      words: [
        words[at + 3] ?? 0,
        words[at + 4] ?? 0,
        words[at + 5] ?? 0,
        words[at + 6] ?? 0,
        words[at + 7] ?? 0,
      ],
    });
  }
  return { events, dropped };
}

const scratch = new DataView(new ArrayBuffer(4));

/** The `f32` whose bits are `bits` (event payloads and report slots store floats as `u32`). */
export function bitsToFloat(bits: number): number {
  scratch.setUint32(0, bits, true);
  return scratch.getFloat32(0, true);
}

/** The local report of the last `step` call, decoded from the report area of the render view. */
export interface LocalReport {
  readonly bits: number;
  readonly clientFrame: number;
  readonly pos: readonly [number, number, number];
  readonly delta: readonly [number, number, number];
  readonly angle: number;
  readonly state: number;
  readonly stateFlags: number;
  readonly motion: number;
  readonly motionEx: number;
  readonly loop: number;
  readonly motionOption: number;
  readonly motionFrame: number;
  readonly angleX: number;
  readonly accPower: number;
  readonly turnAngle: number;
  readonly destPos: readonly [number, number, number];
  readonly destForward: boolean;
  /** `0xFFFFFFFF` when the last destination choice was a position (or nothing was chosen). */
  readonly destObj: number;
  readonly destRange: number;
}

/** Decodes the report area of a render view. */
export function readReport(v: RenderViewViews): LocalReport {
  const s = REPORT.slot;
  const base = v.header.reportOff;
  const f = (i: number): number => v.f32[base + i] ?? 0;
  const u = (i: number): number => v.u32[base + i] ?? 0;
  const i32 = (i: number): number => u(i) | 0;
  return {
    bits: u(s.bits),
    clientFrame: u(s.clientFrame),
    pos: [f(s.pos), f(s.pos + 1), f(s.pos + 2)],
    delta: [f(s.delta), f(s.delta + 1), f(s.delta + 2)],
    angle: f(s.angle),
    state: u(s.state),
    stateFlags: u(s.stateFlags),
    motion: u(s.motion),
    motionEx: i32(s.motionEx),
    loop: i32(s.loop),
    motionOption: u(s.motionOption),
    motionFrame: f(s.motionFrame),
    angleX: f(s.angleX),
    accPower: f(s.accPower),
    turnAngle: f(s.turnAngle),
    destPos: [f(s.destPos), f(s.destPos + 1), f(s.destPos + 2)],
    destForward: f(s.destForward) !== 0,
    destObj: u(s.destObj),
    destRange: f(s.destRange),
  };
}
