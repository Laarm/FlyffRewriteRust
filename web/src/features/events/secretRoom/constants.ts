// Constants of the secret-room feature that the reducers, the board and the quick window share (a module of its own so that none of them imports another).

/** `SRCONT_WAR`: the war of a continent is on from this state. */
export const SR_WAR_STATE = 103;

/** The war states of a guild (`SecretRoomInfo.WarState.war_state`): FAILED, NORMAL, MIDBOSS, BOSS, WIN. */
export const SR_WAR = { Failed: 0, Normal: 1, MidBoss: 2, Boss: 3, Win: 4 } as const;
