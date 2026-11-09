// The `pvp` feature module, revival part (Plan/45 §25.3; WP-CFEAT-12): what the client does when the own character dies and what
// the revival window offers. The combat runtime (17 §11.1, `game/combat`) owns the death animation and the damage handling and
// calls `onOwnDeath`; this module adds the UI half: close the transactional windows, stop the auto-attack, print the death line,
// open `APP_REVIVAL` (not for a guild-combat participant) and close it on the revival messages.

import type { ClientContext } from '../../../app/context';
import { inputProvidersOf } from '../../../app/inputProviders';
import { combatRuntime } from '../../../game/combat';
import { II_SYS_SYS_SCR_RESURRECTION } from '../../../gen/defs/item';
import { APP_REVIVAL } from '../../../gen/defs/resdata';
import { TID_GAME_DEATH, TID_GAME_DEATH_ARENA } from '../../../gen/defs/text';
import { WI_WORLD_ARENA } from '../../../gen/defs/world';
import type { SpawnSelf } from '../../../gen/proto/flyff/v1/world_pb';
import type { RevivalActions, RevivalView } from '../../../ui/windows/dialogs/ports';
import { derivedView, type ViewSource } from '../../common/view';
import { type FeatureModule, fullUi } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { formatTid, sysChat } from '../../common/text';
import { hudOf } from '../../hud';
import { defineInventoryStore } from '../../inventory';
import { sendResurrectionCancel, sendRevival, sendRevivalToLodestar } from './senders';

/** `m_nGuildCombatState`: 1 = a participant of the 1:1 guild combat (no window, no request: the server revives, PL-109). */
export const GC_PARTICIPANT = 1;

/** What the revival window shows (`CWndRevival::OnDraw`): "revive here" needs the scroll and no arena. */
export function revivalViewOf(hasScroll: boolean, worldId: number): RevivalView {
  return {
    standEnabled: hasScroll && worldId !== WI_WORLD_ARENA,
    lodestarEnabled: true,
    // V9 and later hide the lodelight button for good; the item mall is not built (billing DROP)
    lodelightEnabled: false,
    shopVisible: false,
  };
}

/** The public face of the revival feature of a context. */
export interface RevivalFeature {
  /** The revival window is open. */
  isOpen(): boolean;
  /** Opens the window when the player is dead and it is not open (the Esc chain does too). */
  open(): void;
  /** `m_nGuildCombatState` of the own character (the guild-combat feature replaces the join value). */
  setGuildCombatState(fn: () => number): void;
}

const features = new WeakMap<ClientContext, RevivalFeature>();
export function revivalOf(ctx: ClientContext): RevivalFeature | undefined {
  return features.get(ctx);
}

export const revivalModule: FeatureModule = {
  id: 'pvp',
  milestone: 3,
  register(fc) {
    const ctx = fc.app;
    const { selfStore, worldStore } = ctx.stores;
    const inventory = defineInventoryStore(ctx.stores.registry);
    let guildCombatState: () => number = () =>
      (ctx.entities.self()?.spawn as SpawnSelf | undefined)?.player?.guildCombatState ?? 0;

    const view: ViewSource<RevivalView> = derivedView(
      () =>
        revivalViewOf(
          inventory.getState().container.countOf(II_SYS_SYS_SCR_RESURRECTION) > 0,
          worldStore.getState().worldId,
        ),
      [inventory, worldStore],
    );
    const actions: RevivalActions = {
      revive(choice) {
        if (choice === 'lodestar') sendRevivalToLodestar(ctx);
        else if (choice === 'stand') sendRevival(ctx);
        // 'lodelight' is hidden and disabled since V9: no request exists for it
      },
      cancelResurrection() {
        sendResurrectionCancel(ctx);
      },
      shop() {
        // the item mall button is not built (billing DROP, Plan/45 §25.3); it is never visible
      },
    };

    const isOpen = (): boolean => fullUi(ctx)?.wm.isOpen(APP_REVIVAL) ?? false;
    const open = (): void => {
      const ui = fullUi(ctx);
      if (!ui || isOpen()) return;
      ui.dialogs.openRevival({ view, actions });
    };
    const close = (): void => {
      const w = fullUi(ctx)?.wm.get(APP_REVIVAL);
      w?.destroy();
    };

    features.set(ctx, {
      isOpen,
      open,
      setGuildCombatState(fn) {
        guildCombatState = fn;
      },
    });
    inputProvidersOf(ctx).revival = { isOpen, open };

    // the death line the own death printed: it is still on the screen (centre messages hold 3 s and then fade) when a quick
    // revival has already happened, so the revival takes it away again (the original keeps it until the fade; it is the only
    // client message whose cause the revival ends)
    let deathLine: string | null = null;
    const clearDeathLine = (): void => {
      if (deathLine !== null) hudOf(ctx).removeClientMsg(deathLine);
      deathLine = null;
    };

    const rt = combatRuntime(ctx);
    rt.onOwnDeath(() => {
      // 1. every transactional window closes (the NPC dialog, a shop, the bank, a trade, a vendor browse)
      fc.transactional.closeTransactional('OwnDeath');
      // the auto-attack ends (`m_bAutoAttack = FALSE`)
      const targeting = (ctx.input as { core?: { targeting?: { autoAttack: boolean } } }).core
        ?.targeting;
      if (targeting) targeting.autoAttack = false;
      // 2. the death line, 3. the window (a guild-combat participant gets neither window nor request)
      const arena = worldStore.getState().worldId === WI_WORLD_ARENA;
      const tid = arena ? TID_GAME_DEATH_ARENA : TID_GAME_DEATH;
      clearDeathLine();
      if (sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid)) deathLine = formatTid(fc.texts, tid);
      if (guildCombatState() !== GC_PARTICIPANT) open();
    });
    // 4. the revival messages of the own character close the window (HP/MP/FP come from the SetPointParam that follows) and
    // take the death line off the screen
    const revived = (): void => {
      close();
      clearDeathLine();
    };
    rt.onOwnRevived(revived);
    for (const c of ['revivalNtf', 'revivalToLodestarNtf', 'resurrection'] as const) {
      fc.net.on(c, (_m, objId) => {
        if (objId === 0 || ctx.entities.get(objId)?.isSelf === true) revived();
      });
    }

    addProbeMember(ctx, 'revival', () => ({
      open: isOpen(),
      dead: selfStore.getState().dead,
      view: view.get(),
    }));
  },
};
