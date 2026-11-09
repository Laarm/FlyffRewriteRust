// Items -> Banks (Plan/25 section 4.4, ADM-D8): the banks of the ACCOUNT, one per character slot with 42 cells and its penya. The bank
// of this character comes first, the others are labelled with their character names. While any character of the account is online all of
// them are live ("Live via <sibling>"); only an account that is wholly offline shows the saved state ("offline, saved at ...").
import { Badge } from '../../components';
import { useCharacter } from './context';
import type { ContainerModel } from './containers';
import { formatDateTime } from './format';
import { ItemGrid } from './Grids';
import { PenyaLine } from './BagPanel';
import { SelectedStrip, useGridCommon, type ItemsUi } from './itemsShared';
import { T } from './strings';

/** The banks of the account, this character's first. */
export function banksOf(
  containers: ReadonlyMap<string, ContainerModel>,
  characterId: number,
): ContainerModel[] {
  return [...containers.values()]
    .filter((c) => c.kind === 'bank')
    .sort((a, b) => {
      const mine = Number(b.owner === characterId) - Number(a.owner === characterId);
      return mine !== 0 ? mine : a.index - b.index;
    });
}

function Bank({
  model,
  ui,
  viaName,
}: {
  readonly model: ContainerModel;
  readonly ui: ItemsUi;
  readonly viaName: string | null;
}) {
  const { id, live } = useCharacter();
  const common = useGridCommon(ui, model, live.flash);
  const mine = model.owner === id;
  const owner = model.ownerName === '' ? `slot ${String(model.index + 1)}` : model.ownerName;
  return (
    <section
      className="plr-section"
      aria-label={T.items.bank(owner)}
      data-testid={`bank-${String(model.index)}`}
    >
      <div className="plr-section__head">
        <h3>{mine ? T.items.bankThis(owner) : T.items.bank(owner)}</h3>
        {model.live ? (
          <Badge tone="ok">{viaName ? T.items.banksLiveVia(viaName) : T.items.liveBadge}</Badge>
        ) : (
          <Badge>{T.items.banksOffline(formatDateTime(model.savedAt))}</Badge>
        )}
        <PenyaLine model={model} flash={live.goldFlash.get(model.key)} />
      </div>
      <ItemGrid model={model} common={common} label={T.items.bank(owner)} />
    </section>
  );
}

export function BanksPanel({ ui }: { readonly ui: ItemsUi }) {
  const { id, name, live } = useCharacter();
  const banks = banksOf(live.containers, id);
  // the character that serves a live bank: this one when it is online, else the sibling named by the snapshot
  const siblingName = (b: ContainerModel): string | null => {
    if (!b.live) return null;
    if (live.online) return name;
    if (b.liveViaName !== '') return b.liveViaName;
    return b.liveVia !== 0 ? `character ${String(b.liveVia)}` : null;
  };
  return (
    <div className="plr-section">
      <p className="adm-muted">{T.items.banksIntro}</p>
      {banks.length === 0 ? <p className="adm-muted">{T.items.noBanks}</p> : null}
      <div className="plr-bagdoll">
        {banks.map((b) => (
          <Bank key={b.key} model={b} ui={ui} viaName={siblingName(b)} />
        ))}
      </div>
      <SelectedStrip ui={ui} containers={live.containers} />
    </div>
  );
}
