// The bookmark form (Plan/25 section 3.11 "Add bookmark here...", 7.4): group, name, world, position (typed or picked on a tile), zoom and the layer
// policy. It creates a shared bookmark, or updates one; editing a default button saves a copy with the same group and name, which replaces it.
import { useEffect, useMemo, useState } from 'react';
import { Button, Dialog, Field, NumberInput, Select, TextInput } from '../../../components';
import { loadWorlds } from '../../../components/mapMeta';
import { openDialog } from '../../../stores/dialogs';
import { toast } from '../../../stores/toasts';
import type { WorldsDoc } from '../../../world/metaTypes';
import {
  bodyOf,
  createBookmark,
  updateBookmark,
  type BookmarkBody,
  type LocationPolicy,
  type LocationRow,
} from './api';
import { PositionPicker } from './PositionPicker';
import { L } from './strings';
import { BOOKMARK_GROUP, useLocations } from './store';

export interface BookmarkSeed {
  readonly group?: string;
  readonly name?: string;
  readonly world: number;
  readonly x: number;
  readonly z: number;
  readonly zoom?: number | null;
  readonly layerPolicy?: LocationPolicy;
  readonly sort?: number;
}

interface FormState {
  group: string;
  name: string;
  world: string;
  x: string;
  z: string;
  zoom: string;
  policy: 'auto' | LocationPolicy;
  sort: string;
}

function initial(seed: BookmarkSeed): FormState {
  return {
    group: seed.group ?? BOOKMARK_GROUP,
    name: seed.name ?? '',
    world: String(seed.world),
    x: String(Math.round(seed.x)),
    z: String(Math.round(seed.z)),
    zoom: seed.zoom === null || seed.zoom === undefined ? '' : String(seed.zoom),
    policy: seed.layerPolicy ?? 'auto',
    sort: String(seed.sort ?? 0),
  };
}

/** The body the form describes, or `null` while a number is missing. */
export function bodyOfForm(f: FormState, worlds: WorldsDoc | null): BookmarkBody | null {
  const world = Number(f.world);
  const x = Number(f.x);
  const z = Number(f.z);
  if (!Number.isInteger(world) || world <= 0 || f.x.trim() === '' || f.z.trim() === '') return null;
  if (!Number.isFinite(x) || !Number.isFinite(z) || f.name.trim() === '') return null;
  const zoom = f.zoom.trim() === '' ? undefined : Number(f.zoom);
  if (zoom !== undefined && !Number.isFinite(zoom)) return null;
  const sort = Number(f.sort);
  const byWorld = worlds?.worlds.find((w) => w.id === world)?.layer_policy;
  return {
    group: f.group.trim() === '' ? BOOKMARK_GROUP : f.group.trim(),
    name: f.name.trim(),
    world,
    layer_policy: f.policy === 'auto' ? (byWorld ?? 'fixed0') : f.policy,
    x,
    z,
    ...(zoom === undefined ? {} : { zoom }),
    sort: Number.isInteger(sort) ? sort : 0,
  };
}

function BookmarkForm({
  seed,
  editing,
  onClose,
}: {
  readonly seed: BookmarkSeed;
  /** The bookmark being edited; a default row means "save a copy". */
  readonly editing: LocationRow | null;
  readonly onClose: () => void;
}) {
  const [form, setForm] = useState<FormState>(() => initial(seed));
  const [worlds, setWorlds] = useState<WorldsDoc | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const groups = useLocations((s) => s.doc?.groups) ?? [];
  useEffect(() => {
    let live = true;
    loadWorlds().then(
      (w) => {
        if (live) setWorlds(w);
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, []);
  const body = useMemo(() => bodyOfForm(form, worlds), [form, worlds]);
  const update = (patch: Partial<FormState>): void => {
    setForm((f) => ({ ...f, ...patch }));
  };
  const isBookmark = editing?.kind === 'bookmark' && editing.bookmark_id !== null;

  const save = async (): Promise<void> => {
    if (!body) {
      setError(L.form.invalid);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (isBookmark && editing.bookmark_id !== null) {
        const row = await updateBookmark(editing.bookmark_id, body);
        toast.success(L.form.updated(row.name));
      } else {
        const row = await createBookmark(body);
        toast.success(L.form.created(row.name));
      }
      await useLocations.getState().load(true);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };

  const worldRows = worlds?.worlds ?? [];
  const F = L.form;
  return (
    <Dialog
      title={isBookmark ? F.editTitle(editing.name) : F.createTitle}
      onClose={onClose}
      busy={busy}
      size="md"
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button variant="primary" loading={busy} disabled={!body} onClick={() => void save()}>
            {F.save}
          </Button>
        </>
      }
    >
      <form
        className="adm-stack"
        data-testid="bookmark-form"
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <div className="loc-form__row">
          <Field label={F.group} hint={F.groupHint}>
            <TextInput
              list="loc-groups"
              maxLength={32}
              value={form.group}
              onChange={(e) => {
                update({ group: e.target.value });
              }}
            />
          </Field>
          <datalist id="loc-groups">
            {groups.map((g) => (
              <option key={g.id} value={g.label} />
            ))}
          </datalist>
          <Field label={F.name} required>
            <TextInput
              data-autofocus
              maxLength={48}
              value={form.name}
              onChange={(e) => {
                update({ name: e.target.value });
              }}
            />
          </Field>
        </div>
        <Field label={F.world}>
          <Select
            value={form.world}
            onChange={(e) => {
              update({ world: e.target.value });
            }}
          >
            {worldRows.length === 0 ? <option value={form.world}>{form.world}</option> : null}
            {worldRows.map((w) => (
              <option key={w.id} value={String(w.id)}>
                {`${w.name} (${String(w.id)})`}
              </option>
            ))}
          </Select>
        </Field>
        <div className="loc-form__row">
          <Field label={F.x}>
            <NumberInput
              value={form.x}
              onChange={(e) => {
                update({ x: e.target.value });
              }}
            />
          </Field>
          <Field label={F.z}>
            <NumberInput
              value={form.z}
              onChange={(e) => {
                update({ z: e.target.value });
              }}
            />
          </Field>
        </div>
        <PositionPicker
          world={Number(form.world) || seed.world}
          x={Number(form.x) || 0}
          z={Number(form.z) || 0}
          onPick={(x, z) => {
            update({ x: String(x), z: String(z) });
          }}
        />
        <div className="loc-form__row">
          <Field label={F.policy}>
            <Select
              value={form.policy}
              onChange={(e) => {
                update({
                  policy:
                    e.target.value === 'pick_live'
                      ? 'pick_live'
                      : e.target.value === 'fixed0'
                        ? 'fixed0'
                        : 'auto',
                });
              }}
            >
              <option value="auto">By the world</option>
              <option value="fixed0">{F.policyFixed}</option>
              <option value="pick_live">{F.policyLive}</option>
            </Select>
          </Field>
          <Field label={F.sort}>
            <NumberInput
              value={form.sort}
              onChange={(e) => {
                update({ sort: e.target.value });
              }}
            />
          </Field>
        </div>
        <Field label={F.zoom}>
          <NumberInput
            step="0.25"
            value={form.zoom}
            onChange={(e) => {
              update({ zoom: e.target.value });
            }}
          />
        </Field>
        {error ? (
          <p className="adm-field__error" role="alert">
            {error}
          </p>
        ) : null}
      </form>
    </Dialog>
  );
}

/** Opens the form for a new bookmark at `seed`. */
export function openBookmarkDialog(seed: BookmarkSeed): void {
  openDialog((close) => <BookmarkForm seed={seed} editing={null} onClose={close} />);
}

/** Opens the form for a row: a bookmark is updated, a default is copied (the copy replaces it). */
export function openEditLocation(row: LocationRow): void {
  const body = bodyOf(row);
  openDialog((close) => (
    <BookmarkForm
      seed={{
        group: body.group,
        name: body.name,
        world: body.world,
        x: body.x,
        z: body.z,
        zoom: body.zoom ?? null,
        ...(body.layer_policy ? { layerPolicy: body.layer_policy } : {}),
        sort: body.sort,
      }}
      editing={row}
      onClose={close}
    />
  ));
}
