// The item and penya actions on a character (Plan/25 section 4.5): remove an item or reduce a stack, adjust the penya of the wallet or a
// bank, return a pet to its egg or revive it. Each asks for a reason (and a typed confirmation for precious items or big amounts) and
// goes through `runConfirmed`; the server checks the serial and recomputes every rule. A cell that no longer holds the serial the
// console saw answers `stale_target`.
import { useState } from 'react';
import { runConfirmed } from '../../app/mutate';
import { Field, NumberInput } from '../../components';
import type { ItemElem } from '../../../gen/proto/flyff/v1/item_types_pb';
import { adjustGold, petAction, removeItem } from './api';
import { isPrecious, locOf, type ContainerModel } from './containers';
import { formatNumber } from './format';
import { T } from './strings';

function CountField({
  max,
  label,
  initial,
  onValue,
}: {
  readonly max: number;
  readonly label: string;
  readonly initial: number;
  /** Called with the clamped count whenever the field changes (the caller keeps it for the request). */
  readonly onValue: (n: number) => void;
}) {
  const [text, setText] = useState(String(initial));
  return (
    <Field label={label}>
      <NumberInput
        min={1}
        max={max}
        value={text}
        data-autofocus
        onChange={(e) => {
          setText(e.target.value);
          const n = Math.trunc(Number(e.target.value));
          onValue(Number.isFinite(n) ? Math.min(max, Math.max(1, n)) : 1);
        }}
      />
    </Field>
  );
}

/** Removes `count` pieces of the cell (the whole stack by default). Resolves `true` when the removal ran. */
export async function removeCell(args: {
  readonly characterId: number;
  readonly characterName: string;
  readonly container: ContainerModel;
  readonly slot: number;
  readonly elem: ItemElem;
  readonly itemName: string;
}): Promise<boolean> {
  const { elem } = args;
  const count = { current: Math.max(1, elem.num) };
  const out = await runConfirmed({
    title: T.dialogs.removeTitle(args.itemName, args.characterName),
    message:
      elem.num > 1 ? (
        <CountField
          max={elem.num}
          label={T.dialogs.removeCount}
          initial={count.current}
          onValue={(n) => {
            count.current = n;
          }}
        />
      ) : undefined,
    confirmLabel: T.items.remove.replace('...', ''),
    danger: true,
    reason: 'required',
    facts: {
      irreversible: isPrecious(elem),
      targetName: args.characterName,
      items: count.current,
    },
    run: (given) =>
      removeItem(
        args.characterId,
        {
          container: locOf(args.container),
          slot: args.slot,
          serial: Number(elem.serial),
          count: count.current,
        },
        given,
      ),
    success: (r) => T.dialogs.removeDone(r.summary),
  });
  return out.ok;
}

/** Changes the penya of the wallet (inventory) or a bank by `delta` (negative removes). */
export async function changePenya(args: {
  readonly characterId: number;
  readonly characterName: string;
  readonly container: ContainerModel;
}): Promise<boolean> {
  const delta = { current: 0 };
  const out = await runConfirmed({
    title: T.dialogs.goldTitle(args.characterName),
    message: (
      <DeltaField
        onValue={(n) => {
          delta.current = n;
        }}
      />
    ),
    confirmLabel: T.items.adjustPenya.replace('...', ''),
    danger: true,
    reason: 'required',
    facts: { targetName: args.characterName },
    run: (given) => {
      if (delta.current === 0) {
        throw new Error(T.dialogs.goldZero);
      }
      return adjustGold(
        args.characterId,
        { container: locOf(args.container), delta: delta.current },
        given,
      );
    },
    success: (r) => T.dialogs.goldDone(r.summary),
  });
  return out.ok;
}

function DeltaField({ onValue }: { readonly onValue: (n: number) => void }) {
  const [text, setText] = useState('');
  return (
    <Field
      label={T.dialogs.goldDelta}
      hint={
        text !== '' && Number.isFinite(Number(text))
          ? formatNumber(Math.trunc(Number(text)))
          : undefined
      }
    >
      <NumberInput
        value={text}
        data-autofocus
        onChange={(e) => {
          setText(e.target.value);
          const n = Math.trunc(Number(e.target.value));
          onValue(Number.isFinite(n) ? n : 0);
        }}
      />
    </Field>
  );
}

/** Returns the summoned pet to its egg. */
export async function releasePet(characterId: number, characterName: string): Promise<boolean> {
  const out = await runConfirmed({
    title: T.dialogs.petReleaseTitle(characterName),
    reason: 'required',
    facts: { targetName: characterName },
    run: (given) => petAction(characterId, 'release', given),
    success: (r) => T.dialogs.petDone(r.summary),
  });
  return out.ok;
}

/** Revives the dead pet item of an inventory cell. */
export async function revivePet(
  characterId: number,
  characterName: string,
  slot: number,
  elem: ItemElem,
): Promise<boolean> {
  const out = await runConfirmed({
    title: T.dialogs.petReviveTitle(characterName),
    reason: 'required',
    facts: { targetName: characterName },
    run: (given) =>
      petAction(characterId, { revive: { slot, serial: Number(elem.serial) } }, given),
    success: (r) => T.dialogs.petDone(r.summary),
  });
  return out.ok;
}
