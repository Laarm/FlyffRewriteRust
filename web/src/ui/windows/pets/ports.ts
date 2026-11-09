// Ports of the pet windows (Plan/42 §19.4 L / M / S, 45 §18.7; WP-UI-33, WP-UI-34; wired by 45 WP-CFEAT-27): each window is opened with ONE request object that
// carries what it shows and the callbacks of its buttons. The decisions behind the callbacks (the sends, the texts, which item may enter which slot) are the
// pet feature's (`features/pets`); the shells keep the control behaviour of the original (`src:_Interface/WndPetSys.cpp`, `WndPetRes.cpp`, `WndField.cpp`
// `CWndPetfilter`).

import type { ViewSource } from '../char/ports';
import type { DialogItem } from '../dialogs/ports';
import type { PreviewPort } from '../login/ports';

// ---- APP_PET_STATUS --------------------------------------------------------------------------------------------------------------

/** One level of the stat strip: the level (`PL_D..PL_S`) and the 1..9 pick of its bonus. */
export interface PetStatusPick {
  readonly level: number;
  readonly pick: number;
}

/** What `CWndPetStatus` shows of the summoned system pet. */
export interface PetStatusView {
  /** The title: the item name, the mover name of a hatched pet, or the pet's own name (`HasName`). */
  readonly title: string;
  /** `TID_GAME_PETGRADE_x` of the level. */
  readonly grade: string;
  /** `"<stat> +<n>"` of the summed bonus, or `TID_GAME_PET_EGG_ABILITY` for an egg / a pet without a bonus. */
  readonly ability: string;
  /** `GetLife()`; `null` with `features.no_pet_life` (the life number, the HP label and the HP gauge go). */
  readonly life: number | null;
  readonly energy: number;
  readonly energyMax: number;
  readonly exp: number;
  readonly expMax: number;
  /** The levels D..current with a pick, in level order (the strip of the extension). */
  readonly picks: readonly PetStatusPick[];
  /** The model the preview shows (`MoverVisualState` of the 41 preview); the shell never looks inside. */
  readonly subject: unknown;
  /** The camera of the preview: the pet kind (`0..6`, the table of `SetPetCamTable`) of a hatched pet, `null` for an egg. */
  readonly camera: number | null;
}

/** `TID_GAME_PET_STATUS_LEVEL / _ABILITY / _HP / _EXP`. */
export interface PetStatusLabels {
  readonly level: string;
  readonly ability: string;
  readonly hp: string;
  readonly exp: string;
}

export interface PetStatusRequest {
  /** The live view (read every process); `null` = the pet is gone. */
  readonly view: ViewSource<PetStatusView | null>;
  readonly labels: PetStatusLabels;
  /** The tooltip of the pick of a level (`"<stat> +<n>"`, red). */
  pickTip(level: number): string;
  /** The preview port with the preset camera tables of the pet kinds; `null` = an empty canvas. */
  readonly previews?: PreviewPort | null;
}

// ---- APP_PET_ITEM ----------------------------------------------------------------------------------------------------------------

/**
 * `CWndFoodConfirm` (272 x 160): "how many?" for the pet food (`nParent 1`, sends nothing in this build: the feed is one `UsePetFeed`) and for the
 * food mill (`nParent 2`: the count of the dropped material). The edit starts at the stack (at most 1000), digits only, clamped to the stack; OK or
 * Enter takes 1..1000, Cancel and the close button end it.
 */
export interface FoodConfirmRequest {
  /** `1` pet food, `2` food mill: only the texts differ (`TID_GAME_PETFOOD_*` / `TID_GAME_ITEMCOUNT_*`). */
  readonly parent: 1 | 2;
  /** `m_nItemNum` of the item. */
  readonly count: number;
  /** The question over the edit. */
  readonly question: string;
  /** The title bar text. */
  readonly title: string;
  /** OK with a count of 1..1000. */
  confirm(count: number): void;
  closed?(): void;
}

// ---- APP_PET_FOODMILL ------------------------------------------------------------------------------------------------------------

/**
 * `CWndPetFoodMill` (208 x 176): one slot (`WIDC_STATIC2`) and a description. A gem dropped from the bag asks the count (`CWndFoodConfirm(2)`); OK sends
 * `MakePetFeed{material, count, tool}` for a gem that may be ground (`IK3_GEM`, not charged, cost > 0), else `TID_GAME_NOTFOOD`. The server answers with
 * `PetMill`; success empties the slot.
 */
export interface FoodMillRequest {
  readonly description: string;
  /** The tool (the `II_SYS_SYS_FEED_MAKER` item) that opened the window, `null` = the NPC. */
  readonly tool: number | null;
  /** The shell of the bag item for the slot; `null` = gone. */
  item(objId: number): DialogItem | null;
  /** `SetExtra` (the count while the item sits in the slot, `0` to release). */
  reserve(objId: number, extra: number): void;
  /** A drop from the bag: `null` = refused (the request prints why), else the count box is asked and `onCount` continues. */
  askCount(objId: number, onCount: (count: number) => void): void;
  /** The text of the count line in the slot (`TID_GAME_KWAIBAWIBO_PRESENT_NUM` with the count). */
  countText(count: number): string;
  /** OK with a slot item: whether it may be ground; refused with `TID_GAME_NOTFOOD`. */
  accept(objId: number): boolean;
  /** `SendMakePetFeed(material, count, tool)`. */
  send(materialObjId: number, count: number, tool: number | null): void;
  /** `PetMill{result, count}`: 0 prints `TID_GAME_LACKSPACE`, 1 prints `TID_GAME_PETFEED_MAKE` with the count. */
  result(result: number, count: number): void;
  /** A drop came from a pocket window (`TID_GAME_ERROR_FOOD_MILL_POCKET`). */
  pocketRefused(): void;
  closed?(): void;
}

// ---- APP_PET_MIRACLE -------------------------------------------------------------------------------------------------------------

/** `CWndPetMiracle` (272 x 336): the tamer's chance scroll re-rolls the stat picks of the last two levels of a B / A / S pet. */
export interface PetMiracleRequest {
  /** The scroll (`SetItem( dwObjId )`). */
  readonly scroll: number;
  /** `"Lv. C"` / `"Lv. B"` ... of the two compared levels. */
  readonly levelNames: readonly [string, string];
  /** The picks (1..9) of the two levels at the start, or `null` when the pet does not qualify (the window closes at once). */
  readonly picks: readonly [number, number] | null;
  /** The description panel (`PetMiracle.inc`). */
  readonly description: string;
  /** The level icons `Icon_PetLevel1_L.dds` ... `9_L` (index 0..8). */
  readonly pickIcon: (pick: number) => string;
  /** Chance: `SendPetTamerMiracle(scroll)`. */
  send(scroll: number): void;
  /** The animation ticks `InfOpen.wav` (T39). */
  tick(): void;
  /** The status window is told to keep showing the old picks while the roll runs (`LockShowLevel`). */
  lock(on: boolean, which: 0 | 1): void;
  /** `rand() % 9`. */
  random(n: number): number;
  closed?(): void;
}

// ---- APP_PET_TRANS_EGGS ----------------------------------------------------------------------------------------------------------

/** `CWndPetTransEggs` (240 x 320): ten egg slots; OK is on when all are full and sends `TransformItem{0, 10 eggs}`. */
export interface TransEggsRequest {
  readonly description: string;
  /** The egg icon drawn into every filled slot (`II_PET_EGG`). */
  readonly eggIcon: string | null;
  item(objId: number): DialogItem | null;
  reserve(objId: number, extra: number): void;
  /** What a drop may do: `null` = refused (the request prints `TID_GAME_TRANS_EGGS_ERROR1/2`), else the shell of the egg. */
  accept(objId: number): DialogItem | null;
  /** OK with the ten eggs. */
  send(objIds: readonly number[]): void;
  closed?(): void;
}

// ---- APP_PET_RES -----------------------------------------------------------------------------------------------------------------

/** `CWndPetRes` (208 x 208): a B..S pet (not the summoned one, not expired) becomes the resurrection fragment. */
export interface PetResRequest {
  readonly description: string;
  item(objId: number): DialogItem | null;
  reserve(objId: number, on: boolean): void;
  /** The drop rules (prints `TID_GAME_PETTRADE_ERROR` for a refusal). */
  accept(objId: number): boolean;
  /** OK: `SendQuePetResurrection(item)`. */
  send(objId: number): void;
  closed?(): void;
}

// ---- APP_BUFFPET_STATUS ----------------------------------------------------------------------------------------------------------

/** One vis socket of the buff pet. */
export interface BuffPetSlot {
  /** The charm in the socket; `0` = empty. */
  readonly vis: number;
  /** The icon file of the charm (`item/itm_x.dds`), `null` for an empty socket. */
  readonly icon: string | null;
  /** The prerequisites hold (`SUCCSESS_NEEDVIS`): an unsatisfied charm is drawn dim (alpha `0x4f`). */
  readonly valid: boolean;
}

/** What `CWndBuffPetStatus` shows of the activated vis pet. */
export interface BuffPetView {
  /** The name of the eat pet (the title). */
  readonly title: string;
  /** `GetPiercingSize()` of the vis-pet item: the sockets 0..n-1 are usable, the rest show a lock. */
  readonly size: number;
  /** Up to nine sockets. */
  readonly slots: readonly BuffPetSlot[];
  /** The model the preview shows (the eat pet mover); `null` while it is not in view. */
  readonly subject: unknown;
}

export interface BuffPetRequest {
  readonly view: ViewSource<BuffPetView | null>;
  /** The item tooltip over a charm (name, remaining time): the feature's tooltip service call. */
  tooltip(slot: number, rect: { l: number; t: number; r: number; b: number }): void;
  /** A charm from the bag dropped on a free socket: the confirm window (`CVS_EQUIP_VIS`); `visItemId` is the charm's item id. */
  askEquip(objId: number, visItemId: number): void;
  /** A double click on a socket with a charm: the confirm window (`CVS_UNEQUIP_VIS`). */
  askRemove(slot: number, visItemId: number): void;
  /** A charm dragged from one socket to another: `SendSwapVis(a, b)`. */
  swap(a: number, b: number): void;
  /** Whether an inventory item may be dropped: usable and a vis charm (`IsVis()`). */
  acceptsItem(objId: number): boolean;
  /** The charm id of an inventory item (for the confirm question). */
  visItemOf(objId: number): number;
  readonly previews?: PreviewPort | null;
  closed?(): void;
}

// ---- APP_PETFILTER ---------------------------------------------------------------------------------------------------------------

/** `CWndPetfilter`: twelve check boxes (a set bit means SKIP, Plan/19 §6.8); OK persists the mask and sends `PetFilter`. */
export interface PetFilterPort {
  /** The saved mask (`g_Option.m_dwPetfilter`). */
  current(): number;
  /** OK: `g_Option.m_dwPetfilter = mask; Save; SendPlayerPetfilter( mask )`. */
  apply(mask: number): void;
}
