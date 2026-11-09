// The result codes of the `Ultimate{sub, result}` answer (`CUltimateWeapon`, Plan/19 §12.7; `proto/flyff/v1/upgrade.proto`).

/** `ULTIMATE_SUCCESS`. */
export const ULTIMATE_SUCCESS = 0;
/** `ULTIMATE_FAILED`. */
export const ULTIMATE_FAILED = 1;
/** `ULTIMATE_CANCEL`: refused (a stale request, a trade, a bad item). */
export const ULTIMATE_CANCEL = 2;
/** `ULTIMATE_ISULTIMATE`: the weapon is not a general or unique one. */
export const ULTIMATE_ISULTIMATE = 3;
/** `ULTIMATE_INVENTORY`: the bag has no room for the product. */
export const ULTIMATE_INVENTORY = 4;
/** `ULTIMATE_ISNOTULTIMATE`: the weapon is not an ultimate one. */
export const ULTIMATE_ISNOTULTIMATE = 5;
