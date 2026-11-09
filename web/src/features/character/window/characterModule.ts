// The `character` feature module (Plan/45 §20; WP-CFEAT-11): the ports of the character window (`ui.char.setPorts({character})`:
// the base data, the derived numbers and the stat preview, `ModifyStatus` after the confirm), the 3-D previews of the inventory
// and status windows (`previews`), the chat lines of a stat raised by `SetStatLevel`, and the presentation of an own level-up.

import type { ClientContext } from '../../../app/context';
import {
  TID_GAME_DEXUP,
  TID_GAME_INTUP,
  TID_GAME_STAUP,
  TID_GAME_STRUP,
} from '../../../gen/defs/text';
import { MTI_LEVELUP } from '../../../gen/defs/neuz';
import { OBJMSG } from '../../../game/combat/constants';
import { type FeatureModule, fullUi } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { sysChat } from '../../common/text';
import { sendModifyStatus } from '../statAlloc/senders';
import { corePreviewPortOf } from './previews';
import { createCharacterView } from './view';

/** `ANILOOP_1PLAY`: the level-up motion plays once. */
const ANILOOP_1PLAY = 1;
/** `MOP_FIXED` of the motion option word. */
const MOP_FIXED = 0x2;
/** `SetStatLevel.stat_id`: 100 STR, 101 STA, 102 DEX, 103 INT. */
const STAT_UP_TID = [TID_GAME_STRUP, TID_GAME_STAUP, TID_GAME_DEXUP, TID_GAME_INTUP] as const;

/** The public face of the character feature of a context. */
export interface CharacterFeature {
  /** Called with the new level after an own level-up (the audio and effect features may add to it). */
  onLevelUp(fn: (level: number) => void): () => void;
}

const features = new WeakMap<ClientContext, CharacterFeature>();
export function characterOf(ctx: ClientContext): CharacterFeature | undefined {
  return features.get(ctx);
}

export const characterModule: FeatureModule = {
  id: 'character',
  milestone: 3,
  register(fc) {
    const ctx = fc.app;
    const view = createCharacterView(ctx);
    const levelUps = new Set<(level: number) => void>();
    features.set(ctx, {
      onLevelUp(fn) {
        levelUps.add(fn);
        return () => {
          levelUps.delete(fn);
        };
      },
    });

    // `SetStatLevel` of the own character: "your STR rose" (the single-point path; V15 itself sends `ModifyStatus`)
    fc.net.on('setStatLevel', (m, objId) => {
      if (objId !== 0 && !ctx.entities.get(objId)?.isSelf) return;
      const tid = STAT_UP_TID[m.statId - 100];
      if (tid !== undefined) sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid);
    });

    // an own level-up (`OnSetLevel`): the level-up motion when standing, the hooks of the effect and audio features. The level the
    // character had BEFORE the message is remembered here (the core handler of `SetLevel` already updated the store)
    let level = -1;
    fc.net.on('spawnSelf', (m) => {
      level = m.player?.level ?? -1;
    });
    fc.net.on('setExperience', (m, objId) => {
      if (objId === 0 || ctx.entities.get(objId)?.isSelf === true) level = m.level;
    });
    fc.net.on('setLevel', (m, objId) => {
      if (objId !== 0 && ctx.entities.get(objId)?.isSelf !== true) return;
      const before = level;
      level = m.level;
      if (before < 0 || m.level <= before) return;
      // play only when standing: the sim answers `0` (ignored) for a sitting or busy mover
      ctx.sim.localAct(OBJMSG.MOTION, [MTI_LEVELUP, ANILOOP_1PLAY, MOP_FIXED, 0, 0]);
      for (const fn of [...levelUps]) fn(m.level);
    });

    const installed = new WeakSet<object>();
    const ensureUi = (): void => {
      const ui = fullUi(ctx);
      if (!ui || installed.has(ui)) return;
      installed.add(ui);
      ui.char.setPorts({
        character: {
          view,
          actions: {
            modifyStatus: (d) => {
              sendModifyStatus(ctx, d);
            },
          },
        },
        // the paper doll of the inventory window and the portrait of the status window (the transcribed presets of 41)
        previews: corePreviewPortOf(ctx),
      });
    };
    ctx.tickSystems.register('character.ports', ensureUi);
    ctx.machine.subscribe(() => {
      ensureUi();
    });
    ensureUi();

    addProbeMember(ctx, 'character', () => {
      const v = view.get();
      return { ...v, preview: undefined };
    });
  },
};
