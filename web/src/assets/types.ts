// `AssetType<T>` (Plan/40 section 9.3): how a key is loaded. A type declares the worker job that decodes the bytes
// (or `null` when the bytes ARE the asset), whether the decoded CPU data outlives the upload (`cpuKeep`), the
// assets it depends on (the textures of a mesh) and how to free a value. 41 supplies the renderer's types
// (`render/assetTypes.ts`: `TextureType`, `MeshType`, ...), 42 the UI image type, 44 the audio bytes type.

import type { JobKind, WorkerJobKind } from '../workers/protocol';
import type { AssetKey } from './source';
import type { UploadJob } from './uploads';

/** What a decode needs to know about the device (`caps.s3tc` selects `dds` or `dds_bc`). */
export interface AssetCaps {
  readonly s3tc: boolean;
}

/** One worker job. */
export interface DecodeSpec {
  readonly kind: JobKind | WorkerJobKind;
  readonly opts?: unknown;
}

export interface AssetDep {
  readonly key: AssetKey;
  readonly type: AssetType<unknown>;
}

export interface AssetType<T> {
  /** A short unique name; part of the cache identity (`<name>|<key>`). */
  readonly name: string;
  /**
   * `null`: no decode, the fetched `ArrayBuffer` is the input of `toValue` (fetched whole: `FLMT`, `FLYK`, sounds).
   * A kind: that worker job. A function: the job depends on the key and the device (texture form, S3TC).
   */
  readonly decode: JobKind | null | ((key: AssetKey, caps: AssetCaps) => DecodeSpec | null);
  /** Keep the decoded CPU data after the upload. */
  readonly cpuKeep: boolean;
  /** The worker result (or the `ArrayBuffer` when `decode` is null) -> the cached value. Default: the input as it is. */
  toValue?(result: unknown, key: AssetKey): T;
  /** CPU bytes of a value for the budget (default: the byte length of an `ArrayBuffer`/blob inside, else 0). */
  cpuBytes?(value: T): number;
  /** The GPU upload of a decoded value, `null` for none. Queued on the manager's upload queue. */
  upload?(value: T, key: AssetKey): UploadJob | null;
  /** Assets the decoded value needs (acquired with the parent's priority, released with it). */
  dependencies?(value: T): readonly AssetDep[];
  /** Frees what the value owns (called when the entry is evicted or its load was cancelled). */
  dispose?(value: T, key: AssetKey): void;
}

/** Erases the value type of a type, for `AssetDep` lists (the manager only hands the value back to the type). */
export function dep<T>(key: AssetKey, type: AssetType<T>): AssetDep {
  return { key, type: type };
}

/** The plain-bytes type: no decode, the `ArrayBuffer` is the value (sim inputs, audio files, `/dev` content). */
export const BytesType: AssetType<ArrayBuffer> = {
  name: 'bytes',
  decode: null,
  cpuKeep: true,
  cpuBytes: (v) => v.byteLength,
};
