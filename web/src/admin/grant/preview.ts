// The live preview of the builder (Plan/25 section 5.2): the EXACT game tooltip of the item that the server normalised. The same
// composer the game uses (`features/tooltip/compose.ts`, a pure function of the item, the viewer, the texts and one batched
// `item_tooltip_numbers` answer of the wasm rules) runs here over the console's data, with a neutral viewer so no requirement line is red.
import { APP_INVENTORY } from '../../gen/defs/resdata';
import { composeItem } from '../../features/tooltip/compose';
import { jobTypeByLayout } from '../../features/tooltip/jobs';
import type {
  ComposeEnv,
  TooltipItemRecord,
  TooltipItemView,
  TooltipOwnerInfo,
} from '../../features/tooltip/model';
import type { ItemTooltipInput, ItemTooltipNumbers, RulesFacade } from '../../rules';
import { splitRandomOptionWord } from '../../rules';
import type { RichRun } from '../../ui/text/markup';
import type { ItemTooltipCtx, UltimateBand } from '../../ui/tooltip/types';
import type { StoredItemDto } from '../gen/dto';
import { wordOf } from './json';
import { NULL_ID } from './catalog';

/** The tooltip lines of an item and its ultimate band. */
export interface PreviewContent {
  readonly runs: readonly RichRun[];
  readonly ultimate: UltimateBand | undefined;
}

/** What the preview needs of the rules (the whole facade, or a fake). */
export type TooltipNumbersFn = (input: ItemTooltipInput) => ItemTooltipNumbers;

/** A viewer for whom no requirement line is coloured: the sex and job of the item (or a vagrant), level 999. */
export function neutralOwner(sex: number, job: number): TooltipOwnerInfo {
  const s = sex === 1 ? 1 : 0;
  const j = job === NULL_ID ? 0 : job;
  return { sex: s, job: j, level: 999, jobType: jobTypeByLayout(j) ?? 0 };
}

/** The tooltip record of a validated item. */
export function recordOf(dto: StoredItemDto): TooltipItemRecord {
  const pet = dto.pet;
  return {
    itemId: dto.item_id,
    num: dto.count,
    flag: dto.flags,
    abilityOption: dto.upgrade_level,
    idGuild: dto.guild_id,
    itemResist: dto.element,
    resistAbilityOption: dto.element_level,
    resistSmItemId: dto.element_stone_id,
    cards: dto.piercing_cards,
    gems: dto.ultimate_gems,
    visExpireAt: dto.vis_keep_until,
    randomOpt: wordOf(dto.random_option),
    keepTimeAt: dto.keep_until,
    itemText: dto.text,
    repair: 0,
    repairNumber: 0,
    ...(pet
      ? {
          pet: {
            kind: pet.kind,
            level: pet.level,
            exp: pet.exp,
            energy: pet.energy,
            life: pet.life,
            availLevel: pet.avail_levels,
            name: pet.name,
          },
        }
      : {}),
  };
}

/** The input of `itemTooltipNumbers` for a record and its viewer. */
export function numbersInput(item: TooltipItemRecord, owner: TooltipOwnerInfo): ItemTooltipInput {
  return {
    itemId: item.itemId,
    abilityOption: item.abilityOption,
    expired: (item.flag & 1) !== 0,
    element: item.itemResist,
    elementLevel: item.resistAbilityOption,
    ...splitRandomOptionWord(item.randomOpt),
    eggLevelC: (item.pet?.level ?? 0) >= 2,
    piercing: item.cards,
    ultimateGems: item.gems,
    owner: { sex: owner.sex, job: owner.job, level: owner.level, rweaponItem: 0 },
    worn: [],
  };
}

/**
 * Composes the tooltip of `dto`; `null` for an item the data does not know. Without `numbers` (the wasm rules are still loading) the
 * lines that need a number (ability range, random options, set) are left out and the rest is the same.
 */
export function composePreview(
  env: ComposeEnv,
  numbers: TooltipNumbersFn | null,
  dto: StoredItemDto,
  nowMs: number,
): PreviewContent | null {
  const prop = env.prop(dto.item_id);
  if (!prop) return null;
  const owner = neutralOwner(prop.item_sex, prop.item_job);
  const item = recordOf(dto);
  const view: TooltipItemView = {
    name: prop.name,
    kind2: prop.item_kind2,
    parts: prop.parts,
    serial: dto.serial === 0 ? `preview-${String(dto.item_id)}` : dto.serial,
    item,
  };
  const answer = numbers ? numbers(numbersInput(item, owner)) : null;
  const ctx: ItemTooltipCtx = {
    flag: APP_INVENTORY,
    now: nowMs,
    numbers: answer
      ? {
          multiplier: answer.multiplier,
          ability: answer.ability,
          equipDenied: answer.equipDenied,
          detail: answer,
        }
      : null,
    extra: { owner },
  };
  const out = composeItem(view, ctx, env);
  if (!out) return null;
  return { runs: out.runs.runs, ultimate: out.ultimate };
}

/** The plain text of a composed tooltip, one string per line (the tests read this). */
export function previewLines(content: PreviewContent): string[] {
  const lines: string[] = [];
  let current = '';
  for (const run of content.runs) {
    run.text.split('\n').forEach((part, i) => {
      if (i > 0) {
        lines.push(current);
        current = '';
      }
      current += part;
    });
  }
  lines.push(current);
  return lines.filter((l) => l !== '');
}

/** The numbers function of a rules facade (or `null` while it is not built). */
export function numbersFn(rules: RulesFacade | null): TooltipNumbersFn | null {
  return rules ? (input) => rules.itemTooltipNumbers(input) : null;
}
