// The preview subjects of the character screens (Plan/45 §4.1, §5.1; `CDPLoginClient::OnPlayerList`, `CWndCreateChar::SetSex`):
// a `MoverVisualState` (41's input) of a listed character or of the character being created.

import type { MoverVisualState } from '../../game/entities';
import { MI_FEMALE, MI_MALE } from '../../gen/defs/obj';
import { JOB_VAGRANT } from '../../gen/defs/job';
import { PARTS_LWEAPON, PARTS_RWEAPON } from '../../gen/defs/neuz';
import type { LobbySlot } from '../session/lobbyStore';

/** `PARTS_*` -> the equipment record of a `MoverVisualState`. */
export type EquipEntry = MoverVisualState['equip'] extends Map<number, infer V> ? V : never;

/** `NULL_ID`: no item (`0xFFFFFFFF`, or `-1` where a bundle carries it signed). */
export function isNullItem(id: number): boolean {
  return id === 0xffff_ffff || id < 0;
}

/**
 * The legacy placement of the equipment list (`OnPlayerList`): each item goes to the `PARTS_*` slot of its `ItemProp`;
 * an item for the right hand that finds the right hand taken moves the first one to the LEFT hand (the lists come in
 * reverse order, 12 §8.4). Items without a model slot are ignored.
 */
export function placeEquipment(
  ids: readonly number[],
  partsOf: (itemId: number) => number | undefined,
): Map<number, EquipEntry> {
  const equip = new Map<number, EquipEntry>();
  for (const id of ids) {
    if (isNullItem(id)) continue;
    const parts = partsOf(id);
    if (parts === undefined || isNullItem(parts)) continue;
    if (parts === PARTS_RWEAPON) {
      const right = equip.get(PARTS_RWEAPON);
      if (right) equip.set(PARTS_LWEAPON, right);
    }
    equip.set(parts, { itemId: id, flag: 0, option: 0 });
  }
  return equip;
}

/** A `MoverVisualState` of a human with every field defaulted (the visual of a player that nobody has equipped). */
export function playerVisual(
  over: Partial<MoverVisualState> & { id: number; sex: 0 | 1 },
): MoverVisualState {
  return {
    kind: 'player',
    moverIndex: over.sex === 1 ? MI_FEMALE : MI_MALE,
    hairMesh: 0,
    hairColor: 0x00ffffff,
    headMesh: 0,
    skinSet: 0,
    equip: new Map<number, EquipEntry>(),
    hideCoat: [false, false, false, false, false],
    mode: 0,
    authorization: 0,
    disguiseMover: null,
    rideItem: null,
    scale: 1,
    dmgCnt: 0,
    name: '',
    title: '',
    guild: null,
    idWar: 0,
    pk: { chaotic: false, pink: false },
    partyMember: false,
    vendorTitle: null,
    activeAttack: false,
    peaceful: false,
    structure: null,
    ...over,
  };
}

/** The subject of a listed character. */
export function slotSubject(
  slot: LobbySlot,
  partsOf: (itemId: number) => number | undefined,
): MoverVisualState {
  const sex: 0 | 1 = slot.sex === 1 ? 1 : 0;
  return playerVisual({
    id: slot.idPlayer,
    sex,
    moverIndex:
      slot.moverIndex === MI_FEMALE || slot.moverIndex === MI_MALE
        ? slot.moverIndex
        : sex === 1
          ? MI_FEMALE
          : MI_MALE,
    hairMesh: slot.hairMesh,
    hairColor: slot.hairColor,
    headMesh: slot.headMesh,
    skinSet: slot.skinSet,
    equip: placeEquipment(slot.equip, partsOf),
    name: slot.name,
  });
}

/**
 * The starting equipment of a new character (`prj.m_jobItem[JOB_VAGRANT].adwMale[i][sex]`, 14 `jobItem.inc`; the begin
 * items stay server-only): `equip[i] = [male, female]`; every entry goes to its own `PARTS_*` slot (no left-hand shift).
 */
export function starterEquipment(
  rows: readonly {
    readonly job: number;
    readonly equip: readonly (readonly [number, number])[];
  }[],
  sex: 0 | 1,
  partsOf: (itemId: number) => number | undefined,
): Map<number, EquipEntry> {
  const equip = new Map<number, EquipEntry>();
  const row = rows.find((r) => r.job === JOB_VAGRANT);
  if (!row) return equip;
  for (const pair of row.equip) {
    const id = pair[sex];
    if (isNullItem(id)) continue;
    const parts = partsOf(id);
    if (parts === undefined || isNullItem(parts)) continue;
    equip.set(parts, { itemId: id, flag: 0, option: 0 });
  }
  return equip;
}
