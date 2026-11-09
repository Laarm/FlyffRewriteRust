// World & Content -> Locations (Plan/25 section 7.4): the editor of the shared bookmarks. It lists the same buttons the map's panel shows - the default
// ones that come from the game data (read only; editing one saves a copy that replaces it) and the bookmarks staff made - with a static picture of the
// selected place, creating, editing, deleting and ordering of bookmarks, and JSON export and import. Changing needs `act.world`; reading needs `view.map`.
import { useMemo, useState } from 'react';
import { runConfirmed } from '../../../app/mutate';
import {
  Badge,
  Button,
  Dialog,
  EmptyState,
  ErrorState,
  Field,
  Select,
  Skeleton,
  TextArea,
  TextInput,
} from '../../../components';
import { StaticMapPreview } from '../../../components/StaticMapPreview';
import { openDialog } from '../../../stores/dialogs';
import { useCan } from '../../../stores/session';
import { toast } from '../../../stores/toasts';
import { openLocation } from '../jump';
import { useWorldNames } from '../worldNames';
import { bodyOf, createBookmark, updateBookmark, type BookmarkBody, type LocationRow } from './api';
import { openBookmarkDialog, openEditLocation } from './BookmarkDialog';
import { removeBookmark } from './LocationMenu';
import { L } from './strings';
import { useLocations, useLocationsFeed } from './store';
import { exportJson, parseImport, sameSlot } from './transfer';
import './locations.css';

/** Downloads text as a file (the operator pressed the button; nothing is sent anywhere). */
function download(name: string, text: string): void {
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

/** Creates or updates one bookmark per body (a bookmark of the same group and name is updated); returns the counts. */
export async function importBodies(
  bodies: readonly BookmarkBody[],
  existing: readonly LocationRow[],
): Promise<{ created: number; updated: number; failed: string[] }> {
  let created = 0;
  let updated = 0;
  const failed: string[] = [];
  for (const body of bodies) {
    try {
      const old = existing.find((r) => sameSlot(r, body));
      if (old?.bookmark_id != null) {
        await updateBookmark(old.bookmark_id, body);
        updated += 1;
      } else {
        await createBookmark(body);
        created += 1;
      }
    } catch (e) {
      failed.push(`${body.name}: ${e instanceof Error ? e.message : String(e)}`);
    }
  }
  return { created, updated, failed };
}

function ImportDialog({ onClose }: { readonly onClose: () => void }) {
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (): Promise<void> => {
    const parsed = parseImport(text);
    if (!parsed || (parsed.bodies.length === 0 && parsed.errors.length === 0)) {
      setError(L.editor.importBad);
      return;
    }
    setBusy(true);
    setError(null);
    const existing = useLocations.getState().doc?.locations ?? [];
    const r = await importBodies(parsed.bodies, existing);
    await useLocations.getState().load(true);
    const failed = [...parsed.errors, ...r.failed];
    toast.success(L.editor.importDone(r.created, r.updated, failed.length));
    if (failed.length > 0) {
      setError(L.editor.importFailed(failed.length, failed[0] ?? ''));
      setBusy(false);
      return;
    }
    onClose();
  };
  return (
    <Dialog
      title={L.editor.importTitle}
      onClose={onClose}
      busy={busy}
      size="md"
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={text.trim() === ''}
            onClick={() => void run()}
          >
            {L.editor.importAction}
          </Button>
        </>
      }
    >
      <div className="adm-stack">
        <p className="adm-muted">{L.editor.importHint}</p>
        <Field label="JSON">
          <TextArea
            rows={10}
            data-autofocus
            placeholder={L.editor.importPlaceholder}
            value={text}
            onChange={(e) => {
              setText(e.target.value);
            }}
          />
        </Field>
        {error ? (
          <p className="adm-field__error" role="alert">
            {error}
          </p>
        ) : null}
      </div>
    </Dialog>
  );
}

/** Copies every default button into a bookmark of the same group and name (so staff can edit it). */
async function copyDefaults(rows: readonly LocationRow[]): Promise<void> {
  const defaults = rows.filter((r) => r.kind === 'default');
  await runConfirmed({
    title: L.editor.copyDefaultsTitle,
    message: L.editor.copyDefaultsMessage,
    confirmLabel: 'Copy',
    run: async () => {
      const bodies = defaults.map((r) => bodyOf(r));
      const res = await importBodies(bodies, rows);
      await useLocations.getState().load(true);
      if (res.failed.length > 0) throw new Error(res.failed[0]);
      return res;
    },
    success: (r) => L.editor.copyDefaultsDone(r.created + r.updated),
  });
}

function moveWithinGroup(
  rows: readonly LocationRow[],
  row: LocationRow,
  dir: -1 | 1,
): LocationRow | null {
  const mates = rows.filter(
    (r) => r.kind === 'bookmark' && r.group.toLowerCase() === row.group.toLowerCase(),
  );
  const at = mates.findIndex((r) => r.id === row.id);
  return mates[at + dir] ?? null;
}

/** Moves a bookmark one place inside its group: the group is numbered 0, 1, 2... in the new order (only the rows whose number changed are saved). */
async function moveBookmark(
  all: readonly LocationRow[],
  row: LocationRow,
  dir: -1 | 1,
): Promise<void> {
  const mates = all.filter(
    (r) => r.kind === 'bookmark' && r.group.toLowerCase() === row.group.toLowerCase(),
  );
  const i = mates.findIndex((r) => r.id === row.id);
  const j = i + dir;
  const other = mates[j];
  if (i < 0 || !other) return;
  const order = [...mates];
  order[i] = other;
  order[j] = row;
  for (const [k, r] of order.entries()) {
    if (r.sort !== k && r.bookmark_id !== null)
      await updateBookmark(r.bookmark_id, { ...bodyOf(r), sort: k });
  }
  await useLocations.getState().load(true);
}

export function LocationsTab() {
  useLocationsFeed();
  const doc = useLocations((s) => s.doc);
  const status = useLocations((s) => s.status);
  const error = useLocations((s) => s.error);
  const canEdit = useCan('act.world');
  const names = useWorldNames();
  const [text, setText] = useState('');
  const [group, setGroup] = useState('');
  const [picked, setPicked] = useState<string | null>(null);

  const rows = useMemo(() => {
    const needle = text.trim().toLowerCase();
    return (doc?.locations ?? []).filter(
      (r) =>
        (group === '' || r.group.toLowerCase() === group.toLowerCase()) &&
        (needle === '' ||
          r.name.toLowerCase().includes(needle) ||
          r.group.toLowerCase().includes(needle)),
    );
  }, [doc, text, group]);
  const selected = rows.find((r) => r.id === picked) ?? null;
  const bookmarks = (doc?.locations ?? []).filter((r) => r.kind === 'bookmark');
  const E = L.editor;

  if (status === 'error' && doc === null) {
    return (
      <ErrorState
        title={L.panel.failed}
        error={error ?? L.panel.failed}
        onRetry={() => {
          void useLocations.getState().load(true);
        }}
      />
    );
  }
  if (doc === null) return <Skeleton lines={6} />;
  return (
    <div className="loc-editor" data-testid="locations-editor">
      <p className="adm-muted">{E.intro}</p>
      <div className="loc-editor__bar">
        <TextInput
          aria-label={E.search}
          placeholder={E.search}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
          }}
        />
        <Select
          aria-label={E.groups}
          value={group}
          onChange={(e) => {
            setGroup(e.target.value);
          }}
        >
          <option value="">{E.all}</option>
          {doc.groups.map((g) => (
            <option key={g.id} value={g.label}>
              {`${g.label} (${String(g.count)})`}
            </option>
          ))}
        </Select>
        {canEdit ? (
          <Button
            variant="primary"
            onClick={() => {
              const first = doc.locations[0];
              openBookmarkDialog({ world: first?.world ?? 1, x: first?.x ?? 0, z: first?.z ?? 0 });
            }}
          >
            {E.new}
          </Button>
        ) : null}
        <Button
          onClick={() => {
            download('locations.json', exportJson(doc.locations));
            toast.success(E.exported(bookmarks.length));
          }}
        >
          {E.export}
        </Button>
        {canEdit ? (
          <>
            <Button
              onClick={() => {
                openDialog((close) => <ImportDialog onClose={close} />);
              }}
            >
              {E.import}
            </Button>
            <Button
              onClick={() => {
                void copyDefaults(doc.locations);
              }}
            >
              {E.copyDefaults}
            </Button>
          </>
        ) : null}
      </div>
      {!canEdit ? <p className="adm-muted">{E.needsWorld}</p> : null}
      <div className="loc-editor__body">
        {rows.length === 0 ? (
          <EmptyState title={E.empty} />
        ) : (
          <div className="adm-table loc-table">
            <div className="adm-table__scroll">
              <table aria-label={E.title}>
                <thead>
                  <tr>
                    <th>{E.columns.name}</th>
                    <th>{E.columns.group}</th>
                    <th>{E.columns.world}</th>
                    <th>{E.columns.position}</th>
                    <th>{E.columns.source}</th>
                    <th>{E.columns.actions}</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((r) => (
                    <tr
                      key={r.id}
                      data-location={r.id}
                      aria-selected={picked === r.id}
                      className={picked === r.id ? 'is-clickable is-selected' : 'is-clickable'}
                      onClick={() => {
                        setPicked(r.id);
                      }}
                    >
                      <td>
                        {r.name}{' '}
                        {r.kind === 'bookmark' ? <Badge tone="accent">bookmark</Badge> : null}
                      </td>
                      <td>{r.group}</td>
                      <td>{names.get(r.world) ?? String(r.world)}</td>
                      <td>{`${String(Math.round(r.x))}, ${String(Math.round(r.z))}`}</td>
                      <td>{r.kind === 'bookmark' ? (r.created_by ?? 'bookmark') : r.source}</td>
                      <td>
                        <span className="loc-row-actions">
                          <Button
                            size="sm"
                            variant="ghost"
                            onClick={(e) => {
                              e.stopPropagation();
                              openLocation(
                                {
                                  world: r.world,
                                  x: r.x,
                                  z: r.z,
                                  ...(r.zoom === null ? {} : { upp: r.zoom }),
                                },
                                r.layer_policy,
                              );
                            }}
                          >
                            {E.open}
                          </Button>
                          {canEdit ? (
                            <Button
                              size="sm"
                              variant="ghost"
                              onClick={(e) => {
                                e.stopPropagation();
                                openEditLocation(r);
                              }}
                            >
                              {r.kind === 'bookmark' ? E.edit : E.editCopy}
                            </Button>
                          ) : null}
                          {canEdit && r.kind === 'bookmark' ? (
                            <>
                              <Button
                                size="sm"
                                variant="ghost"
                                aria-label={`${E.sortUp}: ${r.name}`}
                                disabled={moveWithinGroup(doc.locations, r, -1) === null}
                                onClick={(e) => {
                                  e.stopPropagation();
                                  void moveBookmark(doc.locations, r, -1);
                                }}
                              >
                                ↑
                              </Button>
                              <Button
                                size="sm"
                                variant="ghost"
                                aria-label={`${E.sortDown}: ${r.name}`}
                                disabled={moveWithinGroup(doc.locations, r, 1) === null}
                                onClick={(e) => {
                                  e.stopPropagation();
                                  void moveBookmark(doc.locations, r, 1);
                                }}
                              >
                                ↓
                              </Button>
                              <Button
                                size="sm"
                                variant="ghost"
                                onClick={(e) => {
                                  e.stopPropagation();
                                  void removeBookmark(r);
                                }}
                              >
                                {E.delete}
                              </Button>
                            </>
                          ) : null}
                        </span>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        )}
        {selected ? (
          <aside className="loc-preview" data-testid="location-preview">
            <h4>{selected.name}</h4>
            <StaticMapPreview
              world={selected.world}
              x={selected.x}
              z={selected.z}
              size={220}
              onOpen={() => {
                openLocation(
                  {
                    world: selected.world,
                    x: selected.x,
                    z: selected.z,
                    ...(selected.zoom === null ? {} : { upp: selected.zoom }),
                  },
                  selected.layer_policy,
                );
              }}
            />
            <dl className="insp-facts">
              <div className="insp-facts__row">
                <dt>{E.columns.world}</dt>
                <dd>{names.get(selected.world) ?? selected.world}</dd>
              </div>
              <div className="insp-facts__row">
                <dt>{E.columns.position}</dt>
                <dd>{`${String(Math.round(selected.x))}, ${String(Math.round(selected.z))}`}</dd>
              </div>
              <div className="insp-facts__row">
                <dt>{E.columns.source}</dt>
                <dd>{selected.source}</dd>
              </div>
              <div className="insp-facts__row">
                <dt>Layer</dt>
                <dd>{selected.layer_policy === 'pick_live' ? 'pick a live layer' : 'layer 0'}</dd>
              </div>
            </dl>
          </aside>
        ) : null}
      </div>
    </div>
  );
}
