// The asset layer (Plan/40 section 9): keys and sources, the fetch scheduler, the ref-counted manager and its
// caches, URL resolution over the asset manifest, the preparing-assets tracker. WP-CLIENT-09.
// World streaming (`land.ts`, WP-CLIENT-21), the upload queue (`uploads.ts`, implemented in render/gl/uploadQueue) and the pack
// source (`packSource.ts`, M8). Layering rules: eslint/rules.js.
export { AssetError, isAssetError, type AssetErrorReason } from './errors';
export {
  concurrencyFor,
  FetchScheduler,
  HTTP_CONCURRENCY,
  HTTPS_CONCURRENCY,
  MAX_RETRY_AFTER_TRIES,
  parseRetryAfter,
  type FetchRequest,
  type FetchSchedulerOptions,
  type FetchSchedulerStats,
} from './fetchScheduler';
export { EntryHandle, type AssetHandle, type HandleState } from './handles';
export {
  createAssetLayer,
  fetchPrepareProgress,
  type AssetLayer,
  type AssetLayerOptions,
} from './layer';
export {
  createAssetManager,
  type AcquireOptions,
  type AssetManager,
  type AssetManagerOptions,
  type AssetStats,
  type DecodePool,
  type FetchBytesOptions,
} from './manager';
export {
  allBinUrl,
  assetUrl,
  collisionUrl,
  cursorHotspot,
  cursorUrl,
  dirOf,
  encodePath,
  extOf,
  fileOf,
  nibbleUrl,
  normalizeResourcePath,
  resolveManifest,
  type ResolvedManifest,
} from './paths';
export {
  createPrepareTracker,
  type PrepareProgress,
  type PrepareTracker,
  type PrepareTrackerOptions,
} from './prepare';
export {
  HttpAssetSource,
  keyString,
  resKey,
  type AssetKey,
  type AssetSource,
  type FetchMeta,
  type HttpAssetSourceOptions,
} from './source';
export {
  BytesType,
  dep,
  type AssetCaps,
  type AssetDep,
  type AssetType,
  type DecodeSpec,
} from './types';
export {
  coordinateContextLoss,
  createUploads,
  GpuAccounting,
  UPLOAD_BUDGETS,
  uploadMode,
  UploadQueue,
  type UploadFrameResult,
  type UploadJob,
  type UploadMode,
  type UploadsController,
  type UploadsDeps,
} from './uploads';
export { createWorldStreamer, WorldStreamer, windowRadius, landOf, type StreamWorld } from './land';
export { AniPackType, ChrPackType, ChunkType, type PackValue } from './poseTypes';
