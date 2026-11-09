// `QuestSubject` over the client stores (Plan/45 §28.1; WP-CFEAT-42): what the display ports of the quest predicates read of the own character - level, job, sex, penya, experience percent,
// PK value, the skill table, the bag, the disguise, the summoned pet, the tutorial state, the campus points, the party, the guild and the quest book. Everything is read at call time, so the
// object is created once. Display only: the server answers every real condition.

import type { ClientContext } from '../../app/context';
import { guildOf } from '../guild/api';
import { inventoryOf } from '../inventory';
import { partyOf } from '../party/partyModule';
import { petsOf } from '../pets/petsModule';
import { relationOf } from '../relation/relationModule';
import { checkSkill as treeCheckSkill, slotOf } from '../skills/tree/model';
import { skillTreeOf } from '../skills/tree/module';
import { playerOf } from '../skills/tree/view';
import { petMaxExp } from '../../game/pets/petData';
import type { QuestBook } from './book';
import type { QuestSubject } from './predicates';

/** What the subject needs beyond the stores. */
export interface SubjectSources {
  /** The quest book now. */
  book(): QuestBook;
  /** `GetTutorialState()` (the last `SpawnSelf`). */
  tutorialState(): number;
}

/** The subject of the own character. */
export function clientSubject(ctx: ClientContext, src: SubjectSources): QuestSubject {
  const self = () => ctx.stores.selfStore.getState();
  const selfRecord = () => ctx.entities.self();
  const disguiseOf = (): number | null => {
    const d = selfRecord()?.visual?.disguiseMover;
    return d ?? null;
  };
  return {
    level: () => self().level,
    job: () => self().job,
    sex: () => self().sex,
    gold: () => self().gold,
    expPercent: () => self().expPercentX100,
    pkValue: () => self().pk.value,
    skillLevel: (skill) => skillTreeOf(ctx)?.levelOf(skill),
    checkSkill: (skill) => {
      const tree = skillTreeOf(ctx);
      if (!tree) return false;
      const table = tree.serverTable();
      const slot = slotOf(table, skill);
      if (slot < 0) return false;
      const s = self();
      return treeCheckSkill(table, slot, tree.env, playerOf(s.job, s.level));
    },
    itemNum: (item) => inventoryOf(ctx)?.countOf(item) ?? 0,
    emptySlots: () => inventoryOf(ctx)?.state().container.emptyCount() ?? 0,
    isDisguised: () => disguiseOf() !== null,
    moverIndex: () => disguiseOf() ?? selfRecord()?.propId ?? 0,
    pet: () => {
      const pet = petsOf(ctx)?.petItem()?.pet;
      if (!pet) return null;
      const max = Math.max(1, petMaxExp(pet.level));
      return { level: pet.level, expPercent: Math.trunc((pet.exp * 100) / max) };
    },
    tutorialState: () => src.tutorialState(),
    campusPoints: () => relationOf(ctx)?.store.getState().campusPoints ?? 0,
    party: () => {
      const p = partyOf(ctx);
      if (!p || self().partyId === 0) return null;
      return { size: p.store.getState().members.length, isLeader: p.isLeader() };
    },
    guild: () => {
      const g = guildOf(ctx);
      if (!g || g.ownGuildId() === 0) return null;
      return { size: g.members().length, isMaster: g.isMaster(self().idPlayer) };
    },
    isActive: (id) => src.book().active.some((r) => r.id === id),
    isComplete: (id) => src.book().completed.includes(id),
    record: (id) => src.book().active.find((r) => r.id === id),
  };
}
