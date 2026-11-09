// `features/events/guildHouse`: the small guild house (the module is registered by `features/houseM7.ts`).
export { guildHouseOf, registerGuildHouse } from './guildHouseModule';
export type { GuildHouseFeature } from './guildHouseModule';
export { GH_OP, initialGuildHouse, NULL_OBJ } from './slice';
export type { GhHouse, GhRow, GuildHouseSlice } from './slice';
