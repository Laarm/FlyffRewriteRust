// The random source of the cosmetic animations (the dice faces, the ladder bars): `rand() % n` of the original as `rng( n )` = an integer in `[0, n)`. Production uses `Math.random`, the tests a scripted list.

/** `rand() % n`. */
export type Rng = (n: number) => number;

/** The production source. */
export const mathRng: Rng = (n) => Math.floor(Math.random() * n);

/** A scripted source for the tests: answers the given values in order (each reduced `% n`), then 0 forever. */
export function scriptedRng(values: readonly number[]): Rng & { readonly calls: number[] } {
  let i = 0;
  const calls: number[] = [];
  const fn = ((n: number): number => {
    calls.push(n);
    const v = values[i] ?? 0;
    i += 1;
    return n > 0 ? v % n : 0;
  }) as Rng & { calls: number[] };
  Object.defineProperty(fn, 'calls', { get: () => calls });
  return fn;
}
