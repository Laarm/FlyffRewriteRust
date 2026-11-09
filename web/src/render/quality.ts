// Render options and quality tiers (Plan/41 section 23, R13). `Medium` is the original default options (the parity
// tier, the default on every device) and renders at the native device pixel ratio up to 2 (owner 2026-10-29); `Low` is the
// player's explicit choice. WP-REND-35 adds the options-window wiring; this file holds the data the base renderer and its
// tests need.

import { DEFAULT_BRIGHT, DEFAULT_CONTRAST, DEFAULT_GAMMA } from './rampLut';

export type QualityTier = 'low' | 'medium' | 'high' | 'custom';

/** `RenderOptions` of Plan/41 section 23.1 (persisted by doc 45; the web-only fields need keys there). */
export interface RenderOptions {
  tier: QualityTier;
  // original options (neuz.ini semantics; defaults = original Init values)
  /** `texture` (1): sky texture variants, weather particle count, point sprites off at 2. */
  textureQuality: 0 | 1 | 2;
  /** `view` (1): terrain LOD ranges. */
  terrainView: 0 | 1 | 2;
  /** `detail` (1): mover/object LOD distances. */
  objectDetail: 0 | 1 | 2;
  /** `distant` (1): the `fDistant` row. */
  objectDistant: 0 | 1 | 2;
  /** `shadow` (2 = blob). */
  shadow: 0 | 1 | 2;
  /** `SFXLEVEL` (5). */
  sfxLevel: 0 | 1 | 2 | 3 | 4 | 5;
  sfxRenderOff: boolean;
  weatherEffect: boolean;
  nameSelf: boolean;
  nameOthers: boolean;
  nameMonsters: boolean;
  hidePets: boolean;
  gamma: number;
  bright: number;
  contrast: number;
  // web-only (needs a ledger row where visible, Plan/41 section 28)
  /** Device pixel ratio cap (`device.ts`: the canvas renders at `min(devicePixelRatio, dprCap)`). */
  dprCap: number;
  msaa: 0 | 4;
  anisotropy: 1 | 4;
  /** `false` keeps the 4 mip levels of the original (KEEP-QUIRK, KQ-43). */
  fullMips: boolean;
  renderInterpolation: boolean;
  poseLod: boolean;
  vramBudgetMb: number;
}

/** The Medium tier = the original defaults (Plan/41 section 23.2). */
export const DEFAULT_RENDER_OPTIONS: Readonly<RenderOptions> = {
  tier: 'medium',
  textureQuality: 1,
  terrainView: 1,
  objectDetail: 1,
  objectDistant: 1,
  shadow: 2,
  sfxLevel: 5,
  sfxRenderOff: false,
  weatherEffect: false,
  nameSelf: true,
  nameOthers: true,
  nameMonsters: true,
  hidePets: false,
  gamma: DEFAULT_GAMMA,
  bright: DEFAULT_BRIGHT,
  contrast: DEFAULT_CONTRAST,
  /** Native device pixel ratio up to 2 (owner 2026-10-29; was 1.5, a DPR-2 screen drew the world at 75 %). */
  dprCap: 2,
  msaa: 0,
  anisotropy: 1,
  fullMips: false,
  renderInterpolation: true,
  poseLod: false,
  vramBudgetMb: 1024,
};

/** The tier presets of Plan/41 section 23.2 (fields not listed keep the Medium value). */
export function tierPreset(tier: 'low' | 'medium' | 'high', devicePixelRatio = 1): RenderOptions {
  const base: RenderOptions = { ...DEFAULT_RENDER_OPTIONS, tier };
  switch (tier) {
    case 'low':
      return {
        ...base,
        textureQuality: 2,
        terrainView: 2,
        objectDetail: 2,
        objectDistant: 2,
        sfxLevel: 3,
        dprCap: 1.0,
        poseLod: true,
        vramBudgetMb: 768,
      };
    case 'medium':
      return base;
    case 'high':
      return {
        ...base,
        textureQuality: 0,
        terrainView: 0,
        objectDetail: 0,
        objectDistant: 0,
        shadow: 1,
        dprCap: Math.min(2, Math.max(1, devicePixelRatio)),
        msaa: 4,
        anisotropy: 4,
        vramBudgetMb: 1536,
      };
  }
}

/** Draw-call budget per tier (Plan/01 section 12.2, Plan/41 section 23.2). */
export const DRAW_CALL_BUDGET: Readonly<Record<'low' | 'medium' | 'high', number>> = {
  low: 800,
  medium: 1200,
  high: 1500,
};
