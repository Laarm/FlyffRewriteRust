// `parsePcmWav` (Plan/44 §6.3, WP-AUDIO-02): the fallback decoder for a `.wav` that `decodeAudioData` rejects (an odd header,
// the single 70 kHz file of the shipped data) and the check behind the corpus test "all 1 004 referenced WAVs are mono PCM".
// A pure function over the bytes: RIFF/WAVE, the chunks walked with the even-size padding rule, `fmt ` with `wFormatTag` 1
// (PCM 8/16/24/32 bit) or 3 (IEEE float 32/64 bit; also `WAVE_FORMAT_EXTENSIBLE` 0xFFFE whose sub-format is one of those),
// the `data` chunk. Anything else (ADPCM, a missing `fmt `/`data`, zero frames) gives `null`. Never throws: the bytes come
// from the network.

import type { AudioBufferLike } from './engine';

/** A decoded PCM file: one `Float32Array` per channel, samples in [-1, 1]. */
export interface PcmWav {
  readonly channels: number;
  readonly sampleRate: number;
  /** Frames (samples per channel). */
  readonly frames: number;
  /** The container format as written. */
  readonly format: 'pcm' | 'float';
  readonly bitsPerSample: number;
  readonly data: readonly Float32Array[];
}

const TAG_PCM = 1;
const TAG_FLOAT = 3;
const TAG_EXTENSIBLE = 0xfffe;

function fourcc(v: DataView, at: number): string {
  return String.fromCharCode(
    v.getUint8(at),
    v.getUint8(at + 1),
    v.getUint8(at + 2),
    v.getUint8(at + 3),
  );
}

interface Fmt {
  readonly tag: number;
  readonly channels: number;
  readonly sampleRate: number;
  readonly bits: number;
}

/** The `fmt ` chunk body; `null` for an unsupported or inconsistent one. */
function parseFmt(v: DataView, at: number, size: number): Fmt | null {
  if (size < 16) return null;
  let tag = v.getUint16(at, true);
  const channels = v.getUint16(at + 2, true);
  const sampleRate = v.getUint32(at + 4, true);
  const bits = v.getUint16(at + 14, true);
  if (tag === TAG_EXTENSIBLE) {
    // cbSize (2), validBits (2), channel mask (4), then the sub-format GUID whose first two bytes are the format tag
    if (size < 26) return null;
    tag = v.getUint16(at + 24, true);
  }
  if (tag !== TAG_PCM && tag !== TAG_FLOAT) return null;
  if (channels < 1 || channels > 32 || sampleRate < 1) return null;
  if (tag === TAG_PCM && bits !== 8 && bits !== 16 && bits !== 24 && bits !== 32) return null;
  if (tag === TAG_FLOAT && bits !== 32 && bits !== 64) return null;
  return { tag, channels, sampleRate, bits };
}

/** Parses a RIFF/WAVE file; `null` when it is not a supported PCM or float WAV. */
export function parsePcmWav(input: ArrayBuffer | Uint8Array): PcmWav | null {
  try {
    const bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
    if (bytes.byteLength < 12) return null;
    const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    if (fourcc(v, 0) !== 'RIFF' || fourcc(v, 8) !== 'WAVE') return null;
    let fmt: Fmt | null = null;
    let at = 12;
    while (at + 8 <= bytes.byteLength) {
      const id = fourcc(v, at);
      const size = v.getUint32(at + 4, true);
      const body = at + 8;
      if (id === 'fmt ') {
        if (body + size > bytes.byteLength) return null;
        fmt = parseFmt(v, body, size);
        if (fmt === null) return null;
      } else if (id === 'data') {
        if (fmt === null) return null;
        // a truncated file declares more than it holds: use what is there, whole frames only
        const available = Math.min(size, bytes.byteLength - body);
        return decode(v, body, available, fmt);
      }
      // chunks are padded to an even size
      at = body + size + (size & 1);
    }
    return null;
  } catch {
    return null;
  }
}

function decode(v: DataView, at: number, bytesAvailable: number, fmt: Fmt): PcmWav | null {
  const bytesPerSample = fmt.bits >> 3;
  const frames = Math.floor(bytesAvailable / (bytesPerSample * fmt.channels));
  if (frames <= 0) return null;
  const data: Float32Array[] = [];
  for (let c = 0; c < fmt.channels; c += 1) data.push(new Float32Array(frames));
  const stride = bytesPerSample * fmt.channels;
  for (let c = 0; c < fmt.channels; c += 1) {
    const out = data[c];
    if (out === undefined) return null;
    let p = at + c * bytesPerSample;
    if (fmt.tag === TAG_FLOAT) {
      if (fmt.bits === 32) {
        for (let i = 0; i < frames; i += 1, p += stride) out[i] = v.getFloat32(p, true);
      } else {
        for (let i = 0; i < frames; i += 1, p += stride) out[i] = v.getFloat64(p, true);
      }
    } else if (fmt.bits === 8) {
      for (let i = 0; i < frames; i += 1, p += stride) out[i] = (v.getUint8(p) - 128) / 128;
    } else if (fmt.bits === 16) {
      for (let i = 0; i < frames; i += 1, p += stride) out[i] = v.getInt16(p, true) / 32768;
    } else if (fmt.bits === 24) {
      for (let i = 0; i < frames; i += 1, p += stride) {
        const raw = v.getUint8(p) | (v.getUint8(p + 1) << 8) | (v.getUint8(p + 2) << 16);
        out[i] = ((raw << 8) >> 8) / 8388608;
      }
    } else {
      for (let i = 0; i < frames; i += 1, p += stride) out[i] = v.getInt32(p, true) / 2147483648;
    }
  }
  return {
    channels: fmt.channels,
    sampleRate: fmt.sampleRate,
    frames,
    format: fmt.tag === TAG_FLOAT ? 'float' : 'pcm',
    bitsPerSample: fmt.bits,
    data,
  };
}

/** The `createBuffer` + `copyToChannel` half of the fallback: a `PcmWav` as an `AudioBuffer` (`null` on failure). */
export function wavToAudioBuffer(
  ctx: {
    createBuffer(
      channels: number,
      length: number,
      sampleRate: number,
    ): AudioBufferLike & { getChannelData?(channel: number): Float32Array };
  },
  wav: PcmWav,
): AudioBufferLike | null {
  try {
    // a context accepts sample rates of 3 kHz..768 kHz; the 70 kHz file is within
    const buffer = ctx.createBuffer(wav.channels, wav.frames, wav.sampleRate);
    for (let c = 0; c < wav.channels; c += 1) {
      const channel = wav.data[c];
      if (channel === undefined) return null;
      if (typeof buffer.copyToChannel === 'function') buffer.copyToChannel(channel, c);
      else buffer.getChannelData?.(c).set(channel);
    }
    return buffer;
  } catch {
    return null;
  }
}
