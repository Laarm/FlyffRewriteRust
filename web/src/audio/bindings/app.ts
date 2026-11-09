// App-state transitions of the music (Plan/44 §11.7, WP-AUDIO-03 title lifecycle, WP-AUDIO-04 in-world resets). The state machine of
// doc 40 decides the screen; this binding turns it into music calls:
//
//   Boot -> Login, return to Login, ReturnToCharSelect (-> CharSelect), Reconnecting -> Login:
//       `playMusicId(BGM_TITLE, 0)` (restarts from 0 when paused, a no-op when it already plays)
//   Login -> CharSelect -> CharCreate: nothing (the title keeps looping)
//   EnterWorld accepted -> Loading (reason `enter`): `music.stop()`, the director state resets (idBgm 0, latch false, firstFlying
//       false, regions cleared); the core sound set is pinned (44 §6.5)
//   Loading (`worldChange`): the `Replace` observer already reset the director (`bindings/net.ts`, pre-phase)
//   Loading (`reattach`) and Reconnecting: nothing, the music continues and the director keeps its state
//   Loading -> InGame: the director ticks start (the world BGM starts within two ticks); the jingle files are warmed
//   `pagehide`: pause

import { BGM_TITLE } from '../../gen/defs/sound';
import type { ScreenState } from '../../stores/screen';
import type { BgmDirector } from '../director';
import type { MusicPlayer } from '../music';
import type { Preloader } from '../preload';

/** English text of `AUDIO_TEXT_OGG_UNSUPPORTED` (§11.9; no TID: a browser-only message). */
export const AUDIO_TEXT_OGG_UNSUPPORTED =
  "This browser cannot play the game's background music (Ogg Vorbis). Sound effects are not affected.";

export interface AppBindingDeps {
  readonly music: MusicPlayer;
  /** One client-only system chat line (`deps.notice`). */
  readonly notice: (text: string) => void;
  /** The BGM director (WP-AUDIO-04): reset when a world is entered. */
  readonly director?: Pick<BgmDirector, 'reset'>;
  /** The prefetch policy (WP-AUDIO-09). */
  readonly preload?: Pick<Preloader, 'onTitle' | 'onLoading' | 'onWorldEntered'>;
}

/** The title states: the title music plays in them. */
const TITLE_STATES: ReadonlySet<ScreenState['s']> = new Set([
  'Login',
  'Connecting',
  'CharSelect',
  'CharCreate',
  'Entering',
]);

export interface AppBinding {
  /** Called on every screen change. */
  onScreen(screen: ScreenState): void;
  pageHide(): void;
}

export function createAppBinding(deps: AppBindingDeps): AppBinding {
  let noticed = false;
  let previous: ScreenState['s'] | null = null;
  return {
    onScreen(screen) {
      const prev = previous;
      previous = screen.s;
      switch (screen.s) {
        case 'Login':
        case 'CharSelect':
          // the title loop; the same file that already plays is a no-op (M3)
          if (prev !== screen.s) deps.music.playMusicId(BGM_TITLE, 0);
          deps.preload?.onTitle();
          break;
        case 'Loading':
          // EnterWorld accepted: the title music stops and a fresh world session starts. A world change is the Replace
          // observer's (it stops the music and resets the regions); a re-attach keeps the music and the director state.
          if (screen.reason === 'enter') {
            deps.music.stop();
            deps.director?.reset();
          } else if (prev === null || TITLE_STATES.has(prev)) {
            // any other way into a loading screen from the title states: no world music exists yet
            deps.music.stop();
          }
          deps.preload?.onLoading();
          break;
        case 'InGame':
          if (prev === 'Loading') deps.preload?.onWorldEntered();
          if (!noticed && deps.music.unsupportedCodecSeen) {
            noticed = true;
            deps.notice(AUDIO_TEXT_OGG_UNSUPPORTED);
          }
          break;
        case 'Boot':
        case 'Connecting':
        case 'CharCreate':
        case 'Entering':
        case 'Leaving':
        case 'Reconnecting':
        case 'Reloading':
        case 'Fatal':
          break;
      }
    },
    pageHide() {
      deps.music.pageHide();
    },
  };
}
