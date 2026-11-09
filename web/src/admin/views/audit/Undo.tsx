// The Undo button of an audit record (Plan/25 section 2.5): shown when the action has an inverse and the operator holds the
// capabilities it needs; it runs the inverse after its confirmation and remembers, for this view, what was undone.
import { useCallback, useMemo, useState } from 'react';
import { Badge, Button } from '../../components';
import { useCaps } from '../../stores/session';
import type { ActionEntry } from './collapse';
import { inverseOf, runInverse } from './inverse';
import { T } from './strings';

export interface Undone {
  readonly has: (key: string) => boolean;
  readonly add: (key: string) => void;
}

/** The keys of the entries undone in this view (a second undo of the same record would only fail). */
export function useUndone(): Undone {
  const [set, setSet] = useState<ReadonlySet<string>>(new Set());
  const add = useCallback((key: string): void => {
    setSet((s) => new Set(s).add(key));
  }, []);
  return useMemo(() => ({ has: (key: string) => set.has(key), add }), [set, add]);
}

export function UndoButton({
  entry,
  undone,
  onUndone,
}: {
  readonly entry: ActionEntry;
  readonly undone: Undone;
  readonly onUndone?: () => void;
}) {
  const caps = useCaps();
  const inverse = useMemo(() => inverseOf(entry), [entry]);
  const [busy, setBusy] = useState(false);
  if (!inverse?.caps.every((c) => caps.has(c))) return null;
  if (undone.has(entry.key)) return <Badge>{T.undo.undone}</Badge>;
  return (
    <Button
      size="sm"
      loading={busy}
      aria-label={`${T.undo.button}: ${inverse.title}`}
      title={inverse.title}
      onClick={(e) => {
        e.stopPropagation();
        setBusy(true);
        void runInverse(inverse)
          .then((ran) => {
            if (ran) {
              undone.add(entry.key);
              onUndone?.();
            }
          })
          .finally(() => {
            setBusy(false);
          });
      }}
    >
      {T.undo.button}
    </Button>
  );
}
