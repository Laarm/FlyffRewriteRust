// Skill props at an effective level for the client (Plan/17 section 6.1; `CCtrl::GetSkillProp`, `prj.GetAddSkillProp`): the `ItemProp` of a
// skill (`skills.skills`, 190 rows) and its `AddSkillProp` row (`skills.levels`, the per-level numbers). The row index rule is the
// KEEP-QUIRK of 14 section 6.4: `idx = sub_define + max(level, 1) - 1`, raised by 5 below 5 (`features/tooltip/env.ts` has the same
// function for the tooltips).

import type { ClientContext } from '../../app/context';
import type { ClientItem } from '../../data';
import { RowLookup } from '../../data';
import type { AddSkillProp } from '../../gen/bundles';

/** `idx` of the add-skill row of `sub_define` at `level`. */
export function addSkillIndex(subDefine: number, level: number): number {
  let idx = subDefine + Math.max(level, 1) - 1;
  if (idx < 5) idx += 5;
  return idx;
}

/** `NULL_ID` as a 32-bit unsigned or signed number (the bundles keep either form). */
export function isNullNumber(v: number): boolean {
  return v === 0xffff_ffff || v === -1;
}

/** `CMover::GetSkillLevel` is the server's: the client uses the stored level (`min(level, expert_max)` for the display). */
export function effectiveLevel(level: number, prop: Pick<ClientItem, 'expert_max'>): number {
  return Math.min(level, prop.expert_max > 0 ? prop.expert_max : level);
}

/** The skill lookups of one `ClientContext` (the add-skill index is rebuilt when a bundle arrives). */
export class SkillProps {
  private levels: { version: number; lookup: RowLookup<AddSkillProp> | undefined } | null = null;

  constructor(private readonly ctx: ClientContext) {}

  /** `prj.GetSkillProp(id)`. */
  prop(skillId: number): ClientItem | undefined {
    return this.ctx.data?.skill(skillId);
  }

  /** `prj.GetAddSkillProp(sub_define, level)`. */
  add(prop: Pick<ClientItem, 'sub_define'>, level: number): AddSkillProp | undefined {
    const d = this.ctx.data;
    if (!d) return undefined;
    if (this.levels?.version !== d.version) {
      const table = d.bundle('skills')?.levels;
      this.levels = {
        version: d.version,
        lookup: table ? new RowLookup<AddSkillProp>(table) : undefined,
      };
    }
    return this.levels.lookup?.get(addSkillIndex(prop.sub_define, level));
  }

  /** The add-skill row of skill `skillId` at `level`. */
  addOf(skillId: number, level: number): AddSkillProp | undefined {
    const p = this.prop(skillId);
    return p ? this.add(p, level) : undefined;
  }
}
