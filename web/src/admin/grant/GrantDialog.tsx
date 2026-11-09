// The grant dialog (Plan/25 section 5.3): the grant UI in a wide modal, with the recipient (or the kit, or the ground point) prefilled.
// It opens from anywhere: `openGrantDialog({ recipients: [...] })` (the player rows, the character drawer, the map inspector, the palette).
import { useMemo } from 'react';
import { Dialog } from '../components';
import { openDialog } from '../stores/dialogs';
import { GrantPanel } from './GrantPanel';
import { createGrantStore, GrantProvider, type GrantInit } from './store';
import { T } from './strings';

function GrantDialogBody({
  init,
  close,
}: {
  readonly init: GrantInit;
  readonly close: () => void;
}) {
  const store = useMemo(() => createGrantStore(init), [init]);
  const who = init.recipients?.length === 1 ? init.recipients[0]?.name : undefined;
  return (
    <Dialog title={who ? T.dialog.titleFor(who) : T.dialog.title} onClose={close} size="lg">
      <GrantProvider value={store}>
        <GrantPanel variant="dialog" />
      </GrantProvider>
    </Dialog>
  );
}

/** Opens the grant dialog; `init` prefills it. */
export function openGrantDialog(init: GrantInit = {}): void {
  openDialog((close) => <GrantDialogBody init={init} close={close} />);
}
