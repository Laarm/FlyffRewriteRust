// `features/events/minigame` (WP-CFEAT-45): the NPC mini-games - Kawibawibo, find word, dice and puzzle: the machines the windows poll, the reducers of the `MiniGame` multiplexer, the menu and the senders.
export { miniGameOf, MINIGAME_CASES, registerMiniGames } from './miniGameModule';
export type { MiniGameFeature } from './miniGameModule';
export { KAWI_RESULT, KawibawiboMachine, kawibawiboYou } from './kawibawibo';
export type { KawiOut, KawiPrizes, KawiReply, KawiWinInfo } from './kawibawibo';
export { DICE_DEFAULTS, DiceMachine, FIVE_RESULT } from './dice';
export type { DiceOut } from './dice';
export { ALPHABET_RESULT, FindWordMachine, isLetterCard, WORD_SLOTS } from './findWord';
export { PUZZLE_SLOTS, PuzzleMachine } from './puzzle';
export type { CardFacts, CardItem } from './cards';
export { MINIGAME_REDUCERS, reassembleLine, reduceMiniGame } from './reducers';
export type { MiniGameEffect, MiniGameEnv, MiniGameMachines } from './reducers';
export { MINIGAME_MENU_IDS, menuTable } from './menu';
export type { MiniGameMenuActions } from './menu';
export { miniGameSenders } from './senders';
export type { MiniGameSenders } from './senders';
