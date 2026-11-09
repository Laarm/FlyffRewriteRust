// The `pet` feature module (Plan/45 §18.7, §7.12; WP-CFEAT-27; windows WP-UI-33, WP-UI-34; the local pet WP-CLIENT-31): the summoned SYSTEM pet of the own
// character and of the players in view, its windows, the pet loot filter and the vis-pet window.
//
//   handlers   `PetCall` / `PetReleaseNtf` / `PetLevelup` / `Pet` build and end the LOCAL pets (`game/pets/eggPet.ts`: the sim drives them) and open or close the
//              status window; `PetState` / `PetSetExp` / `PetFeed` edit the pet data of the summoned item (`InvItem.pet`); `PetMill` and the chance result
//              (`Pet` with `PF_PET_GET_AVAIL`) go to their windows; `SetPetName` renames; `VisPetActivate` opens or closes the vis-pet window;
//              `MoverChangeSfx` attaches the charm effect to the eat pet; `SpawnPlayer` brings the pets of the players that come into view
//   windows    `ui.pets.*` over `views.ts`: the status window follows the inventory item, the chance window runs on `Pet`, the mill and the egg transformer
//              take bag items through the feed protocol (D3), the NPC menus open them (`MMI_PET_STATUS`, `_RELEASE`, `_FOODMILL`, `_RES`, `_EGG01`, `_BUFFPET_STATUS`,
//              `_RENAME_CANCEL`) and the item dispatch asks for them (`petMiracle`, `petFoodMill`, `petLifeConfirm`)
//   filter     `APP_PETFILTER` (hotkey Y, `features.pet_filter`): the mask is `options.game.petFilter` (persisted); OK applies and sends `PetFilter{mask}`; after the
//              join batch the saved mask is sent once (FIX of 45 Open question 9: the legacy server filter reset to 0 at every login while the window showed the saved boxes)
//   commands   `/ClearPetName` (`/cpn`, row 105) and `/makepetfeed` (`/mpf`, row 93)

import type { ClientContext } from '../../app/context';
import { createEggPets, type EggPets } from '../../game/pets/eggPet';
import {
  PF_PET,
  PF_PET_GET_AVAIL,
  PL_B,
  PL_EGG,
  petItemId,
  petMaxEnergy,
  petMaxExp,
  petMoverIndex,
  type PetTables,
} from '../../game/pets/petData';
import {
  APP_BUFFPET_STATUS,
  APP_INVENTORY,
  APP_PET_FOODMILL,
  APP_PET_RES,
  APP_PET_STATUS,
  APP_PET_TRANS_EGGS,
} from '../../gen/defs/resdata';
import {
  MMI_BUFFPET_STATUS,
  MMI_PET_EGG01,
  MMI_PET_FOODMILL,
  MMI_PET_RELEASE,
  MMI_PET_RES,
  MMI_PET_STATUS,
  MMI_RENAME_CANCEL,
} from '../../gen/defs/neuz';
import {
  TID_GAME_BUFFPET_CANCEL,
  TID_GAME_BUFFPET_EQUIP,
  TID_GAME_ERROR_FOOD_MILL_POCKET,
  TID_GAME_ITEMCOUNT_CONFIRM,
  TID_GAME_ITEMCOUNT_QUESTION,
  TID_GAME_KWAIBAWIBO_PRESENT_NUM,
  TID_GAME_LACKSPACE,
  TID_GAME_NAME_PET_S00,
  TID_GAME_NOTFOOD,
  TID_GAME_NOWUSING,
  TID_GAME_PET_GRADE_A,
  TID_GAME_PET_GRADE_B,
  TID_GAME_PET_GRADE_C,
  TID_GAME_PET_GRADE_S,
  TID_GAME_PET_LIFE,
  TID_GAME_PET_STATUS_EXP,
  TID_GAME_PET_STATUS_HP,
  TID_GAME_PET_STATUS_LEVEL,
  TID_GAME_PET_STATUS_ABILITY,
  TID_GAME_PET_USELIFE,
  TID_GAME_PETFEED_MAKE,
  TID_GAME_PETFOODMILL_DESC,
  TID_GAME_PETTRADE,
  TID_GAME_PETTRADE_ERROR,
  TID_GAME_PETTRADE_FAIL,
  TID_GAME_PETTRADE_OK,
  TID_GAME_TRANS_EGGS_ERROR1,
  TID_GAME_TRANS_EGGS_ERROR2,
  TID_GAME_WND_OVERLAPED,
} from '../../gen/defs/text';
import { IK3_EGG, IK3_GEM, IK3_VIS } from '../../gen/defs/itemkind';
import { II_PET_EGG } from '../../gen/defs/item';
import { ItemElemSchema } from '../../gen/proto/flyff/v1/item_types_pb';
import { create } from '@bufbuild/protobuf';
import { fxRuntime } from '../../game/fx';
import type {
  BuffPetRequest,
  BuffPetView,
  FoodConfirmRequest,
  FoodMillRequest,
  PetFilterPort,
  PetMiracleRequest,
  PetResRequest,
  PetStatusRequest,
  PetStatusView,
  TransEggsRequest,
} from '../../ui/windows/pets/ports';
import { type ViewSource } from '../../ui/windows/char/ports';
import { type FeatureContext, type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { formatTid, sysChat } from '../common/text';
import { localCommandsOf } from '../chat/commands';
import { inventoryOf, itemFromWire, type InvItem } from '../inventory';
import { itemUseOf } from '../itemuse/itemuseModule';
import { menuOf } from '../npc/menuModule';
import { setOptionByPath } from '../options';
import { createPetSenders, type PetSenders } from './senders';
import { definePetStore, patchPets, type PetStore } from './store';
import { petPreviewPortOf } from './previews';
import { buffPetView, miraclePicks, pickTip, petStatusView, type StatusEnv } from './views';
import { patchInvItem } from './itemPet';

const NULL_ID = 0xffff_ffff;
/** `TID_GAME_PET_GRADE_x` by level (the chance window names the last two levels of a B / A / S pet: "Lv. C" ... "Lv. S"). */
const GRADE_TID: Readonly<Record<number, number>> = {
  2: TID_GAME_PET_GRADE_C,
  3: TID_GAME_PET_GRADE_B,
  4: TID_GAME_PET_GRADE_A,
  5: TID_GAME_PET_GRADE_S,
};
const loWord = (v: number): number => v & 0xffff;
const hiWord = (v: number): number => (v >>> 16) & 0xffff;

/** The public face of the pet feature of a context. */
export interface PetsFeature {
  readonly store: PetStore;
  readonly senders: PetSenders;
  readonly eggPets: EggPets;
  /** The inventory item of the summoned system pet (`GetPetItem()`), `null` = none. */
  petItem(): InvItem | null;
  /** The mask of the pet filter that is saved (`g_Option.m_dwPetfilter`). */
  filter(): number;
  /** `APP_PETFILTER` OK: saves the mask and sends it. */
  applyFilter(mask: number): void;
  /** Opens (or, when open, closes) the status window like `MMI_PET_STATUS`. */
  toggleStatus(): void;
  readonly stats: {
    petCalls: number;
    released: number;
    filterSent: number;
    miracleResults: number;
    millResults: number;
  };
}

const features = new WeakMap<ClientContext, PetsFeature>();

/** The pet feature of a context; `undefined` before the module registered. */
export function petsOf(ctx: ClientContext): PetsFeature | undefined {
  return features.get(ctx);
}

export const petsModule: FeatureModule = {
  id: 'pet',
  milestone: 4,
  register(fc) {
    registerPets(fc);
  },
};

function registerPets(fc: FeatureContext): PetsFeature | undefined {
  const ctx = fc.app;
  const inv = inventoryOf(ctx);
  if (!inv) return undefined;
  const store = definePetStore(ctx.stores.registry);
  const senders = createPetSenders(ctx);
  const eggPets = createEggPets(ctx);
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const say = (tid: number, args: readonly (string | number)[] = []): void => {
    sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid, args);
  };
  const box = (message: string): void => {
    fullUi(ctx)?.msgbox.open(message);
  };
  const stats: PetsFeature['stats'] = {
    petCalls: 0,
    released: 0,
    filterSent: 0,
    miracleResults: 0,
    millResults: 0,
  };
  const tables = (): PetTables | undefined => ctx.data?.bundle('item-ext')?.pet;
  const ownId = (): number => ctx.entities.self()?.objId ?? 0;
  const isSelf = (objId: number): boolean =>
    objId === 0 || ctx.entities.get(objId)?.isSelf === true;

  // ---- the pet data of the own item -------------------------------------------------------------------------------------------

  const petItem = (): InvItem | null => {
    const id = store.getState().summonedItem;
    return id === null ? null : inv.item(id);
  };
  const eatPetName = (): string => {
    const id = store.getState().eatPetId;
    const rec = id === null ? undefined : ctx.entities.get(id);
    return rec ? (ctx.data?.mover(rec.propId)?.name ?? rec.visual?.name ?? '') : '';
  };
  itemUseOf(ctx)?.setEatPetName(eatPetName);

  /** Edits the pet of the summoned item (`pet.SetEnergy`, `SetExp`, ... clamp like the original). */
  const editPet = (
    fn: (pet: NonNullable<InvItem['pet']>) => Partial<NonNullable<InvItem['pet']>>,
  ) => {
    const it = petItem();
    if (!it?.pet) return false;
    return patchInvItem(inv.store, it.objId, (cur) => {
      if (!cur.pet) return null;
      return { ...cur, pet: { ...cur.pet, ...fn(cur.pet) } };
    });
  };

  // ---- the status window ------------------------------------------------------------------------------------------------------

  const statusEnv = (): StatusEnv => ({
    tables: tables(),
    text,
    itemName: (id) => ctx.data?.item(id)?.name ?? '',
    moverName: (idx) => ctx.data?.mover(idx)?.name ?? '',
    noPetLife: ctx.features.features.no_pet_life,
    moverIndex: (pet) => petMoverIndex(pet.kind, pet.level, tables()),
  });

  const localVisual = (): unknown => {
    const local = eggPets.localIdOf(ownId());
    return local === undefined ? null : (ctx.entities.get(local)?.visual ?? null);
  };

  const listeners = new Set<() => void>();
  const notify = (): void => {
    for (const fn of [...listeners]) fn();
  };
  inv.store.subscribe(notify);
  store.subscribe(notify);

  let statusCache: { item: InvItem; subject: unknown; value: PetStatusView } | null = null;
  const statusView: ViewSource<PetStatusView | null> = {
    get() {
      const it = petItem();
      if (!it?.pet) {
        statusCache = null;
        return null;
      }
      const subject = localVisual();
      if (statusCache?.item === it && statusCache.subject === subject) {
        return statusCache.value;
      }
      const value = petStatusView(it.itemId, it.pet, subject, statusEnv());
      statusCache = { item: it, subject, value };
      return value;
    },
    subscribe(fn) {
      listeners.add(fn);
      return () => {
        listeners.delete(fn);
      };
    },
  };

  const statusRequest = (): PetStatusRequest => ({
    view: statusView,
    labels: {
      level: text(TID_GAME_PET_STATUS_LEVEL),
      ability: text(TID_GAME_PET_STATUS_ABILITY),
      hp: text(TID_GAME_PET_STATUS_HP),
      exp: text(TID_GAME_PET_STATUS_EXP),
    },
    pickTip: (level) => {
      const it = petItem();
      return it?.pet ? pickTip(it.pet, level, statusEnv()) : '';
    },
    previews: petPreviewPortOf(ctx),
  });

  /** `if( FALSE == GetWndBase( APP_PET_STATUS ) ) new CWndPetStatus` - the window of a summoned pet whose data is known. */
  const ensureStatus = (): void => {
    const ui = fullUi(ctx);
    if (!ui || store.getState().summonedItem === null || !petItem()?.pet) return;
    if (ui.wm.isOpen(APP_PET_STATUS)) return;
    ui.pets.openPetStatus(statusRequest());
  };
  const toggleStatus = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    if (ui.wm.isOpen(APP_PET_STATUS)) {
      ui.wm.close(APP_PET_STATUS);
      return;
    }
    if (petItem()?.pet) ui.pets.openPetStatus(statusRequest());
  };

  // ---- the messages -----------------------------------------------------------------------------------------------------------

  /** Assigned below (`installPort`); the handlers above it call it through this holder. */
  let installPortLater: () => void = () => undefined;

  fc.net.on('spawnSelf', (m) => {
    installPortLater();
    const id = m.self?.petId ?? NULL_ID;
    patchPets(store, { summonedItem: id === NULL_ID ? null : id });
  });

  fc.net.on('spawnPlayer', (m, objId) => {
    const pid = m.other?.petId ?? NULL_ID;
    if (pid === NULL_ID) {
      eggPets.release(objId);
      return;
    }
    // the proto says MAKELONG(kind, level); the legacy client read the mover index in the low word: a kind is below 16, a mover index above 700
    const low = loWord(pid);
    const level = hiWord(pid);
    const moverIndex = low < 16 ? petMoverIndex(low, level, tables()) : low;
    if (moverIndex === null) return;
    eggPets.summon(objId, { moverIndex, level, name: m.other?.petName ?? '' });
    eggPets.ownerArrived(objId);
  });

  fc.net.on('delObj', (_m, objId) => {
    eggPets.ownerGone(objId);
  });

  fc.net.on('petCall', (m, objId) => {
    stats.petCalls += 1;
    const owner = objId === 0 ? ownId() : objId;
    if (isSelf(objId)) patchPets(store, { summonedItem: m.itemObjId });
    eggPets.summon(owner, { moverIndex: m.moverIndex, level: m.level, name: m.name });
    if (isSelf(objId)) ensureStatus();
  });

  fc.net.on('petReleaseNtf', (_m, objId) => {
    stats.released += 1;
    const owner = objId === 0 ? ownId() : objId;
    eggPets.release(owner);
    if (isSelf(objId)) {
      patchPets(store, { summonedItem: null, lastFlag: PF_PET });
      fullUi(ctx)?.pets.closeSystemPetWindows();
    }
  });

  fc.net.on('petLevelup', (m, objId) => {
    // the other players' pets: the model changes with the level (`SetPetId( dwPetId ); PetLevelup()`)
    if (isSelf(objId)) return;
    const prev = eggPets.specOf(objId);
    eggPets.summon(objId, {
      moverIndex: loWord(m.petId),
      level: hiWord(m.petId),
      name: prev?.name ?? '',
    });
  });

  fc.net.on('pet', (m) => {
    const it = petItem();
    if (!it || !m.pet) return;
    const pet = m.pet;
    // `*( pItemElem->m_pPet ) = pet`; a level change also changes the item (`m_dwItemId = GetItemId()`) and replaces the model
    const newId = m.flag !== PF_PET ? petItemId(pet.kind, pet.level, tables()) : null;
    patchInvItem(inv.store, it.objId, (cur) => ({
      ...cur,
      pet,
      itemId: newId ?? cur.itemId,
    }));
    patchPets(store, { lastFlag: m.flag });
    if (m.flag !== PF_PET) {
      const idx = petMoverIndex(pet.kind, pet.level, tables());
      const owner = ownId();
      if (idx !== null && owner !== 0) {
        eggPets.summon(owner, { moverIndex: idx, level: pet.level, name: pet.name });
      }
    }
    if (m.flag === PF_PET_GET_AVAIL) {
      stats.miracleResults += 1;
      // `CreatePetSfx`: the chance window learns the new picks of the last two levels
      const win = fullUi(ctx)?.pets.miracle();
      const level = pet.level;
      win?.receiveResult(pet.availLevel[level - 2] ?? 0, pet.availLevel[level - 1] ?? 0);
    }
    ensureStatus();
  });

  fc.net.on('petState', (m) => {
    const it = inv.item(m.itemObjId);
    if (!it?.pet) return;
    const pet = it.pet;
    const old = pet.life;
    const max = petMaxEnergy(pet.level, tables());
    patchInvItem(inv.store, it.objId, (cur) =>
      cur.pet
        ? {
            ...cur,
            pet: {
              ...cur.pet,
              life: m.life,
              energy: Math.min(m.energy, max),
              exp: Math.min(m.exp, petMaxExp(pet.level)),
            },
          }
        : null,
    );
    if (old < m.life) box(formatTid(fc.texts, TID_GAME_PET_LIFE, [m.life - old]));
  });

  fc.net.on('petSetExp', (m) => {
    const it = petItem();
    if (!it?.pet) return;
    const max = petMaxExp(it.pet.level);
    editPet(() => ({ exp: Math.min(m.exp, max) }));
    // (`TID_GAME_PETEGG_HATCH` at the maximum is compiled out by `__SYS_PETLEVELING_MOBKILL`: the eggs level by kills)
  });

  fc.net.on('petFeed', (m, objId) => {
    // `pMover->GetPet()` exists for the own mover only
    if (!isSelf(objId)) return;
    const it = petItem();
    if (!it?.pet) return;
    const max = petMaxEnergy(it.pet.level, tables());
    editPet(() => ({ energy: Math.min(m.energy, max) }));
  });

  fc.net.on('petMill', (m) => {
    stats.millResults += 1;
    fullUi(ctx)?.pets.foodMill()?.receiveResult(m.result, m.count);
  });

  fc.net.on('quePetResurrectionResult', (m) => {
    box(text(m.ok ? TID_GAME_PETTRADE_OK : TID_GAME_PETTRADE_FAIL));
  });

  fc.net.on('setPetName', (m, objId) => {
    const owner = objId === 0 ? ownId() : objId;
    const spec = eggPets.specOf(owner);
    if (spec) eggPets.summon(owner, { ...spec, name: m.name });
    if (isSelf(objId)) {
      editPet(() => ({ name: m.name }));
      say(TID_GAME_NAME_PET_S00);
    }
  });

  // ---- the vis pet --------------------------------------------------------------------------------------------------------------

  const visItem = (): InvItem | null => {
    const id = store.getState().visPetItem;
    return id === null ? null : inv.item(id);
  };
  const needsOf = (itemId: number): readonly [number, number] | undefined => {
    const p = ctx.data?.item(itemId);
    return p ? [p.refer_target1, p.refer_target2] : undefined;
  };
  const iconOf = (itemId: number): string | null => {
    const p = ctx.data?.item(itemId);
    return p && p.icon !== '' ? `item/${p.icon}` : null;
  };
  let buffCache: { item: InvItem; eat: number | null; value: BuffPetView } | null = null;
  const buffView: ViewSource<BuffPetView | null> = {
    get() {
      const it = visItem();
      if (!it) {
        buffCache = null;
        return null;
      }
      const eat = store.getState().eatPetId;
      if (buffCache?.item === it && buffCache.eat === eat) return buffCache.value;
      const rec = eat === null ? undefined : ctx.entities.get(eat);
      const value = buffPetView(it.cards, eatPetName(), rec?.visual ?? null, {
        needs: needsOf,
        icon: iconOf,
      });
      buffCache = { item: it, eat, value };
      return value;
    },
    subscribe(fn) {
      listeners.add(fn);
      return () => {
        listeners.delete(fn);
      };
    },
  };

  /** A charm of a socket as an item for the tooltip (`new CItemElem; m_dwItemId; m_dwKeepTime`). */
  const charmView = (slot: number) => {
    const it = visItem();
    const vis = it?.cards[slot] ?? 0;
    if (!it || vis === 0) return null;
    const item = itemFromWire(
      create(ItemElemSchema, {
        objId: 0,
        itemId: vis,
        num: 1,
        serial: BigInt(vis),
        keepTimeAt: BigInt(it.visExpireAt[slot] ?? 0),
      }),
    );
    const prop = ctx.data?.item(vis);
    return {
      name: prop?.name ?? '',
      kind2: prop?.item_kind2 ?? 0,
      parts: 0,
      serial: `vis:${String(vis)}:${String(slot)}`,
      item,
    };
  };

  /** `CWndBuffPetStatus::DoModal_ConfirmQuestion` -> `CWndConfirmVis`: the question and what Yes sends. */
  const askVis = (kind: 'equip' | 'remove', visItemId: number, yes: () => void): void => {
    const name = ctx.data?.item(visItemId)?.name ?? '';
    fullUi(ctx)?.itemUse.openConfirmEnter({
      text: formatTid(
        fc.texts,
        kind === 'equip' ? TID_GAME_BUFFPET_EQUIP : TID_GAME_BUFFPET_CANCEL,
        [name],
      ),
      yes,
    });
  };

  const buffRequest = (): BuffPetRequest => ({
    view: buffView,
    tooltip: (slot, rect) => {
      const ui = fullUi(ctx);
      const win = ui?.pets.buffPetStatus();
      const view = charmView(slot);
      if (ui && win && view) ui.tooltip.putItem(win, view, rect, APP_BUFFPET_STATUS);
    },
    askEquip: (objId, visItemId) => {
      askVis('equip', visItemId, () => {
        const cur = inv.item(objId);
        // `SendDoUseItem( MAKELONG( ITYPE_ITEM, m_dwItemId ), m_objid )` with the own object as the target
        if (cur) inv.use.router.useItem(cur, ownId(), -1, true);
      });
    },
    askRemove: (slot, visItemId) => {
      askVis('remove', visItemId, () => {
        senders.removeVis(slot);
      });
    },
    swap: (a, b) => {
      senders.swapVis(a, b);
    },
    acceptsItem: (objId) => {
      const it = inv.item(objId);
      if (it?.extra !== 0) return false;
      return ctx.data?.item(it.itemId)?.item_kind3 === IK3_VIS;
    },
    visItemOf: (objId) => inv.item(objId)?.itemId ?? 0,
    previews: petPreviewPortOf(ctx),
  });

  const toggleBuffStatus = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    if (ui.wm.isOpen(APP_BUFFPET_STATUS)) {
      ui.wm.close(APP_BUFFPET_STATUS);
      return;
    }
    if (visItem()) ui.pets.openBuffPetStatus(buffRequest());
  };

  fc.net.on('visPetActivate', (m) => {
    // `SetEatPetId( objIdVisPetId ); SetVisPetItem( objIdVisPetItem ); if( HasActivatedVisPet() ) open else destroy`
    patchPets(store, {
      eatPetId: m.eatPetObjId === NULL_ID ? null : m.eatPetObjId,
      visPetItem: m.visPetItemObjId === NULL_ID ? null : m.visPetItemObjId,
    });
    const ui = fullUi(ctx);
    if (!ui) return;
    if (visItem()) {
      if (!ui.wm.isOpen(APP_BUFFPET_STATUS)) ui.pets.openBuffPetStatus(buffRequest());
    } else {
      ui.wm.close(APP_BUFFPET_STATUS);
    }
  });

  /** `OnChangeMoverSfx`: the effect of the charms hangs on the mover for good (`CreateSfx( ..., -1 )`); a new id replaces it. */
  const sfxHandles = new Map<number, number>();
  fc.net.on('moverChangeSfx', (m, objId) => {
    const fx = fxRuntime(ctx).fx();
    if (!fx) return;
    const old = sfxHandles.get(objId);
    if (old !== undefined) {
      fx.removeSfx(old);
      sfxHandles.delete(objId);
    }
    if (m.sfxId !== 0 && m.sfxId !== NULL_ID) {
      sfxHandles.set(objId, fx.sfx(m.sfxId, { src: objId, dest: objId, nSec: -1 }));
    }
  });
  ctx.entities.onRemove((rec) => {
    const h = sfxHandles.get(rec.objId);
    if (h !== undefined) {
      fxRuntime(ctx).fx()?.removeSfx(h);
      sfxHandles.delete(rec.objId);
    }
  });

  // ---- the item windows -------------------------------------------------------------------------------------------------------

  const itemShell = (objId: number) => {
    const it = inv.item(objId);
    return it ? inv.itemView(it) : null;
  };
  const reserve = (objId: number, extra: number): void => {
    inv.setExtra(objId, extra);
  };

  /** `APP_PET_MIRACLE`: the chance scroll of the tamer. */
  const openMiracle = (item: InvItem): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    const pet = petItem()?.pet;
    const picks = miraclePicks(pet);
    const grade = (level: number): string => text(GRADE_TID[level] ?? TID_GAME_PET_GRADE_C);
    const level = pet?.level ?? PL_B;
    const request: PetMiracleRequest = {
      scroll: item.objId,
      levelNames: [grade(level - 1), grade(level)],
      picks,
      description: ctx.bundles.get('help')?.panels['petmiracle'] ?? '',
      pickIcon: (n) => `icon/Icon_PetLevel${String(n)}_L.dds`,
      send: (scroll) => {
        senders.petTamerMiracle(scroll);
      },
      tick: () => {
        ctx.audio.cues?.miniGame('tick');
      },
      lock: (on, which) => {
        const status = ui.pets.petStatus();
        const p = petItem()?.pet;
        if (!status || !p) return;
        const lvl = which === 0 ? p.level - 1 : p.level;
        status.lockLevel(which, lvl, on ? (p.availLevel[lvl - 1] ?? 0) : null);
      },
      random: (n) => Math.floor(Math.random() * n),
    };
    ui.pets.openMiracle(request);
  };

  /** `APP_PET_FOODMILL`: the NPC (`tool = null`) or the feed-maker item. */
  const openFoodMill = (tool: InvItem | null): boolean => {
    const ui = fullUi(ctx);
    if (!ui) return false;
    // `CreateApplet( APP_INVENTORY )`: the grinder is fed from the bag
    if (!ui.wm.isOpen(APP_INVENTORY)) ui.wm.open(APP_INVENTORY);
    const request: FoodMillRequest = {
      description: text(TID_GAME_PETFOODMILL_DESC),
      tool: tool === null ? null : tool.objId,
      item: itemShell,
      reserve,
      askCount: (objId, onCount) => {
        const it = inv.item(objId);
        if (!it) return;
        const confirm: FoodConfirmRequest = {
          parent: 2,
          count: it.num,
          question: text(TID_GAME_ITEMCOUNT_QUESTION),
          title: text(TID_GAME_ITEMCOUNT_CONFIRM),
          confirm: onCount,
        };
        ui.pets.openFoodConfirm(confirm);
      },
      countText: (n) => formatTid(fc.texts, TID_GAME_KWAIBAWIBO_PRESENT_NUM, [n]),
      accept: (objId) => {
        const it = inv.item(objId);
        const prop = it ? ctx.data?.item(it.itemId) : undefined;
        // `IK3_GEM`, not charged, `dwCost > 0`
        if (it && prop?.item_kind3 === IK3_GEM && !it.charged && prop.cost > 0) return true;
        say(TID_GAME_NOTFOOD);
        return false;
      },
      send: (material, count, t) => {
        senders.makePetFeed(material, count, t);
      },
      result: (result, count) => {
        if (result === 0) say(TID_GAME_LACKSPACE);
        else if (result === 1) say(TID_GAME_PETFEED_MAKE, [count]);
      },
      pocketRefused: () => {
        box(text(TID_GAME_ERROR_FOOD_MILL_POCKET));
      },
    };
    return ui.pets.openFoodMill(request) !== null || ui.wm.isOpen(APP_PET_FOODMILL);
  };

  /** `APP_PET_TRANS_EGGS`: ten eggs. */
  const openTransEggs = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    if (ui.wm.isOpen(APP_PET_TRANS_EGGS)) {
      say(TID_GAME_WND_OVERLAPED);
      return;
    }
    if (!ui.wm.isOpen(APP_INVENTORY)) ui.wm.open(APP_INVENTORY);
    const eggIcon = iconOf(II_PET_EGG);
    const request: TransEggsRequest = {
      description: ctx.bundles.get('help')?.panels['pettranseggs'] ?? '',
      eggIcon,
      item: itemShell,
      reserve,
      accept: (objId) => {
        const it = inv.item(objId);
        if (!it) return null;
        // the summoned pet (and every item in use) is refused with the first text
        if ((it.flag & 0x04) !== 0 || objId === store.getState().summonedItem) {
          say(TID_GAME_TRANS_EGGS_ERROR2);
          return null;
        }
        const prop = ctx.data?.item(it.itemId);
        const egg = prop?.item_kind3 === IK3_EGG && (!it.pet || it.pet.level === PL_EGG);
        if (!egg) {
          say(TID_GAME_TRANS_EGGS_ERROR1);
          return null;
        }
        // `IsUsableItem`: nothing of the stack is reserved
        return it.extra === 0 ? itemShell(objId) : null;
      },
      send: (ids) => {
        senders.transformItem(ids);
      },
    };
    ui.pets.openTransEggs(request);
  };
  inv.registerFeed(APP_PET_TRANS_EGGS, (item) => {
    const win = fullUi(ctx)?.pets.transEggs();
    return win?.setItem(item.objId) ?? false;
  });

  /** `APP_PET_RES`: the pet exchange NPC. */
  const openPetRes = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    if (ui.wm.isOpen(APP_PET_RES)) {
      say(TID_GAME_WND_OVERLAPED);
      return;
    }
    const request: PetResRequest = {
      description: text(TID_GAME_PETTRADE),
      item: itemShell,
      reserve: (objId, on) => {
        const it = inv.item(objId);
        if (it) inv.setExtra(objId, on ? it.extra + 1 : 0);
      },
      accept: (objId) => {
        const it = inv.item(objId);
        const pet = it?.pet;
        const refuse = (): boolean => {
          sayRed(TID_GAME_PETTRADE_ERROR);
          return false;
        };
        if (!it || !pet) return refuse();
        if (objId === store.getState().summonedItem || it.extra !== 0) return refuse();
        if (pet.level < PL_B || pet.level > 5) return refuse();
        if ((it.flag & 0x01) !== 0) return refuse();
        return true;
      },
      send: (objId) => {
        senders.quePetResurrection(objId);
      },
    };
    ui.pets.openPetRes(request);
  };
  const sayRed = (tid: number): void => {
    fullUi(ctx)?.chat.putString(text(tid), null, 0xffff0000, 0x600);
  };

  /** `CWndPetLifeConfirm` (`TID_GAME_PET_USELIFE`): Yes uses the life scroll. */
  const petLifeConfirm = (item: InvItem): void => {
    fullUi(ctx)?.itemUse.openFunnyCoinConfirm({
      text: text(TID_GAME_PET_USELIFE),
      yes: () => {
        const cur = inv.item(item.objId);
        if (cur) inv.use.router.useItem(cur, NULL_ID, -1, false);
      },
    });
  };

  inv.registerDispatchWindows({
    petMiracle: openMiracle,
    petFoodMill: (tool) => openFoodMill(tool),
    petLifeConfirm,
    hasActivatedVisPet: () => store.getState().visPetItem !== null,
  });
  inv.registerDispatchEnv({ hasPet: () => petItem()?.pet !== undefined });

  // ---- the NPC menus and the chat commands --------------------------------------------------------------------------------------

  const menu = menuOf(ctx);
  menu?.registerAction(MMI_PET_FOODMILL, () => {
    const ui = fullUi(ctx);
    if (!ui) return;
    if (ui.wm.isOpen(APP_PET_FOODMILL)) {
      say(TID_GAME_NOWUSING);
      return;
    }
    openFoodMill(null);
  });
  menu?.registerAction(MMI_PET_STATUS, toggleStatus);
  menu?.registerAction(MMI_PET_RELEASE, () => {
    senders.petRelease();
    fullUi(ctx)?.wm.close(APP_PET_STATUS);
  });
  menu?.registerAction(MMI_PET_RES, openPetRes);
  menu?.registerAction(MMI_PET_EGG01, openTransEggs);
  menu?.registerAction(MMI_BUFFPET_STATUS, toggleBuffStatus);
  menu?.registerAction(MMI_RENAME_CANCEL, () => {
    senders.clearPetName();
  });
  const commands = localCommandsOf(ctx);
  commands.set(105, () => {
    senders.clearPetName();
  });
  commands.set(93, () => {
    const ui = fullUi(ctx);
    if (ui && !ui.wm.isOpen(APP_PET_FOODMILL)) openFoodMill(null);
  });

  // ---- the filter --------------------------------------------------------------------------------------------------------------

  const filter = (): number => fc.options.getState().game.petFilter >>> 0;
  const applyFilter = (mask: number): void => {
    setOptionByPath(fc.options, 'game.petFilter', mask >>> 0);
    senders.petFilter(mask);
    stats.filterSent += 1;
  };
  const filterPort: PetFilterPort = { current: filter, apply: applyFilter };
  /** The UI exists at the boot step of the modules in the game and later in some tests: the port is installed as soon as it is there. */
  let portInstalled: unknown = null;
  const installPort = (): void => {
    const ui = fullUi(ctx);
    if (!ui || portInstalled === ui) return;
    portInstalled = ui;
    ui.pets.setFilterPort(filterPort);
  };
  installPort();
  installPortLater = installPort;

  // 45 §7.12 step 5: after the join batch the saved mask goes to the server once
  fc.net.on('addGameJoin', () => {
    installPort();
    if (!ctx.features.features.pet_filter) return;
    const mask = filter();
    if (mask === 0) return;
    senders.petFilter(mask);
    stats.filterSent += 1;
  });

  const feature: PetsFeature = {
    store,
    senders,
    eggPets,
    petItem,
    filter,
    applyFilter,
    toggleStatus,
    stats,
  };
  features.set(ctx, feature);
  addProbeMember(ctx, 'pets', () => {
    const s = store.getState();
    const pet = petItem()?.pet;
    const own = ownId();
    return {
      summonedItem: s.summonedItem,
      eatPetId: s.eatPetId,
      visPetItem: s.visPetItem,
      pet: pet
        ? {
            kind: pet.kind,
            level: pet.level,
            exp: pet.exp,
            energy: pet.energy,
            life: pet.life,
            name: pet.name,
          }
        : null,
      localPet: eggPets.localIdOf(own) ?? null,
      locals: { ...eggPets.stats },
      filter: filter(),
      stats: { ...stats },
    };
  });
  return feature;
}
