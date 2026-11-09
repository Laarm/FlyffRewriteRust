// `game/pets/` (Plan/40 §7.9, WP-CLIENT-31): the local system pets (`CAIEgg`). The sim drives them (`flyff_sim::client::local_pet`); `eggPet.ts` builds and removes their
// movers, `petData.ts` is the pet tables (`CPet`, `CPetProperty`).
export {
  createEggPets,
  eggPetsOf,
  LOCAL_ID_BASE,
  LOCAL_ID_END,
  type EggPetStats,
  type EggPets,
  type LocalPetSpec,
} from './eggPet';
export * from './petData';
