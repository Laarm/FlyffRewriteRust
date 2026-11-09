// The server's verdict on the item being built (`POST /admin/api/items/validate`), asked 300 ms after the last change and cancelled when
// the form changes again. The server is authoritative (Plan/25 section 5.2): the form shows what it says.
import { useEffect, useMemo, useState } from 'react';
import type { ItemIssue, ItemSpec, ValidatedItem } from '../gen/dto';
import { validateItem } from './api';

export const VALIDATE_DEBOUNCE_MS = 300;

export interface Validation {
  /** The last answer for this item (it may be one change behind while `pending`); `null` before the first. */
  readonly result: ValidatedItem | null;
  /** A request is out or waiting for the debounce: `result` is not yet the verdict on the form as it is. */
  readonly pending: boolean;
  /** The request failed (the network, the session); the form stays usable. */
  readonly failure: string | null;
}

/** The first message for `field`: an exact match, or any `field[..]` / `field.x` below it. */
export function errorFor(
  issues: readonly ItemIssue[] | undefined,
  field: string,
): string | undefined {
  const hit = issues?.find(
    (i) => i.field === field || i.field.startsWith(`${field}[`) || i.field.startsWith(`${field}.`),
  );
  return hit?.message;
}

/** Validates `spec` (null = nothing to check). */
export function useValidation(spec: ItemSpec | null, delayMs = VALIDATE_DEBOUNCE_MS): Validation {
  const key = useMemo(() => (spec ? JSON.stringify(spec) : null), [spec]);
  const [state, setState] = useState<{
    key: string | null;
    itemId: number;
    result: ValidatedItem | null;
    failure: string | null;
  }>({ key: null, itemId: 0, result: null, failure: null });
  useEffect(() => {
    if (key === null || spec === null) return;
    const controller = new AbortController();
    const timer = setTimeout(() => {
      validateItem(spec, controller.signal).then(
        (result) => {
          if (!controller.signal.aborted)
            setState({ key, itemId: spec.item_id, result, failure: null });
        },
        (error: unknown) => {
          if (controller.signal.aborted) return;
          setState({
            key,
            itemId: spec.item_id,
            result: null,
            failure: error instanceof Error ? error.message : String(error),
          });
        },
      );
    }, delayMs);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
    // `key` is the JSON of `spec`: it changes exactly when the content does
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, delayMs]);
  if (key === null) return { result: null, pending: false, failure: null };
  const current = state.key === key;
  return {
    result: state.itemId === spec?.item_id ? state.result : null,
    pending: !current,
    failure: current ? state.failure : null,
  };
}
