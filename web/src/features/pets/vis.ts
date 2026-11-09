// The prerequisites of the vis charms of a buff pet (`CMover::GetValidVisTable`, `src:_Common/MoverSkill.cpp` / `Mover.cpp`; the server twin is
// `flyff_game::item::pets::vis::valid_table`; WP-CFEAT-27, WP-UI-34): a charm works only while the charms it names (`refer_target1/2`) are socketed in the same pet
// and work themselves. The check is recursive; a socket under evaluation counts as failed (a cycle in the data fails instead of looping, as on the server).

/** `UNDEFINED_NEEDVIS` ... `FAILED_BOTH_NEEDVIS`. */
export const NeedVis = {
  Undefined: 0,
  Success: 1,
  Failed1st: 2,
  Failed2nd: 3,
  FailedBoth: 4,
} as const;
export type NeedVis = (typeof NeedVis)[keyof typeof NeedVis];

const NULL_ID = 0xffff_ffff;

/** The two prerequisites of a charm item (`dwReferTarget1/2`), `undefined` = the item is unknown. */
export type VisNeeds = (itemId: number) => readonly [number, number] | undefined;

/** `GetValidVisTable( pet )`: the state of every socket (`cards[i]` is the charm item id, `0` = empty). */
export function validTable(cards: readonly number[], needs: VisNeeds): NeedVis[] {
  const v: NeedVis[] = cards.map(() => NeedVis.Undefined);
  cards.forEach((c, i) => {
    if (c === 0) v[i] = NeedVis.FailedBoth;
  });
  for (let i = 0; i < cards.length; i += 1) {
    if (v[i] === NeedVis.Undefined) decide(cards, needs, i, v);
  }
  return v;
}

/** `SetValidNeedVis( item, pos, valid )`. */
function decide(cards: readonly number[], needs: VisNeeds, pos: number, valid: NeedVis[]): boolean {
  const cur = valid[pos];
  if (cur !== undefined && cur !== NeedVis.Undefined) return cur === NeedVis.Success;
  const n = needs(cards[pos] ?? 0);
  if (!n) {
    valid[pos] = NeedVis.FailedBoth;
    return false;
  }
  const [t1, t2] = n;
  if (t1 === NULL_ID && t2 === NULL_ID) {
    valid[pos] = NeedVis.Success;
    return true;
  }
  // while this socket is being decided it must not satisfy itself
  valid[pos] = NeedVis.FailedBoth;
  let r1 = t1 === NULL_ID;
  let r2 = t2 === NULL_ID;
  for (let i = 0; i < cards.length; i += 1) {
    if (t1 === cards[i]) r1 = decide(cards, needs, i, valid);
    if (t2 === cards[i]) r2 = decide(cards, needs, i, valid);
    if (r1 && r2) {
      valid[pos] = NeedVis.Success;
      return true;
    }
  }
  valid[pos] = r1 && !r2 ? NeedVis.Failed2nd : !r1 && r2 ? NeedVis.Failed1st : NeedVis.FailedBoth;
  return false;
}
