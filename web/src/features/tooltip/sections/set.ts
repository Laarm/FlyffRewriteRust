// Section 16 of the item tooltip: the named armour set the item belongs to (Plan/45 §10.2; `PutSetItemOpt`,
// `src:_Interface/WndManager.cpp:5035-5125`). The summary comes from `numbers.set`: the worn count, the members with their worn
// state and the active effects.

import { TID_TOOLTIP_SET } from '../../../gen/defs/text';
import { dstSetLine } from '../dst';
import type { ComposeCx } from '../model';

export function putSetItemOpt(cx: ComposeCx): void {
  const set = cx.numbers?.set;
  if (!set) return;
  const { env, b, colors } = cx;
  b.add(`\n\n${set.name} (${String(set.worn)}/${String(set.total)})`, colors.setName);
  for (const member of set.members) {
    const prop = env.prop(member.itemId);
    if (!prop) continue;
    b.add(`\n   ${prop.name}`, member.worn ? colors.setItem1 : colors.setItem0);
  }
  const label = env.text(TID_TOOLTIP_SET);
  for (const [dst, adj] of set.activeDsts) {
    b.add(dstSetLine(dst, adj, label, env.text), colors.setEffect);
  }
}
