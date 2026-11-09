// `features/character` (WP-CFEAT-11): the character window's ports and the stat allocation.
export { characterModule, characterOf, type CharacterFeature } from './window/characterModule';
export { createCharacterView } from './window/view';
export {
  atkExtrasOf,
  derivedForWindow,
  shownAttack,
  shownAttackSpeed,
  SM_ATTACK_UP,
  SM_ATTACK_UP1,
  unflattenDerived,
} from './window/derived';
export { sendModifyStatus } from './statAlloc/senders';
export { corePreviewPortOf } from './window/previews';
