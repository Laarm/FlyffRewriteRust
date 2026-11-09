// Items -> Equipped & bag and Pockets (Plan/25 section 4.4): the equipment in a paper-doll arrangement, the bag grid of 6 columns whose
// size is the capacity of the snapshot, and up to three pockets (unavailable ones are greyed).
import { Badge, Button } from '../../components';
import { useCan } from '../../stores/session';
import { useCharacter } from './context';
import type { ContainerModel } from './containers';
import { formatDateTime, formatNumber } from './format';
import { ItemGrid, PaperDoll } from './Grids';
import { changePenya } from './itemActions';
import { SelectedStrip, useGridCommon, type ItemsUi } from './itemsShared';
import { T } from './strings';

/** `Live` or `Saved <time>`: where the numbers of a container come from. */
export function SourceBadge({ model }: { readonly model: ContainerModel }) {
  return model.live ? (
    <Badge tone="ok">{T.items.liveBadge}</Badge>
  ) : (
    <Badge tone="neutral">{T.items.savedBadge(formatDateTime(model.savedAt))}</Badge>
  );
}

/** The penya of a container (wallet or bank) with its adjust button; the number flashes when it changes. */
export function PenyaLine({
  model,
  flash,
}: {
  readonly model: ContainerModel;
  readonly flash: number | undefined;
}) {
  const { id, name } = useCharacter();
  const canAdjust = useCan('act.items');
  const parity = flash === undefined ? undefined : flash % 2 === 0 ? 'a' : 'b';
  return (
    <span className="plr-penya" data-testid={`penya-${model.kind}`}>
      <span className="adm-muted">{T.items.penya}</span>
      <strong className="plr-flash-text" data-flash={parity}>
        {formatNumber(model.gold)}
      </strong>
      {canAdjust ? (
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            void changePenya({ characterId: id, characterName: name, container: model });
          }}
        >
          {T.items.adjustPenya}
        </Button>
      ) : null}
    </span>
  );
}

function usedOf(model: ContainerModel): number {
  return model.cells.size;
}

export function BagPanel({ ui }: { readonly ui: ItemsUi }) {
  const { id, live } = useCharacter();
  const equip = live.containers.get(`equipment:0:${String(id)}`);
  const bag = live.containers.get(`inventory:0:${String(id)}`);
  const equipCommon = useGridCommon(ui, equip, live.flash);
  const bagCommon = useGridCommon(ui, bag, live.flash);
  return (
    <div className="plr-section">
      <div className="plr-bagdoll">
        <section className="plr-section" aria-label={T.items.equipment}>
          <div className="plr-section__head">
            <h3>{T.items.equipment}</h3>
            {equip ? <SourceBadge model={equip} /> : null}
          </div>
          {equip ? (
            <PaperDoll model={equip} common={equipCommon} />
          ) : (
            <p className="adm-muted">{T.common.loading}</p>
          )}
        </section>
        <section className="plr-section" aria-label="Bag">
          <div className="plr-section__head">
            <h3>{bag ? T.items.bag(usedOf(bag), bag.capacity) : 'Bag'}</h3>
            {bag ? <SourceBadge model={bag} /> : null}
            {bag ? <PenyaLine model={bag} flash={live.goldFlash.get(bag.key)} /> : null}
          </div>
          {bag ? (
            <ItemGrid model={bag} common={bagCommon} scroll label="Bag" />
          ) : (
            <p className="adm-muted">{T.common.loading}</p>
          )}
        </section>
      </div>
      <SelectedStrip ui={ui} containers={live.containers} />
      <p className="adm-muted adm-small">{T.items.flashNote}</p>
    </div>
  );
}

function Pocket({ index, ui }: { readonly index: number; readonly ui: ItemsUi }) {
  const { id, live } = useCharacter();
  const model = live.containers.get(`pocket:${String(index)}:${String(id)}`);
  const common = useGridCommon(ui, model, live.flash);
  const unavailable = !model || model.capacity === 0;
  return (
    <section
      className={`plr-section ${unavailable ? 'plr-pocket--off' : ''}`}
      aria-label={T.items.pocket(index)}
      aria-disabled={unavailable}
    >
      <div className="plr-section__head">
        <h3>{T.items.pocket(index)}</h3>
        {model && !unavailable ? (
          <>
            <Badge>{T.items.pocketUsed(usedOf(model), model.capacity)}</Badge>
            <SourceBadge model={model} />
          </>
        ) : (
          <Badge tone="neutral">{T.items.pocketUnavailable}</Badge>
        )}
      </div>
      {model && !unavailable ? (
        <ItemGrid model={model} common={common} label={T.items.pocket(index)} />
      ) : null}
    </section>
  );
}

export function PocketsPanel({ ui }: { readonly ui: ItemsUi }) {
  const { live } = useCharacter();
  return (
    <div className="plr-section">
      <div className="plr-bagdoll">
        {[0, 1, 2].map((i) => (
          <Pocket key={i} index={i} ui={ui} />
        ))}
      </div>
      <SelectedStrip ui={ui} containers={live.containers} />
    </div>
  );
}
