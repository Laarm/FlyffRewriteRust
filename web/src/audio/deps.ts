// The narrow views the audio layer reads from the rest of the client (Plan/44 §2.2 `AudioDeps`, §1.4 data consumed). They are
// declared STRUCTURALLY here so `audio/` depends on the shapes it needs and not on the stores, the entity table or the bundle
// index themselves: `ClientDataIndex`, `EntityRecord`, `SelfState` and `WorldState` satisfy them as they are, and the tests build
// small fakes. The wiring (`features/session/audioMount.ts`, `app/hosts.ts audioDeps`) is the only place that knows both sides.

import type { ClientRegion } from '../gen/bundles';
import type { ScreenState } from '../stores/screen';

/** A position in LH world coordinates (01 §8). */
export type Vec3 = readonly [number, number, number];

/** The `items` bundle fields the sound triggers read (44 §1.4). */
export interface AudioItemProp {
  readonly id: number;
  /** `dwSndAttack1`: the melee hit sound (`NULL_ID` = none). */
  readonly snd_attack1: number;
  /** `dwSndAttack2`: the critical hit sound (`NULL_ID` = none). */
  readonly snd_attack2: number;
  /** `dwSubDefine`: the drop sound of a ground item (`__ITEM_DROP_SOUND`). */
  readonly sub_define: number;
  readonly item_kind1: number;
  readonly item_kind2: number;
  readonly item_kind3: number;
  readonly parts: number;
}

/** The `skills` bundle fields the sound triggers read (T09: the skill's own `dwSndAttack1`). */
export interface AudioSkillProp {
  readonly id: number;
  /** `dwSndAttack1`: the sound of the skill's effect (`NULL_ID` = none). */
  readonly snd_attack1: number;
}

/** The `movers` bundle fields the sound triggers read. */
export interface AudioMoverProp {
  /** `RANK_*` (`RANK_BOSS = 4`). */
  readonly class: number;
  /** The virtual attack item of a monster. */
  readonly atk1: number;
  /** `dwSndDmg1..3` (`[1]` is the one `CSfx::DamageToTarget` plays). */
  readonly snd_dmg: readonly number[];
  /** `dwSndIdle1..2` (`[0]` is played when the mover is clicked). */
  readonly snd_idle: readonly number[];
}

/** The part of a `worlds` folder the BGM director reads. */
export interface AudioWorldFolder {
  readonly settings: { readonly bgm: number; readonly indoor: boolean };
  readonly regions: readonly ClientRegion[];
}

/** Lookups over the bundles that are not the two sound tables (`sounds`, `music`). `ClientDataIndex` is assignable. */
export interface AudioData {
  item(id: number): AudioItemProp | undefined;
  /** A skill row of the `skills` bundle (T09); optional for the data ports of the early tests. */
  skill?(id: number): AudioSkillProp | undefined;
  mover(id: number): AudioMoverProp | undefined;
  /** The `sfx-table` row of an `XI_` id: `sound` is the bare file name under `Sound/` (`null` = none). */
  sfx(id: number): { readonly sound: string | null } | undefined;
  /** An NPC definition by key; `music` is the dialog music (`character.inc SetMusic`, 0 = none). */
  npc(key: string): { readonly music: number } | undefined;
  /** The client data of a world folder (`.wld` settings and the region list), `folder` in any letter case. */
  worldFolder(folder: string): AudioWorldFolder | undefined;
  /** Every `MA_SOUND` id of the clips of one model's motion slots (the prefetch of §6.5); optional. */
  motionSounds?(ot: number, index: number): readonly number[];
}

/** The entity record fields of a mover the sound triggers read (`EntityRecord` is assignable). */
export interface AudioMover {
  readonly objId: number;
  /** `EntityKind` (Player 1, Npc 2, Item 3, Ctrl 4, LocalPet 5). */
  readonly kind: number;
  /** MoverProp / ItemProp / CtrlProp id. */
  readonly propId: number;
  readonly isSelf: boolean;
  /** The object state word (`OBJSTA_*`). */
  readonly state: number;
  /** `DelObj` was processed; the record is deleted at the end of the drain. */
  readonly removed: boolean;
  readonly visual?:
    | {
        /** `MODE_*` bits (`TRANSPARENT_MODE = 2`). */
        readonly mode: number;
        /** `PARTS_*` -> the worn item (`PARTS_RWEAPON` is the active hand). */
        readonly equip: ReadonlyMap<number, { readonly itemId: number }>;
      }
    | undefined;
}

/** Read-only views over the stores and the entity table (44 §2.2 `AudioStateReaders`). */
export interface AudioStateReaders {
  screen(): ScreenState;
  /** The own character: `objId` (0 before the join), `level`, `skillPoints`, and the flight level of `SetFxp` (T16; absent = 0). */
  self(): {
    readonly objId: number;
    readonly level: number;
    readonly skillPoints: number;
    readonly flightLevel?: number;
  };
  /** The current world: `worldId` (`WI_*`) and the folder of the `worlds` bundle. */
  world(): { readonly worldId: number; readonly folder: string };
  mover(objId: number): AudioMover | undefined;
  /** The live LH position of a mover (the render view of the sim) into `out`; `false` when it is not known. */
  posOf(objId: number, out: [number, number, number]): boolean;
  /** The own position (render view, else the 4 Hz store value) into `out`; `false` before the join. */
  selfPos(out: [number, number, number]): boolean;
  /** The local player is a participant of a 1:1 guild combat (T13 exception); optional, `false` when unknown. */
  inGuildCombat1to1?(): boolean;
  /**
   * The own quest book (T38, WP-AUDIO-13: the jingles of `CMover::SetQuest` / `RemoveQuest`); optional, absent = the quest jingles stay silent. The observers run in phase Pre, so
   * this is the book BEFORE the message.
   */
  readonly quests?: AudioQuestBook;
}

/** What T38 reads of the quest book (`FindQuest`, `IsCompleteQuest`, `m_nQuestSize`). */
export interface AudioQuestBook {
  /** The quest is an active record. */
  isActive(questId: number): boolean;
  /** The quest id is in the completed list. */
  isCompleted(questId: number): boolean;
  /** The number of active records. */
  activeCount(): number;
}

/** The two client feature switches the triggers consult (`ClientFeatureConfig`, delivery flag C; 44 Open question 11). */
export interface AudioFeatures {
  splashAutoattacks(): boolean;
  betterClockworkAlpha(): boolean;
}

export const NO_FEATURES: AudioFeatures = {
  splashAutoattacks: () => false,
  betterClockworkAlpha: () => true,
};

/** `NULL_ID` of the data files (0xFFFFFFFF): an unset sound id. */
export const NULL_ID = 0xffff_ffff;

/** A sound id from the data that can play: positive and not `NULL_ID`. */
export const isSoundId = (id: number | undefined): id is number =>
  id !== undefined && id > 0 && id !== NULL_ID;
