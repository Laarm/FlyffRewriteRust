// The random source of the SFX particles (Plan/41 section 16.2, ref/20 section 5.2).
//
//   RANDF = (rand() % 50000) / 50000.0f                                 (sfxbase.cpp:14)
//
// KEEP (a fidelity finding, not in the plan text): the original is a Win32 / MSVC program whose CRT `rand()` returns
// 0..32767 (`RAND_MAX = 0x7FFF`), so `rand() % 50000` is `rand()` itself and RANDF is in [0, 0.65534], never the "0..1" its
// comment promises. Spreads, speeds and spin of every particle effect therefore reach only 65.5 % of their configured range.
// `Rand15` reproduces the CRT generator bit for bit (`seed = seed * 214013 + 2531011`, result `(seed >> 16) & 0x7FFF`), so a
// seeded sequence is the one the original produces after `srand(seed)`; the game seeds it from `Math.random` (cosmetic, not
// synchronised between clients, PL-013 spirit).

/** `RAND_MAX` of the MSVC CRT. */
export const RAND_MAX = 0x7fff;

/** The MSVC CRT `rand()` / `srand()` generator. */
export class Rand15 {
  #seed: number;

  constructor(seed = 1) {
    this.#seed = seed >>> 0;
  }

  /** `srand(seed)`. */
  seed(seed: number): void {
    this.#seed = seed >>> 0;
  }

  /** `rand()`: 0..32767. */
  next(): number {
    this.#seed = (Math.imul(this.#seed, 214013) + 2531011) >>> 0;
    return (this.#seed >>> 16) & 0x7fff;
  }
}

/** A `RANDF` source: `() => (rand() % 50000) / 50000` over a `rand()` function. */
export type Randf = () => number;

export function makeRandf(rand: () => number): Randf {
  return () => Math.fround((rand() % 50000) / 50000);
}

/** A `RANDF` over a seeded CRT generator (tests: reproduces the original sequence of `srand(seed)`). */
export function seededRandf(seed: number): Randf {
  const r = new Rand15(seed);
  return makeRandf(() => r.next());
}

/** A `RANDF` seeded from `Math.random` (the game). */
export function randomRandf(): Randf {
  return seededRandf(Math.floor(Math.random() * 0x1_0000_0000));
}
