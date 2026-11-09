// The item detail (Plan/25 section 4.4 "the item drawer"): everything about one stored item (the exact game tooltip for the character,
// the raw properties, where it lies), "Find all with this serial" and "Copy to builder". It opens over the character's drawer from a
// double click on a cell; it is a dialog and not a route drawer because `?i=` belongs to the catalogue item of the Items category.
// The ownership history of the audit index (WP-ADMIN-29) is shown as phase-gated.
import { useNow } from '../../app/polling';
import { runConfirmedClose } from './dialogClose';
import { Button, Dialog } from '../../components';
import { navigate } from '../../stores/route';
import { openDialog } from '../../stores/dialogs';
import { useCan } from '../../stores/session';
import { toast } from '../../stores/toasts';
import { openGrantDialog } from '../../grant';
import { blankSpec, elementOfId } from '../../grant/spec';
import type { ItemSpec } from '../../gen/dto';
import type { ItemElem } from '../../../gen/proto/flyff/v1/item_types_pb';
import { AuditTimeline } from './AuditTimeline';
import { isBound, isExpired, type ContainerModel } from './containers';
import { formatDateTime, formatNumber } from './format';
import { infoOf } from './ItemCell';
import { usePlayerGame } from './gamedata';
import { removeCell } from './itemActions';
import { T } from './strings';
import { TipBody, flagText, useTipContent } from './Tip';
import type { TipEquipment, TipViewer } from './tooltip';

export interface ItemDialogArgs {
  readonly elem: ItemElem;
  readonly slot: number;
  readonly container: ContainerModel;
  readonly character: { readonly id: number; readonly name: string };
  readonly viewer: TipViewer;
  readonly equipment: TipEquipment;
  /** The route of the console, to leave for the Items category. */
  readonly onNavigate?: () => void;
}

/** Where a cell lies, in words. */
export function whereText(
  c: Pick<ContainerModel, 'kind' | 'index' | 'ownerName'>,
  slot: number,
): string {
  switch (c.kind) {
    case 'equipment':
      return `Equipped, part ${String(slot)}`;
    case 'inventory':
      return `Bag, slot ${String(slot)}`;
    case 'pocket':
      return `Pocket ${String(c.index + 1)}, slot ${String(slot)}`;
    case 'bank':
      return `Bank of ${c.ownerName === '' ? `slot ${String(c.index + 1)}` : c.ownerName}, slot ${String(slot)}`;
  }
}

/** The spec the grant builder starts from for a copy of this item (random options and vis are not copied: the word is raw). */
export function specOf(elem: ItemElem): ItemSpec {
  const spec = blankSpec(elem.itemId, Math.max(1, elem.num));
  const keep = Number(elem.keepTimeAt);
  return {
    ...spec,
    upgrade: Math.max(0, elem.abilityOption),
    element: elementOfId(elem.itemResist),
    element_level: elem.itemResist > 0 ? Math.max(0, elem.resistAbilityOption) : 0,
    piercing: elem.piercing ? [...elem.piercing.cardItemIds] : [],
    ultimate_gems: elem.piercing ? [...elem.piercing.ultimateGemIds] : [],
    charged: elem.charged,
    keep_until: keep > 0 ? { at: { unix: keep } } : 'item',
  };
}

function Row({ label, children }: { readonly label: string; readonly children: React.ReactNode }) {
  return (
    <>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </>
  );
}

function ItemDialogBody({
  args,
  close,
}: {
  readonly args: ItemDialogArgs;
  readonly close: () => void;
}) {
  const { elem, slot, container, character } = args;
  const game = usePlayerGame();
  const info = infoOf(game.data, elem.itemId);
  const canRemove = useCan('act.items');
  const canFind = useCan('view.inventory');
  const at = useNow(60_000);
  const tip = useTipContent({
    elem,
    worn: container.kind === 'equipment',
    at,
    viewer: args.viewer,
    equipment: args.equipment,
  });
  const piercing = elem.piercing;
  const cards = piercing?.cardItemIds.filter((c) => c !== 0) ?? [];
  const gems = piercing?.ultimateGemIds.filter((g) => g !== 0) ?? [];
  const nameOf = (id: number): string => `${infoOf(game.data, id).name} (${String(id)})`;
  const keep = Number(elem.keepTimeAt);

  return (
    <Dialog
      title={info.name}
      size="lg"
      onClose={close}
      footer={
        <>
          {canFind ? (
            <Button
              onClick={() => {
                close();
                args.onNavigate?.();
                navigate({
                  category: 'items',
                  tab: 'search',
                  params: { item: String(elem.itemId), serial: elem.serial.toString() },
                });
              }}
            >
              {T.items.dialog.findSerial}
            </Button>
          ) : null}
          {canRemove ? (
            <>
              <Button
                onClick={() => {
                  openGrantDialog({ spec: specOf(elem) });
                  toast.info(T.items.dialog.copied);
                  close();
                }}
              >
                {T.items.copyToBuilder}
              </Button>
              <Button
                variant="danger"
                onClick={() => {
                  void runConfirmedClose(
                    removeCell({
                      characterId: character.id,
                      characterName: character.name,
                      container,
                      slot,
                      elem,
                      itemName: info.name,
                    }),
                    close,
                  );
                }}
              >
                {T.items.remove}
              </Button>
            </>
          ) : null}
          <Button variant="primary" onClick={close}>
            {T.common.close}
          </Button>
        </>
      }
    >
      <div className="plr-itemdlg">
        <div className="plr-itemdlg__tip">
          <div className="plr-tip plr-tip--static" data-testid="item-dialog-tooltip">
            <TipBody state={tip.state} content={tip.content} rulesReady={tip.rulesReady} />
          </div>
        </div>
        <div className="plr-itemdlg__props">
          <dl className="adm-dl">
            <Row label={T.items.dialog.where}>{whereText(container, slot)}</Row>
            <Row label={T.items.dialog.owner}>{character.name}</Row>
            <Row label={T.items.dialog.itemId}>{elem.itemId}</Row>
            <Row label={T.items.footer.serial}>
              <code>{elem.serial.toString()}</code>
            </Row>
            <Row label={T.items.dialog.count}>{formatNumber(elem.num)}</Row>
            <Row label={T.items.dialog.upgrade}>
              {elem.abilityOption > 0 ? `+${String(elem.abilityOption)}` : '-'}
            </Row>
            <Row label={T.items.dialog.element}>
              {elem.itemResist > 0
                ? `${String(elem.itemResist)} (level ${String(elem.resistAbilityOption)})`
                : '-'}
            </Row>
            <Row label={T.items.footer.flags}>{flagText(elem.flag)}</Row>
            <Row label={T.items.dialog.state}>
              {[
                isBound(elem) ? T.items.footer.bound : '',
                isExpired(elem) ? T.items.footer.expired : '',
              ]
                .filter(Boolean)
                .join(', ') || '-'}
            </Row>
            <Row label={T.items.footer.expires}>
              {keep > 0 ? formatDateTime(keep) : T.items.dialog.never}
            </Row>
            <Row label={T.items.dialog.options}>
              {elem.randomOptItemId === 0n ? (
                '-'
              ) : (
                <code>0x{BigInt.asUintN(64, elem.randomOptItemId).toString(16)}</code>
              )}
            </Row>
            <Row label={T.items.dialog.cards}>
              {cards.length === 0 ? '-' : cards.map(nameOf).join(', ')}
            </Row>
            <Row label={T.items.dialog.gems}>
              {gems.length === 0 ? '-' : gems.map(nameOf).join(', ')}
            </Row>
            {elem.idGuild !== 0 ? <Row label={T.items.footer.guild}>{elem.idGuild}</Row> : null}
            {elem.itemText !== '' ? <Row label={T.items.dialog.text}>{elem.itemText}</Row> : null}
            {elem.pet ? (
              <Row label={T.items.dialog.pet}>
                {`${elem.pet.name || '-'}, kind ${String(elem.pet.kind)}, ${T.items.pets.levels[elem.pet.level] ?? '?'}, exp ${String(elem.pet.exp)}, energy ${String(elem.pet.energy)}, life ${String(elem.pet.life)}`}
              </Row>
            ) : null}
          </dl>
          <h4 className="adm-h3">{T.items.dialog.history}</h4>
          <p className="adm-muted adm-small">{T.items.dialog.historyHint}</p>
          <AuditTimeline
            subject={{ serial: elem.serial.toString() }}
            filterable={false}
            empty={T.history.emptySerial}
          />
        </div>
      </div>
    </Dialog>
  );
}

/** Opens the detail of one cell. */
export function openItemDialog(args: ItemDialogArgs): void {
  openDialog((close) => <ItemDialogBody args={args} close={close} />);
}
