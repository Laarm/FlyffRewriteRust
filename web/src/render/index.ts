// Public surface of src/render/ (Plan/41 section 2). The renderer is loaded lazily by the game (a dynamic import
// of this module); code on the boot path imports `render/space` and `render/rampLut` DIRECTLY (they pull in no
// three.js). Layering rules: eslint/rules.js.
export {
  addPreviewProbeRenderer,
  installRenderProbe,
  NO_TEXTURES,
  toRenderApi,
  WorldRenderer,
} from './WorldRenderer';
export type {
  MoverAssets,
  RenderPortSources,
  WorldRendererApi,
  WorldRendererOptions,
} from './WorldRenderer';
export { RenderCamera, FAR_PLANE, NEAR_PLANE } from './camera';
export {
  RenderDevice,
  RenderUnsupportedError,
  configureColorPipeline,
  isGammaNaive,
} from './device';
export { StubRenderFx } from './fx';
export type { RenderFx, SfxHandle, SfxOptions } from './fx';
export { DEFAULT_RENDER_OPTIONS, DRAW_CALL_BUDGET, tierPreset } from './quality';
export type { QualityTier, RenderOptions } from './quality';
export { buildRampLut } from './rampLut';
export { emptyStats, StatsCollector } from './stats';
export type { PassName, RenderStats } from './stats';
export type {
  CameraState,
  EntityViewSource,
  FrameInput,
  GameClockSample,
  ObjId,
  PickProxy,
  RenderEvent,
  ScreenRect,
  Vec3LH,
  WorldRenderDesc,
} from './types';
export { mirrorMat, mirrorPos, mirrorQuat, yawToThree } from './space';
export { DEFAULT_ENV } from './gl/materials/env';
export type { EnvLight } from './gl/materials/env';
export { MaterialFactory } from './gl/materials/MaterialFactory';
export { deriveMaterial, XE } from './gl/materials/key';
export { TextureCache, TextureNotFoundError } from './gl/textures';
export type { DecodedTexture, TextureHandle, TextureLoader } from './gl/textures';
export { createPoolTextureLoader } from './gl/textureLoader';
export type { TextureByteSource } from './gl/textureLoader';
export { GpuAccounting, UploadQueue } from './gl/uploadQueue';
export type { UploadJob } from './gl/uploadQueue';
export { applyTextureRules, modelTexturePath, textureCandidates } from './gl/textureNames';
export type { TextureNameRules } from './gl/textureNames';
export { buildModelGeometry, ModelCache } from './objects/ModelCache';
export type { ModelGeometry } from './objects/ModelCache';
export { ModelInstance } from './objects/ModelInstance';
export type { ModelInstanceOptions } from './objects/ModelInstance';
export { LAYER } from './layers';
export { MeshType, SfxTextureType, TextureType, meshTextureKeys } from './assetTypes';

// WP-REND-04/05/06 (render-chars): skinned movers (player / NPC / monster visuals) and the 3D previews.
export { SkeletonType } from './assetTypes';
export { assembleParts, equipOf } from './movers/partRules';
export type { Assembly, AssemblyInput, EquipEntry, PartElement } from './movers/partRules';
export { VisualRegistry } from './movers/registry';
export { MoverVisual, lodFor, LOD_STEP, resolveVisualPlan } from './movers/MoverVisual';
export type {
  ModelProvider,
  MoverFrameInput,
  MoverVisualOptions,
  VisualHost,
} from './movers/MoverVisual';
export { BindPoseSource, MOVER_FLAGS, SimPoseSource } from './movers/poseSource';
export type { MoverPoseFrame, PoseSource } from './movers/poseSource';
export { PaletteTexture } from './movers/PaletteTexture';
export { SimPreviewHost } from './movers/SimPreviewHost';
export type { PreviewMotionSink } from './movers/SimPreviewHost';
export { PreviewRenderer, createWebGlPreviewBackend } from './preview/PreviewRenderer';
export type { PreviewHandle, PreviewSlot } from './preview/PreviewRenderer';
export { PREVIEW_PRESETS } from './preview/presets';

// WP-REND-12/13/15 (render-movers): the world's movers, their blob shadows and the pick support.
export { MoverBatcher, visualHash } from './movers/MoverBatcher';
export type { MoverBatcherDeps, MoverEntry, MoverSimSink } from './movers/MoverBatcher';
export { createManagerModelProvider } from './movers/managerProvider';
export type { ManagerModelProvider } from './movers/managerProvider';
export { HighlightTwins } from './movers/HighlightTwins';
export {
  AUTH_GAMEMASTER,
  FAR_CUT_M,
  FRUSTUM_MARGIN_M,
  MODE as MOVER_MODE,
  decideDraw,
} from './movers/moverRules';
export { BlobShadows, shadowMatrixLH, shadowSkip } from './shadows/BlobShadows';
export { MoverPickSource, PickSupport, projectBox } from './pick/PickSupport';
export type { PickSource } from './pick/PickSupport';

// WP-REND-14 (hud-chat): the in-world overlay (Canvas2D layer above the world canvas). `app/renderHost.ts` / `app/overlayHost.ts`
// wire it into the live game (task wire-overlay).
export {
  composeFx,
  createOverlayAssets,
  DEFAULT_NAME_COLORS,
  DEFAULT_NAME_OPTIONS,
  DEFAULT_NAME_TITLES,
  OverlayLayer,
  parseArgb,
  projectorOfCamera,
  subjectOf,
} from './overlay';
export type {
  DrawnMark,
  DrawnQuestMark,
  DrawnText,
  LegendClass,
  NameColors,
  NameOptions,
  NameTitles,
  OverlayConfig,
  OverlayFx,
  OverlayRenderer,
  OverlayStats,
  OverlaySubject,
  OverlayTextureSource,
  OverlayViewer,
  Placement,
  Projector,
  RgbaImage,
  SpriteRect,
  ViewerSiege,
} from './overlay';

// WP-REND-18/19 (render-combat): the SFX runtime.
export { createManagerSfxSource, SfxMetaType } from './sfx/managerSource';
export type { ManagerSfxSource } from './sfx/managerSource';
export { SfxSystem } from './sfx/SfxSystem';
export type { SfxCatalog, SfxCatalogRow, SfxFrame, WorldQuery } from './sfx/SfxSystem';
export type { SfxAssets } from './WorldRenderer';
