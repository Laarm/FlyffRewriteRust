// Mounting the audio engine (Plan/40 §3.1 B5, 44 §2.2): `createGameAudio(ctx)` is the `createAudio(audioDeps(ctx))` step of
// boot. The context is created SUSPENDED and resumes on the first gesture; the title music starts with the login screen
// (`audio/bindings/app.ts`). The options come from 45's store (`audio` section), the tables from the `sounds` and `music` bundles,
// the files from the asset manager's one-shot fetch at the audio priorities (`Prio.AudioDemand` / `Prio.AudioPrefetch`).
//
// The protocol observers (44 §10.2) and the sim-event consumer (44 §8.3) are registered HERE, at the same step, before
// `registry.freeze()` (boot runs `createAudio` first, then `registerHandlerModules`): the `OBSERVERS` group of `app/features.ts`
// needs no audio entry. `window.__flyffDebug.audio` is the audio probe; with the test hooks on, `injectBatch(base64)` feeds a
// server batch through the real registry (Playwright: a `PlaySound` message without a combat server).

import { audioDeps } from '../../app/hosts';
import type { ClientContext } from '../../app/context';
import { type Audio, createAudio } from '../../audio';
import { audioDataOf } from '../../audio/dataPort';
import type { AudioData } from '../../audio/deps';
import type { ClientDataIndex } from '../../data';
import { Prio } from '../../workers/priority';
import { audioOptionsPort, defineOptionsStore } from '../options';

/** The `ChatStyle` of system lines and the colour of client notices (yellow). */
const NOTICE_COLOR = 0xffffff00;

export function createGameAudio(ctx: ClientContext): Audio {
  const options = defineOptionsStore(ctx.stores.registry);
  const base = audioDeps(ctx, { options: audioOptionsPort(options) });
  // the data port is rebuilt only when the index changes (it memoises the motion table)
  let portOf: ClientDataIndex | null = null;
  let port: AudioData | null = null;
  const data = (): AudioData | null => {
    const index = ctx.data;
    if (index !== portOf) {
      portOf = index;
      port = audioDataOf(index);
    }
    return port;
  };
  const fetchBytes = (path: string, priority: 'demand' | 'prefetch'): Promise<ArrayBuffer> =>
    ctx.assets.fetchBytes(
      { ns: 'res', path },
      priority === 'demand' ? Prio.AudioDemand : Prio.AudioPrefetch,
    );
  const audio = createAudio({
    log: base.log,
    visibility: base.visibility,
    options: base.options,
    assetUrl: base.assetUrl,
    musicBundle: () => ctx.bundles.get('music')?.music,
    soundsBundle: () => ctx.bundles.get('sounds')?.sounds,
    notice: (text) => {
      // one client-only system line; without a chat window (title screens) it goes to the log
      const chat = (ctx.ui as { chat?: { putString?: (...a: unknown[]) => void } } | null)?.chat;
      if (typeof chat?.putString === 'function') chat.putString(text, null, NOTICE_COLOR, 0);
      else ctx.log.info('audio', text);
    },
    onScreen: (fn) => ctx.machine.subscribe(fn),
    screen: () => ctx.machine.screen,
    data,
    fetchBytes,
    state: base.state,
    landHeight: base.landHeight,
    features: {
      splashAutoattacks: () => ctx.features.features.splash_autoattacks,
      betterClockworkAlpha: () => ctx.features.features.better_clockwork_alpha,
    },
    now: () => ctx.now(),
  });
  if (audio.probe && !ctx.probe.sections().includes('audio')) {
    ctx.probe.extend('audio', audio.probe);
    // e2e only (`?e2e=1` / dev builds): feeds a server batch (base64 of a `ServerBatch`) through the inbound queue, so a
    // Playwright run can deliver `PlaySound` / `PlayMusic` / `SetExperience` without a combat server
    if (ctx.probe.test !== undefined) {
      Object.defineProperty(audio.probe, 'injectBatch', {
        enumerable: true,
        value: (base64: string): void => {
          const bin = atob(base64);
          const bytes = new Uint8Array(bin.length);
          for (let i = 0; i < bin.length; i += 1) bytes[i] = bin.charCodeAt(i);
          ctx.net.inbound.enqueue(bytes);
        },
      });
    }
  }
  // before `registry.freeze()` and with the sim bridge installed (boot step B3)
  audio.registerObservers(ctx.net.registry);
  audio.bindSimEvents(ctx.sim.events);
  return audio;
}
