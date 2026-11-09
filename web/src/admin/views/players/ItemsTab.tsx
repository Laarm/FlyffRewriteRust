// The Items tab of the character drawer (Plan/25 section 4.4): the sub-tabs Equipped & bag, Pockets, Banks, Pets, Mail (n/50) and Pending
// over the live containers of the character. Reading containers needs `view.inventory`; the pending rows need `act.items`.
import { useState } from 'react';
import { ErrorState, Tabs, tabButtonId, tabPanelId, type TabItem } from '../../components';
import { useCan } from '../../stores/session';
import { BagPanel, PocketsPanel } from './BagPanel';
import { BanksPanel } from './BanksPanel';
import { useCharacter } from './context';
import { FilterBar, useEquipment, useItemsUi, useViewer } from './itemsShared';
import { MailPanel, useMailbox } from './MailPanel';
import { PendingPanel } from './PendingPanel';
import { PetsPanel } from './PetsPanel';
import { T } from './strings';
import { TipProvider } from './Tip';

type Sub = 'bag' | 'pockets' | 'banks' | 'pets' | 'mail' | 'pending';

export function ItemsTab() {
  const canInventory = useCan('view.inventory');
  const canItems = useCan('act.items');
  const { id, live } = useCharacter();
  const ui = useItemsUi();
  const viewer = useViewer();
  const equipment = useEquipment();
  const [sub, setSub] = useState<Sub>('bag');
  const [pending, setPending] = useState<number | null>(null);
  const { mailbox, error } = useMailbox(id, canInventory);
  if (!canInventory) return <p className="adm-muted">{T.common.needsInventory}</p>;

  const tabs: TabItem[] = [
    { id: 'bag', label: T.items.subtabs.bag },
    { id: 'pockets', label: T.items.subtabs.pockets },
    { id: 'banks', label: T.items.subtabs.banks },
    { id: 'pets', label: T.items.subtabs.pets },
    {
      id: 'mail',
      label: mailbox
        ? T.items.mailCount(mailbox.mails.length, mailbox.limit)
        : T.items.subtabs.mail,
    },
    ...(canItems
      ? [
          {
            id: 'pending',
            label: T.items.subtabs.pending,
            ...(pending ? { badge: pending } : {}),
          },
        ]
      : []),
  ];
  const showFilter = sub === 'bag' || sub === 'pockets' || sub === 'banks';
  return (
    <TipProvider viewer={viewer} equipment={equipment}>
      <div className="plr-section" data-testid="items-tab">
        <Tabs
          tabs={tabs}
          value={sub}
          onChange={(id2) => {
            ui.clear();
            setSub(id2 as Sub);
          }}
          idPrefix="plr-items"
          label="Item containers"
        />
        {live.containersError && live.containers.size === 0 ? (
          <ErrorState
            error={live.containersError}
            onRetry={live.reload}
            title={T.items.loadFailed}
          />
        ) : null}
        {live.refusal ? (
          <p className="adm-alert adm-alert--warn">{T.drawer.socketRefused(live.refusal)}</p>
        ) : null}
        {showFilter ? <FilterBar ui={ui} /> : null}
        <div
          role="tabpanel"
          id={tabPanelId('plr-items')}
          aria-labelledby={tabButtonId('plr-items', sub)}
        >
          {sub === 'bag' ? <BagPanel ui={ui} /> : null}
          {sub === 'pockets' ? <PocketsPanel ui={ui} /> : null}
          {sub === 'banks' ? <BanksPanel ui={ui} /> : null}
          {sub === 'pets' ? <PetsPanel /> : null}
          {sub === 'mail' ? <MailPanel mailbox={mailbox} error={error} /> : null}
          {sub === 'pending' ? <PendingPanel onChanged={setPending} /> : null}
        </div>
      </div>
    </TipProvider>
  );
}
