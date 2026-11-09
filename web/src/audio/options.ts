// Audio options (Plan/44 §12): the typed `AudioOptions`, their defaults, the legacy `neuz.ini` mapping and the gain
// tables (`volume-steps.json`). The values live in 45's options store (`flyff.options.v1`, `audio` section); the engine
// receives them through `deps.options` and applies a change with `setOptions` (20 ms ramps).

export interface AudioOptions {
  /** 0..10: legacy `m_fEffectVolume * 10`. Default 10. */
  sfxStep: number;
  /** 0..10: legacy `m_fBGMVolume * 10`. Default 5. */
  bgmStep: number;
  /** 0..10: NEW (browser master volume). Default 10. */
  masterStep: number;
  /** Legacy `m_bVoice` (T28 file branch). Default true. */
  voice: boolean;
  /** Legacy `BattleBGM` (§11.6). Default false. */
  battleBgm: boolean;
  /** NEW, 0..2000 ms (§11.8). Default 0 (hard cut, the original). */
  bgmCrossfadeMs: number;
  /** NEW: play SFX while the tab is hidden (§4.3). Default false. */
  sfxInBackground: boolean;
  /** NEW: keep the BGM audible while the tab is hidden (§4.3). Default true. */
  bgmInBackground: boolean;
  /** NEW, KQ-30 switch: the alias table of the two fixable BGM references (§5.4). Default false. */
  bgmAliasFix: boolean;
}

export const DEFAULT_AUDIO_OPTIONS: Readonly<AudioOptions> = Object.freeze({
  sfxStep: 10,
  bgmStep: 5,
  masterStep: 10,
  voice: true,
  battleBgm: false,
  bgmCrossfadeMs: 0,
  sfxInBackground: false,
  bgmInBackground: true,
  bgmAliasFix: false,
});

const clampStep = (v: unknown, fallback: number): number => {
  if (typeof v !== 'number' || !Number.isFinite(v)) return fallback;
  return Math.min(10, Math.max(0, Math.round(v)));
};
const bool = (v: unknown, fallback: boolean): boolean => (typeof v === 'boolean' ? v : fallback);

/** A raw record (the options store's `audio` section, a patch) as typed options; a bad field takes its default. */
export function normalizeAudioOptions(
  raw: Readonly<Record<string, unknown>> | null | undefined,
  base: Readonly<AudioOptions> = DEFAULT_AUDIO_OPTIONS,
): AudioOptions {
  const r = raw ?? {};
  const fade = r['bgmCrossfadeMs'];
  return {
    sfxStep: clampStep(r['sfxStep'], base.sfxStep),
    bgmStep: clampStep(r['bgmStep'], base.bgmStep),
    masterStep: clampStep(r['masterStep'], base.masterStep),
    voice: bool(r['voice'], base.voice),
    battleBgm: bool(r['battleBgm'], base.battleBgm),
    bgmCrossfadeMs:
      typeof fade === 'number' && Number.isFinite(fade)
        ? Math.min(2000, Math.max(0, Math.round(fade)))
        : base.bgmCrossfadeMs,
    sfxInBackground: bool(r['sfxInBackground'], base.sfxInBackground),
    bgmInBackground: bool(r['bgmInBackground'], base.bgmInBackground),
    bgmAliasFix: bool(r['bgmAliasFix'], base.bgmAliasFix),
  };
}

export function sameAudioOptions(a: AudioOptions, b: AudioOptions): boolean {
  return (
    a.sfxStep === b.sfxStep &&
    a.bgmStep === b.bgmStep &&
    a.masterStep === b.masterStep &&
    a.voice === b.voice &&
    a.battleBgm === b.battleBgm &&
    a.bgmCrossfadeMs === b.bgmCrossfadeMs &&
    a.sfxInBackground === b.sfxInBackground &&
    a.bgmInBackground === b.bgmInBackground &&
    a.bgmAliasFix === b.bgmAliasFix
  );
}

/**
 * The legacy `Sound v e b` line of `neuz.ini`: `voice = v != 0`, `sfxStep = trunc(e * 10)`, `bgmStep = trunc(b * 10)`
 * (float32 products, like the C++ `float`).
 */
export function optionsFromNeuzIni(
  v: number,
  e: number,
  b: number,
): Pick<AudioOptions, 'voice' | 'sfxStep' | 'bgmStep'> {
  return {
    voice: v !== 0,
    sfxStep: clampStep(Math.trunc(Math.fround(e * 10)), 10),
    bgmStep: clampStep(Math.trunc(Math.fround(b * 10)), 5),
  };
}

/**
 * The legacy attenuation in hundredths of a dB: `h = (int)(-(1.0f - step * 0.1f) * 5000)` evaluated in float32 (every
 * intermediate result rounded with `Math.fround`, 44 §12.2). `-5000` means "not played".
 */
export function sfxHundredthsDb(step: number): number {
  const s = clampStep(step, 10);
  const volume = Math.fround(Math.fround(s) * Math.fround(0.1));
  const h = Math.trunc(Math.fround(-Math.fround(Math.fround(1) - volume) * Math.fround(5000)));
  return h === 0 ? 0 : h;
}

/** `false` for step 0 (legacy: `h == -5000` returns before playing). */
export function sfxPlayable(step: number): boolean {
  return sfxHundredthsDb(step) > -5000;
}

/** The SFX bus gain of a step: `10^(h / 2000)`, 0 when not played. */
export function sfxGain(step: number): number {
  const h = sfxHundredthsDb(step);
  return h <= -5000 ? 0 : 10 ** (h / 2000);
}

/** The BGM bus gain: linear `step / 10` (the Miles master level). */
export function bgmGain(step: number): number {
  return clampStep(step, 5) / 10;
}

/** The master bus gain: `step / 10`. */
export function masterGain(step: number): number {
  return clampStep(step, 10) / 10;
}
