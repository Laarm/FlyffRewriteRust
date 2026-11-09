// Schema `flyff.options.v1` of the client options (Plan/45 §27.1, PL-128): ONE JSON object under
// `localStorage['flyff.options.v1']`. The options WINDOW is M8 (WP-CFEAT-4x); the store exists from the title screens on
// because the UI (scale, window alpha, ramp, chat filters), the audio engine and the renderer read it. Sections:
// `render` (41 §23.1), `ui` (42), `input` (43), `audio` (44 §12.1), `game` (45).
//
// Missing fields take the defaults, unknown fields are kept on save, a version other than 1 or unparsable JSON gives the
// defaults (`persistedStore`, the broken value is kept under `<key>.bad`). Forced values of the original stay forced
// (PL-128): `input.mouseSpeed` is 1 at load.

import {
  DEFAULT_AUDIO_OPTIONS,
  normalizeAudioOptions,
  type AudioOptions,
} from '../../audio/options';
import { DEFAULT_RENDER_OPTIONS, type RenderOptions } from '../../render/quality';

export const OPTIONS_KEY = 'flyff.options.v1';
export const OPTIONS_VERSION = 1;

export interface UiOptionsV1 {
  /** `WindowAlpha` 0-255. */
  windowAlpha: number;
  /** 0 = automatic, else 0.6-2.0 (42 §16). */
  uiScale: number;
  /** `ChatFilter0..4` (8.1). */
  chatFilters: number[];
  chatFilterEtc: number;
  instantHelp: boolean;
  tooltipColorSet: 0 | 1 | 2;
  buffStatusMode: 0 | 1;
  notice: boolean;
  noticeSeenHash: string;
  tips: boolean;
}

export interface InputOptionsV1 {
  /** 0/1/2 -> 1.0/0.2/0.1; forced 1 at load. */
  mouseSpeed: 0 | 1 | 2;
  cameraLock: boolean;
  autoAttack: boolean;
  zoomLimit: boolean;
  offTarget: boolean;
  showFps: boolean;
}

export interface GameOptionsV1 {
  damageRender: boolean;
  viewMask: boolean;
  ignoreNpcDialogue: boolean;
  privacy: {
    trade: boolean;
    whisper: boolean;
    party: boolean;
    shout: boolean;
    messenger: boolean;
    friendLogin: boolean;
  };
  petFilter: number;
}

export interface OptionsV1 {
  v: 1;
  render: RenderOptions;
  ui: UiOptionsV1;
  input: InputOptionsV1;
  audio: AudioOptions;
  game: GameOptionsV1;
}

export function defaultOptions(): OptionsV1 {
  return {
    v: 1,
    render: { ...DEFAULT_RENDER_OPTIONS },
    ui: {
      windowAlpha: 255,
      uiScale: 0,
      chatFilters: [],
      chatFilterEtc: 0,
      instantHelp: true,
      tooltipColorSet: 0,
      buffStatusMode: 0,
      notice: true,
      noticeSeenHash: '',
      tips: true,
    },
    input: {
      mouseSpeed: 1,
      cameraLock: true,
      autoAttack: true,
      zoomLimit: true,
      offTarget: false,
      showFps: false,
    },
    audio: { ...DEFAULT_AUDIO_OPTIONS },
    game: {
      damageRender: false,
      viewMask: true,
      ignoreNpcDialogue: false,
      privacy: {
        trade: true,
        whisper: true,
        party: true,
        shout: true,
        messenger: true,
        friendLogin: true,
      },
      petFilter: 0,
    },
  };
}

type Rec = Record<string, unknown>;
const isRec = (v: unknown): v is Rec => typeof v === 'object' && v !== null && !Array.isArray(v);

/** Merges `stored` over `base` section by section (a section that is not an object is ignored); unknown keys are kept. */
function mergeSection<T extends object>(base: T, stored: unknown): T {
  if (!isRec(stored)) return base;
  const out: Rec = { ...stored };
  for (const [k, v] of Object.entries(base)) {
    if (isRec(v)) out[k] = mergeSection(v as object, stored[k]);
    else if (!(k in stored) || typeof stored[k] !== typeof v) out[k] = v;
  }
  return out as T;
}

/** What a loaded state is forced to (the original's forced options, PL-128), and the section defaults. */
export function forceOptions(state: OptionsV1): OptionsV1 {
  const d = defaultOptions();
  const merged = mergeSection(d, state);
  return {
    ...merged,
    v: 1,
    audio: normalizeAudioOptions(merged.audio as unknown as Rec),
    input: { ...merged.input, mouseSpeed: 1 },
  };
}
