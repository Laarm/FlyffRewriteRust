// Items & Economy -> Kits & templates (Plan/25 section 5.4): the saved items and baskets; grant one (opens the dialog filled in),
// duplicate, export and import as JSON, delete.
import { useCallback, useEffect, useState } from 'react';
import { runConfirmed } from '../../app/mutate';
import {
  Button,
  Dialog,
  EmptyState,
  ErrorState,
  Field,
  Skeleton,
  Table,
  TextArea,
  TextInput,
  type Column,
} from '../../components';
import { openDialog } from '../../stores/dialogs';
import { toast } from '../../stores/toasts';
import { useCan } from '../../stores/session';
import type { ItemSpec, Kit, TemplateRowDto } from '../../gen/dto';
import { createTemplate, deleteTemplate, listTemplates } from '../../grant/api';
import { ensureGrantData, useGrantData } from '../../grant/data';
import { formatNumber } from '../../grant/format';
import { openGrantDialog } from '../../grant';
import { T } from './strings';

/** The JSON a row exports as: an item as its spec, a kit as `{name, items, gold}`. */
export function exportJson(row: TemplateRowDto): string {
  return JSON.stringify(row.kind === 'kit' ? row.kit : row.item, null, 2);
}

/** Reads imported JSON: a kit has `items`, anything else is an item spec. */
export function parseImport(
  text: string,
  name: string,
):
  | { ok: true; kind: 'item'; item: ItemSpec }
  | { ok: true; kind: 'kit'; kit: Kit }
  | { ok: false; message: string } {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (error) {
    return {
      ok: false,
      message: T.kits.importBad(error instanceof Error ? error.message : String(error)),
    };
  }
  if (typeof value !== 'object' || value === null || Array.isArray(value))
    return { ok: false, message: T.kits.importBad('an object was expected') };
  if ('items' in value && Array.isArray(value.items)) {
    const gold = 'gold' in value && typeof value.gold === 'number' ? value.gold : 0;
    return { ok: true, kind: 'kit', kit: { name, items: value.items as ItemSpec[], gold } };
  }
  if ('item_id' in value) return { ok: true, kind: 'item', item: value as ItemSpec };
  return { ok: false, message: T.kits.importBad('neither an item_id nor items were found') };
}

function ExportDialog({
  row,
  close,
}: {
  readonly row: TemplateRowDto;
  readonly close: () => void;
}) {
  return (
    <Dialog
      title={T.kits.exportTitle(row.name)}
      onClose={close}
      dismissOnOverlay
      footer={<Button onClick={close}>{T.kits.close}</Button>}
    >
      <Field label={T.kits.exportHint}>
        <TextArea className="items-json" readOnly rows={10} value={exportJson(row)} />
      </Field>
    </Dialog>
  );
}

function ImportDialog({
  close,
  onDone,
}: {
  readonly close: () => void;
  readonly onDone: () => void;
}) {
  const [name, setName] = useState('');
  const [text, setText] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async (): Promise<void> => {
    if (name.trim() === '') {
      setError(T.kits.importNeedsName);
      return;
    }
    const parsed = parseImport(text, name.trim());
    if (!parsed.ok) {
      setError(parsed.message);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await createTemplate(
        parsed.kind === 'kit'
          ? { kind: 'kit', name: name.trim(), kit: parsed.kit }
          : { kind: 'item', name: name.trim(), item: parsed.item },
      );
      toast.success(T.kits.imported(name.trim()));
      onDone();
      close();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };
  return (
    <Dialog
      title={T.kits.importTitle}
      onClose={close}
      busy={busy}
      footer={
        <>
          <Button variant="ghost" onClick={close} disabled={busy}>
            {T.kits.close}
          </Button>
          <Button variant="primary" loading={busy} onClick={() => void run()}>
            {T.kits.importRun}
          </Button>
        </>
      }
    >
      <div className="adm-stack">
        <Field label={T.kits.importName}>
          <TextInput
            value={name}
            maxLength={64}
            data-autofocus
            onChange={(e) => {
              setName(e.target.value);
            }}
          />
        </Field>
        <Field label={T.kits.importHint} {...(error ? { error } : {})}>
          <TextArea
            className="items-json"
            rows={10}
            value={text}
            onChange={(e) => {
              setText(e.target.value);
            }}
          />
        </Field>
      </div>
    </Dialog>
  );
}

export function KitsTab() {
  const canGrant = useCan('act.items');
  const data = useGrantData((s) => s.data);
  const [rows, setRows] = useState<readonly TemplateRowDto[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [epoch, setEpoch] = useState(0);
  const reload = useCallback(() => {
    setEpoch((n) => n + 1);
  }, []);
  useEffect(() => {
    void ensureGrantData();
  }, []);
  useEffect(() => {
    let live = true;
    listTemplates().then(
      (r) => {
        if (live) {
          setRows(r);
          setError(null);
        }
      },
      (e: unknown) => {
        if (live) setError(e);
      },
    );
    return () => {
      live = false;
    };
  }, [epoch]);

  const grant = (row: TemplateRowDto): void => {
    if (row.kit) openGrantDialog({ kit: row.kit });
    else if (row.item) openGrantDialog({ spec: row.item });
  };
  const duplicate = async (row: TemplateRowDto): Promise<void> => {
    const name = T.kits.copyOf(row.name).slice(0, 64);
    try {
      await createTemplate(
        row.kind === 'kit' && row.kit
          ? { kind: 'kit', name, kit: { ...row.kit, name } }
          : { kind: 'item', name, item: row.item },
      );
      toast.success(T.kits.duplicated(name));
      reload();
    } catch (e) {
      toast.error(e, T.kits.duplicate);
    }
  };
  const remove = (row: TemplateRowDto): void => {
    void runConfirmed({
      title: T.kits.deleteTitle(row.name),
      message: T.kits.deleteBody,
      confirmLabel: T.kits.delete,
      danger: true,
      run: () => deleteTemplate(row.id),
      success: T.kits.deleted(row.name),
    }).then((o) => {
      if (o.ok) reload();
    });
  };

  const columns: Column<TemplateRowDto>[] = [
    {
      id: 'name',
      header: T.kits.name,
      cell: (r) => r.name,
      text: (r) => r.name,
      sortable: true,
      filterable: true,
    },
    {
      id: 'kind',
      header: T.kits.kind,
      cell: (r) => (r.kind === 'kit' ? T.kits.kit : T.kits.template),
      text: (r) => r.kind,
      sortable: true,
    },
    {
      id: 'contents',
      header: T.kits.contents,
      cell: (r) => contentsOf(r, data?.itemName),
      text: (r) => contentsOf(r, data?.itemName),
    },
    {
      id: 'author',
      header: T.kits.author,
      cell: (r) => r.created_by,
      text: (r) => r.created_by,
      sortable: true,
    },
    {
      id: 'updated',
      header: T.kits.updated,
      cell: (r) => new Date(r.updated_at * 1000).toISOString().slice(0, 16).replace('T', ' '),
      text: (r) => r.updated_at,
      sortable: true,
      sortValue: (r) => r.updated_at,
    },
    {
      id: 'actions',
      header: '',
      cell: (r) => (
        <div className="grant-inline">
          {canGrant && (
            <Button
              size="sm"
              variant="primary"
              aria-label={`${T.kits.grant} ${r.name}`}
              onClick={() => {
                grant(r);
              }}
            >
              {T.kits.grant}
            </Button>
          )}
          <Button
            size="sm"
            aria-label={`${T.kits.duplicate} ${r.name}`}
            onClick={() => void duplicate(r)}
          >
            {T.kits.duplicate}
          </Button>
          <Button
            size="sm"
            aria-label={`${T.kits.export} ${r.name}`}
            onClick={() => {
              openDialog((close) => <ExportDialog row={r} close={close} />);
            }}
          >
            {T.kits.export}
          </Button>
          <Button
            size="sm"
            variant="danger"
            aria-label={`${T.kits.delete} ${r.name}`}
            onClick={() => {
              remove(r);
            }}
          >
            {T.kits.delete}
          </Button>
        </div>
      ),
    },
  ];

  if (error) return <ErrorState error={error} onRetry={reload} />;
  if (rows === null) return <Skeleton />;
  return (
    <div className="items-tab">
      <div className="items-toolbar">
        <span className="grant-hint">{T.kits.hint}</span>
        <Button
          onClick={() => {
            openDialog((close) => <ImportDialog close={close} onDone={reload} />);
          }}
        >
          {T.kits.import}
        </Button>
      </div>
      {rows.length === 0 ? (
        <EmptyState title={T.kits.empty} hint={T.kits.emptyHint} />
      ) : (
        <Table columns={columns} rows={rows} rowKey={(r) => String(r.id)} label={T.kits.title} />
      )}
    </div>
  );
}

function contentsOf(r: TemplateRowDto, name: ((id: number) => string) | undefined): string {
  if (r.kit) return T.kits.kitContents(r.kit.items.length, r.kit.gold);
  if (r.item) {
    const n = name?.(r.item.item_id);
    return `${n === undefined || n === '' ? `#${String(r.item.item_id)}` : n}${r.item.count > 1 ? ` x${formatNumber(r.item.count)}` : ''}`;
  }
  return '';
}
