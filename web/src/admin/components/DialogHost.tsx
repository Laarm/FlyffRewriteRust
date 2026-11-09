// Renders the dialogs opened with `openDialog` (stores/dialogs.ts).
import { Fragment } from 'react';
import { useDialogs } from '../stores/dialogs';

export function DialogHost() {
  const entries = useDialogs((s) => s.entries);
  return (
    <>
      {entries.map((e) => (
        <Fragment key={e.id}>
          {e.render(() => {
            useDialogs.setState((s) => ({ entries: s.entries.filter((x) => x.id !== e.id) }));
          })}
        </Fragment>
      ))}
    </>
  );
}
