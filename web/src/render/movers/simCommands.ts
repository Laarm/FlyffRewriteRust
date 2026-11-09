// The slice of the `SimWorld` command buffer the renderer's own movers use (Plan/31 section 17.4): preview movers
// (ADD_MOVER kind 4, motions through ACT records, SET_VISIBLE, REMOVE_MOVER). The layouts come from the generated
// `gen/sim/wire.ts` (single source: `flyff_sim::wire::OPS`), so a layout change fails the build, not the page.
//
// This is not the game's `SimBridge` (doc 40 section 7.3, WP-CLIENT-18), which owns the command buffer of the
// session; previews and the dev pages need only these four records and write them with the same layout.

import { OP, OP_FIELDS, OP_PAYLOAD_LEN, type OpName, type WireField } from '../../gen/sim/wire';

/** Field values of a record: numbers, 64-bit values as `bigint`, arrays for multi-element fields. */
export type FieldValue = number | bigint | readonly (number | bigint)[];

const SIZE: Readonly<Record<WireField['ty'], number>> = {
  u8: 1,
  u16: 2,
  u32: 4,
  i32: 4,
  u64: 8,
  i64: 8,
  f32: 4,
};

/** Encodes one record `{op u16, len u16, payload}` from named field values (unnamed padding stays zero). */
export function encodeCommand(
  op: OpName,
  values: Readonly<Record<string, FieldValue>>,
): Uint8Array {
  const len = OP_PAYLOAD_LEN[op];
  const out = new Uint8Array(4 + len);
  const dv = new DataView(out.buffer);
  dv.setUint16(0, OP[op], true);
  dv.setUint16(2, len, true);
  const fields = OP_FIELDS[op] as Readonly<Record<string, WireField>>;
  for (const [name, field] of Object.entries(fields)) {
    const raw = values[name] ?? 0;
    const items: readonly (number | bigint)[] = typeof raw === 'object' ? raw : [raw];
    items.forEach((v, k) => {
      const at = 4 + field.off + k * SIZE[field.ty];
      switch (field.ty) {
        case 'u8':
          dv.setUint8(at, Number(v));
          break;
        case 'u16':
          dv.setUint16(at, Number(v), true);
          break;
        case 'u32':
          dv.setUint32(at, Number(v), true);
          break;
        case 'i32':
          dv.setInt32(at, Number(v), true);
          break;
        case 'u64':
          dv.setBigUint64(at, BigInt(v), true);
          break;
        case 'i64':
          dv.setBigInt64(at, BigInt(v), true);
          break;
        case 'f32':
          dv.setFloat32(at, Number(v), true);
          break;
      }
    });
  }
  return out;
}

/** Concatenates records into one command buffer. */
export function concatCommands(parts: readonly Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

/** `OBJMSG_MOTION` (`flyff_sim::types`): the `ACT` message that starts a motion on a preview mover. */
export const OBJMSG_MOTION = 46;
/** `ANILOOP_*` of a motion record. */
export const ANILOOP = { PLAY1: 1, CONT: 2, LOOP: 4 } as const;
/** `MOP_*` motion options. */
export const MOP = { SWDFORCE: 1, FIXED: 2, NO_TRANS: 4 } as const;
/** `ADD_MOVER` kind of a preview mover. */
export const KIND_PREVIEW = 4;
/** `MoverKind` codes of `ADD_MOVER`. */
export const MOVER_KIND = { player: 0, monster: 1, npc: 2, pet: 3, preview: 4 } as const;

/** What `ADD_MOVER` needs for a preview mover. */
export interface PreviewMoverSpec {
  readonly id: number;
  /** `MoverProp` id (`MI_*`). */
  readonly moverIndex: number;
  /** Registry object type (`OT_MOVER`) and index of the model (the motion table key). */
  readonly modelType: number;
  readonly modelIndex: number;
  /** The id the skeleton was added under (`add_skeleton`). */
  readonly skelId: number;
  /** LH position, yaw (degrees) and scale. */
  readonly pos: readonly [number, number, number];
  readonly angle: number;
  readonly scale: number;
}

/** `ADD_MOVER` of a preview mover. */
export function addPreviewMover(s: PreviewMoverSpec): Uint8Array {
  return encodeCommand('ADD_MOVER', {
    id: s.id,
    kind: KIND_PREVIEW,
    is_active: 0,
    mover_index: s.moverIndex,
    model_type: s.modelType,
    model_index: s.modelIndex,
    skel_id: s.skelId,
    pos: s.pos,
    angle: s.angle,
    angle_x: 0,
    state: 0,
    state_flags: 0,
    motion: 0xffff_ffff,
    scale: s.scale,
  });
}

/**
 * `ACT` / `OBJMSG_MOTION` of a preview mover: `mti` with the loop mode and option; with `queue` the motion waits
 * behind a running `1PLAY` motion (`MTI_GETUP` then `MTI_RUN`). The frame of the record is ignored for previews.
 */
export function previewMotion(
  id: number,
  mti: number,
  loop: number = ANILOOP.LOOP,
  option = 0,
  queue = false,
): Uint8Array {
  return encodeCommand('ACT', {
    id,
    frame: 0n,
    msg: OBJMSG_MOTION,
    p: [mti, loop, option, 0, 0],
    queue_if_busy: queue ? 1 : 0,
  });
}

export const removeMover = (id: number): Uint8Array => encodeCommand('REMOVE_MOVER', { id });

export const setVisible = (id: number, visible: boolean, wantAttach: boolean): Uint8Array =>
  encodeCommand('SET_VISIBLE', { id, visible: visible ? 1 : 0, want_attach: wantAttach ? 1 : 0 });
