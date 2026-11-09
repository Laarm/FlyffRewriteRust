// `features/events/rainbow`: the rainbow race (the module is registered by `features/rainbowM7.ts`).
export { rainbowOf, registerRainbow } from './rainbowModule';
export type { RainbowFeature } from './rainbowModule';
export { initialRainbow, racingOf } from './state';
export type { RainbowSlice } from './state';
export { EVENT_RAINBOW_RACE, RAINBOW_MENU_IDS } from './menu';
