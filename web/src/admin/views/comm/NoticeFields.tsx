// The fields every notice has (Plan/25 section 8.1): the text with its byte counter, the look and the audience (everybody, one world, or some
// characters picked by name). The form of "send now" and the dialog of a scheduled notice both use it.
import { useEffect, useId, useState } from 'react';
import { Badge, Field, Select, TextArea, TextInput } from '../../components';
import { loadWorlds } from '../../components/mapMeta';
import { searchCharacters } from '../../grant/api';
import type { CharacterListItem, NoticeStyleDto } from '../../gen/dto';
import type { WorldsDoc } from '../../world/metaTypes';
import {
  type Draft,
  type Limits,
  MAX_CHARACTERS,
  type PickedCharacter,
  type TargetDraft,
  maxBytes,
  textProblem,
} from './draft';
import { utf8Length } from './repeat';
import { T } from './strings';

const N = T.notices;

const STYLES: readonly NoticeStyleDto[] = ['system', 'caption'];

function WorldSelect({
  value,
  onChange,
  doc,
}: {
  readonly value: number | null;
  readonly onChange: (world: number | null) => void;
  readonly doc: WorldsDoc | null;
}) {
  return (
    <Field label={N.world}>
      <Select
        value={value === null ? '' : String(value)}
        onChange={(e) => {
          const n = Number(e.target.value);
          onChange(e.target.value === '' || !Number.isInteger(n) ? null : n);
        }}
      >
        <option value="">{N.pickWorld}</option>
        {doc?.groups.map((g) => {
          const worlds = doc.worlds.filter((w) => w.group === g.id);
          if (worlds.length === 0) return null;
          return (
            <optgroup key={g.id} label={g.label}>
              {worlds.map((w) => (
                <option key={w.id} value={String(w.id)}>
                  {w.name} ({String(w.online)} online)
                </option>
              ))}
            </optgroup>
          );
        })}
      </Select>
    </Field>
  );
}

function CharacterFinder({
  chosen,
  onChange,
}: {
  readonly chosen: readonly PickedCharacter[];
  readonly onChange: (next: readonly PickedCharacter[]) => void;
}) {
  const [q, setQ] = useState('');
  const [found, setFound] = useState<readonly CharacterListItem[]>([]);
  const listId = useId();
  useEffect(() => {
    const term = q.trim();
    if (term.length < 2) return undefined;
    const controller = new AbortController();
    const timer = setTimeout(() => {
      searchCharacters(term, 8, controller.signal).then(
        (page) => {
          if (!controller.signal.aborted) setFound(page.items);
        },
        () => undefined,
      );
    }, 200);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [q]);
  const add = (c: CharacterListItem): void => {
    if (chosen.some((x) => x.id === c.id) || chosen.length >= MAX_CHARACTERS) return;
    onChange([...chosen, { id: c.id, name: c.name }]);
    setQ('');
    setFound([]);
  };
  const hits = q.trim().length < 2 ? [] : found;
  const free = hits.filter((h) => !chosen.some((c) => c.id === h.id));
  return (
    <div className="comm-finder">
      <Field label={N.findCharacter} hint={N.findHint}>
        <TextInput
          value={q}
          autoComplete="off"
          onChange={(e) => {
            setQ(e.target.value);
          }}
        />
      </Field>
      {free.length > 0 && (
        <ul id={listId} className="comm-hits" role="listbox" aria-label={N.findCharacter}>
          {free.map((h) => (
            <li key={h.id} role="option" aria-selected="false">
              <button
                type="button"
                className="comm-hit"
                onClick={() => {
                  add(h);
                }}
              >
                <span>{h.name}</span>
                {h.online ? <Badge tone="ok">online</Badge> : <Badge>offline</Badge>}
              </button>
            </li>
          ))}
        </ul>
      )}
      {chosen.length > 0 && (
        <ul className="comm-chosen" aria-label={N.chosen}>
          {chosen.map((c) => (
            <li key={c.id}>
              <span>{c.name}</span>
              <button
                type="button"
                className="adm-iconbtn"
                aria-label={N.removeCharacter(c.name)}
                onClick={() => {
                  onChange(chosen.filter((x) => x.id !== c.id));
                }}
              >
                {'x'}
              </button>
            </li>
          ))}
        </ul>
      )}
      {chosen.length >= MAX_CHARACTERS && (
        <div className="adm-field__hint">{N.tooMany(MAX_CHARACTERS)}</div>
      )}
    </div>
  );
}

export interface NoticeFieldsProps {
  readonly draft: Draft;
  readonly onChange: (next: Draft) => void;
  readonly limits: Limits;
  /** Messages next to the fields (a failed validation of the dialog). */
  readonly errors?: { readonly text?: string | undefined; readonly target?: string | undefined };
}

export function NoticeFields({ draft, onChange, limits, errors = {} }: NoticeFieldsProps) {
  const [worlds, setWorlds] = useState<WorldsDoc | null>(null);
  useEffect(() => {
    let live = true;
    loadWorlds().then(
      (doc) => {
        if (live) setWorlds(doc);
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, []);
  const used = utf8Length(draft.text.trim());
  const max = maxBytes(draft.style, limits);
  const problem = textProblem(draft.text, draft.style, limits);
  const target = draft.target;
  const setTarget = (t: TargetDraft): void => {
    onChange({ ...draft, target: t });
  };
  return (
    <div className="comm-fields">
      <Field
        label={N.text}
        hint={N.textHint(used, max)}
        error={errors.text ?? (problem !== null && used > max ? problem : undefined)}
        required
      >
        <TextArea
          rows={3}
          value={draft.text}
          onChange={(e) => {
            onChange({ ...draft, text: e.target.value });
          }}
        />
      </Field>
      <div className="comm-row">
        <Field label={N.style}>
          <Select
            value={draft.style}
            onChange={(e) => {
              onChange({ ...draft, style: STYLES.find((s) => s === e.target.value) ?? 'system' });
            }}
          >
            {STYLES.map((s) => (
              <option key={s} value={s}>
                {N.styles[s]}
              </option>
            ))}
          </Select>
        </Field>
        <Field label={N.target} error={errors.target}>
          <Select
            value={target.kind}
            onChange={(e) => {
              const v = e.target.value;
              if (v === 'world') setTarget({ kind: 'world', world: null });
              else if (v === 'characters') setTarget({ kind: 'characters', characters: [] });
              else setTarget({ kind: 'all' });
            }}
          >
            <option value="all">{N.targetAll}</option>
            <option value="world">{N.targetWorld}</option>
            <option value="characters">{N.targetCharacters}</option>
          </Select>
        </Field>
        {target.kind === 'world' && (
          <WorldSelect
            value={target.world}
            doc={worlds}
            onChange={(world) => {
              setTarget({ kind: 'world', world });
            }}
          />
        )}
      </div>
      {target.kind === 'characters' && (
        <CharacterFinder
          chosen={target.characters}
          onChange={(characters) => {
            setTarget({ kind: 'characters', characters });
          }}
        />
      )}
    </div>
  );
}

/** The world's display name for the list (`World <id>` until the world list arrives). */
export function worldLabel(doc: WorldsDoc | null, id: number): string {
  return doc?.worlds.find((w) => w.id === id)?.name ?? `World ${String(id)}`;
}
