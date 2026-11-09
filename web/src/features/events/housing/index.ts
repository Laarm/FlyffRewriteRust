// `features/events/housing`: the personal mini room (the module is registered by `features/houseM7.ts`).
export { housingOf, registerHousing } from './housingModule';
export type { HousingFeature } from './housingModule';
export { initialHousing, NULL_ITEM, WORLD_MINIROOM } from './slice';
export type { HousingRow, HousingSlice } from './slice';
export { computeRoomTextures, NO_ROOM_TEXTURES, roomTexturesOf } from './roomTextures';
export type { RoomTextures } from './roomTextures';
