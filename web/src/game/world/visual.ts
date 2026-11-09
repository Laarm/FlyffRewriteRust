// `MoverVisualState` builders (Plan/41 section 12.1, Plan/40 section 6.3): what the spawn handlers know about how a mover
// looks, built from `SpawnSelf` / `SpawnPlayer` / `SpawnNpc`. The renderer reads it per frame; the feature handlers of 45
// keep it current (guild, party, vendor title, ...).

import type { MoverVisualState } from '../entities';
import type { ClientDataIndex } from '../../data/clientData';
import { BELLI_PEACEFUL } from '../../gen/defs/attribute';
import { AII_EGG, AII_MONSTER, AII_PET, PARTS_RIDE } from '../../gen/defs/neuz';
import { MI_FEMALE } from '../../gen/defs/obj';
import type {
  MoverCommon,
  NpcPart,
  ObjCommon,
  PlayerOther,
  PlayerPublic,
  PlayerSelf,
} from '../../gen/proto/flyff/v1/mover_types_pb';

/** The data the visual builders consult; a bundle that is not loaded gives the defaults. */
export type VisualData = Pick<ClientDataIndex, 'mover' | 'npc'>;

type Equip = MoverVisualState['equip'];

/** `0x00RRGGBB` of an ARGB word (the renderer ignores the alpha byte). */
export function rgbOf(argb: number): number {
  return argb & 0x00ff_ffff;
}

function hideCoat(p: PlayerPublic): MoverVisualState['hideCoat'] {
  const h = p.hideCoat;
  return [h[0] === true, h[1] === true, h[2] === true, h[3] === true, h[4] === true];
}

function scaleOf(obj: ObjCommon | undefined): number {
  const x100 = obj?.scaleX100 ?? 100;
  return x100 > 0 ? x100 / 100 : 1;
}

function pkOf(p: PlayerPublic): MoverVisualState['pk'] {
  return { chaotic: p.pkPropensity > 0, pink: p.pkTimeMs > 0 };
}

function playerEquipOther(p: PlayerPublic, other: PlayerOther | undefined): Equip {
  const equip: Equip = new Map();
  for (const e of other?.equip ?? []) {
    equip.set(e.part, { itemId: e.itemId, flag: e.flag, option: p.equipOption[e.part] ?? 0 });
  }
  return equip;
}

function playerEquipSelf(p: PlayerPublic, self: PlayerSelf | undefined): Equip {
  const equip: Equip = new Map();
  const ids = self?.equipItemIds ?? [];
  ids.forEach((itemId, part) => {
    // an empty slot is `NULL_ID` (0xFFFFFFFF) or 0 in the flat array
    if (itemId !== 0 && itemId !== 0xffff_ffff) {
      equip.set(part, { itemId, flag: 0, option: p.equipOption[part] ?? 0 });
    }
  });
  return equip;
}

function rideOf(equip: Equip): number | null {
  const ride = equip.get(PARTS_RIDE);
  return ride ? ride.itemId : null;
}

function base(
  id: number,
  kind: MoverVisualState['kind'],
  obj: ObjCommon | undefined,
  mover: MoverCommon | undefined,
): MoverVisualState {
  return {
    id,
    kind,
    moverIndex: obj?.index ?? 0,
    sex: 0,
    hairMesh: 0,
    hairColor: 0,
    headMesh: 0,
    skinSet: 0,
    equip: new Map(),
    hideCoat: [false, false, false, false, false],
    mode: 0,
    authorization: 0,
    disguiseMover: null,
    rideItem: null,
    scale: scaleOf(obj),
    dmgCnt: 0,
    name: '',
    title: '',
    guild: null,
    idWar: 0,
    pk: { chaotic: false, pink: false },
    partyMember: false,
    vendorTitle: null,
    activeAttack: false,
    peaceful: (mover?.belligerence ?? 0) === BELLI_PEACEFUL,
    structure: null,
  };
}

/** The visual state of a player: `SpawnSelf` (own equipment from `PlayerSelf.equip_item_ids`) or `SpawnPlayer`. */
export function playerVisual(
  id: number,
  obj: ObjCommon | undefined,
  mover: MoverCommon | undefined,
  p: PlayerPublic,
  other: PlayerOther | undefined,
  self: PlayerSelf | undefined,
): MoverVisualState {
  const v = base(id, 'player', obj, mover);
  const equip = self ? playerEquipSelf(p, self) : playerEquipOther(p, other);
  v.moverIndex = obj?.index ?? 0;
  v.sex = p.sex === 1 ? 1 : 0;
  v.hairMesh = p.hairMesh;
  v.hairColor = rgbOf(p.hairColor);
  v.headMesh = p.headMesh;
  v.skinSet = p.skinSet;
  v.equip = equip;
  v.hideCoat = hideCoat(p);
  v.mode = p.mode;
  v.authorization = p.authorization;
  v.rideItem = rideOf(equip);
  v.name = p.name;
  v.level = p.level;
  v.job = p.job;
  v.pk = pkOf(p);
  v.vendorTitle = other && other.vendorTitle !== '' ? other.vendorTitle : null;
  return v;
}

/** `monster` / `npc` / `pet` from the property row and the spawn (a `character_key` makes an NPC). */
export function npcKind(
  moverAi: number | undefined,
  characterKey: string,
): { visual: MoverVisualState['kind']; sim: 'monster' | 'npc' | 'pet' } {
  if (moverAi === AII_PET || moverAi === AII_EGG) return { visual: 'pet', sim: 'pet' };
  if (characterKey !== '') return { visual: 'npc', sim: 'npc' };
  if (moverAi === AII_MONSTER) return { visual: 'monster', sim: 'monster' };
  return { visual: 'monster', sim: 'monster' };
}

/** The visual state of an NPC, monster or pet. */
export function npcVisual(
  id: number,
  obj: ObjCommon | undefined,
  mover: MoverCommon | undefined,
  npc: NpcPart | undefined,
  data: VisualData | null,
): MoverVisualState {
  const prop = data?.mover(obj?.index ?? 0);
  const key = npc?.characterKey ?? '';
  const kind = npcKind(prop?.ai, key).visual;
  const v = base(id, kind, obj, mover);
  const row = key === '' ? undefined : data?.npc(key);
  v.name = row?.name ?? prop?.name ?? '';
  v.sex = v.moverIndex === MI_FEMALE ? 1 : 0;
  v.hairMesh = npc?.hairMesh ?? 0;
  v.hairColor = rgbOf(npc?.hairColor ?? 0);
  v.headMesh = npc?.headMesh ?? 0;
  v.activeAttack = npc?.activeAttack === true;
  for (const e of npc?.equip ?? [])
    v.equip.set(e.part, { itemId: e.itemId, flag: e.flag, option: 0 });
  v.structure = row && row.structure >= 0 ? String(row.structure) : null;
  return v;
}
