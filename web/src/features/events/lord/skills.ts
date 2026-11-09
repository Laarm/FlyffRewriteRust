// The lord skills of the client (Plan/45 §28.2, §19.4; WP-CFEAT-43; `src:_Interface/WndManager.cpp:2961-2998`, `_Interface/WndTaskBar.cpp:446-451, 617-637, 2944-2951`, `Neuz/clord.cpp` `DoEndVoteComplete`): the
// decision of `lord.useSkill( id )` (what a click on a LORDSKILL slot or a double click in the skill window does), the pieces of the taskbar source (icon, cooldown radar, tooltip) and the clean-up
// of the lord shortcuts when the lordship ends. Pure: the module gathers the facts and carries out the decision.
//
// OQ7 (KEEP-QUIRK): the skill window drags `index + 1` so that skill 0 never looks empty; the taskbar stores `id - 1` (`ui/taskbar/drop.ts` `slotShortcut`) and adds one on a drag out
// (`pickupShortcut`). The id a slot stores, persists and executes is therefore the `LI_*` id 0..7, which is what `useSkill` and the taskbar ports below take.

import { type LordSkillRow, LI_SUMMON, LI_TELEPORT } from '../../../ui/windows/lord/ports';

/** What `GetObjFocus()` is. */
export type LordFocus =
  /** No focus: the skill goes to the own character. */
  | { readonly kind: 'none' }
  /** A player mover: the skill goes to its name. */
  | { readonly kind: 'player'; readonly name: string }
  /** An NPC, a monster, an item or a control: nothing is sent (`WndManager.cpp:2986-2995`). */
  | { readonly kind: 'other' };

export interface UseSkillEnv {
  /** The skill exists in `lordskill.txt`. */
  hasSkill(id: number): boolean;
  /** `GetTick()`: the minutes left of the cooldown. */
  tick(id: number): number;
  focus(): LordFocus;
  /** `g_pPlayer->GetName()`. */
  selfName(): string;
}

export type UseSkillDecision =
  /** Unknown skill, or a focus that is not a player: nothing happens. */
  | { readonly kind: 'nothing' }
  /** SUMMON / TELEPORT at tick 0: the name prompt (`CWndLordSkillConfirm`). */
  | { readonly kind: 'confirm'; readonly skill: number }
  /** SUMMON / TELEPORT while the cooldown runs: `TID_GAME_STILLNOTUSE`. */
  | { readonly kind: 'stillNotUse' }
  /** `SendLordSkillUse( skill, name )`. */
  | { readonly kind: 'send'; readonly skill: number; readonly name: string };

/**
 * `ObjectExecutor( SHORTCUT_LORDSKILL )`. FIX: an unknown id does nothing (the original dereferenced the missing component for 5 and 6 and sent any other id). There is no client check of the
 * lordship or of the cooldown of the other skills: the server answers `TID_GAME_LORD_SKILL_USE_E001` / `_E005`.
 */
export function decideUseSkill(id: number, env: UseSkillEnv): UseSkillDecision {
  if (!env.hasSkill(id)) return { kind: 'nothing' };
  if (id === LI_SUMMON || id === LI_TELEPORT) {
    return env.tick(id) === 0 ? { kind: 'confirm', skill: id } : { kind: 'stillNotUse' };
  }
  const focus = env.focus();
  switch (focus.kind) {
    case 'none':
      return { kind: 'send', skill: id, name: env.selfName() };
    case 'player':
      return { kind: 'send', skill: id, name: focus.name };
    case 'other':
      return { kind: 'nothing' };
  }
}

/** `RenderRadar( cool - tick, cool )` in the clock of the UI: the sweep spans the cooldown; `at` is the `ctx.now()` of the last tick message (the original stepped per minute). */
export function cooldownSpan(
  skill: Pick<LordSkillRow, 'tick' | 'cooltime'>,
  at: number,
): { startMs: number; endMs: number } | null {
  if (skill.tick <= 0) return null;
  return {
    startMs: at - (skill.cooltime - skill.tick) * 60_000,
    endMs: at + skill.tick * 60_000,
  };
}

/** The two numbers of `TID_TOOLTIP_COOLTIME( tick, 0 )` ("%.2d:%.2d"): both padded to two digits (the text formatter ignores widths). */
export function cooltimeArgs(tick: number): [string, string] {
  return [String(Math.trunc(tick)).padStart(2, '0'), '00'];
}

/**
 * The tooltip of a LORDSKILL slot (`WndTaskBar.cpp:617-637`): `"#b#cff2fbe6d<name>#nb#nc\n<desc>"` (no blank before the line break, unlike the window) and, while the cooldown runs, a line
 * `TID_TOOLTIP_COOLTIME( tick, 0 )` ("Waiting Time for reuse: 10:00"; KEEP: minutes and `:00`). `cooltime` is that line for the tick.
 */
export function slotTooltipText(
  skill: Pick<LordSkillRow, 'name' | 'desc' | 'tick'>,
  cooltime: (tick: number) => string,
): string {
  const head = `#b#cff2fbe6d${skill.name}#nb#nc\n${skill.desc}`;
  return skill.tick !== 0 ? `${head}\n${cooltime(skill.tick)}` : head;
}

/** The text of the cooldown line for `tick` minutes from the template `TID_TOOLTIP_COOLTIME`. */
export function cooltimeLine(template: string, tick: number): string {
  const [m, s] = cooltimeArgs(tick);
  let i = 0;
  return template.replace(/%(?:%|[-+ #0]*\d*(?:\.\d+)?[dsu])/g, (c) => {
    if (c === '%%') return '%';
    const a = i === 0 ? m : s;
    i += 1;
    return a;
  });
}

/** A slot of the taskbar rows that holds a lord skill. */
export interface LordSlotRef {
  readonly row: 'item' | 'applet';
  readonly page: number;
  readonly slot: number;
}

/**
 * `DoEndVoteComplete`: every item slot (8 pages x 9) and then every applet slot (18) of kind LORDSKILL, in the order of the original. `isLordSlot( row, page, slot )` tells whether the model
 * holds a lord shortcut there.
 */
export function lordSlots(
  isLordSlot: (row: 'item' | 'applet', page: number, slot: number) => boolean,
  pages = 8,
  perPage = 9,
  applets = 18,
): LordSlotRef[] {
  const out: LordSlotRef[] = [];
  for (let page = 0; page < pages; page += 1) {
    for (let slot = 0; slot < perPage; slot += 1) {
      if (isLordSlot('item', page, slot)) out.push({ row: 'item', page, slot });
    }
  }
  for (let slot = 0; slot < applets; slot += 1) {
    if (isLordSlot('applet', 0, slot)) out.push({ row: 'applet', page: 0, slot });
  }
  return out;
}
