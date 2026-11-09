// The recipients (Plan/25 section 5.3): a type-ahead over online and offline characters (online first, marked), several at once, a pasted
// list of names or ids, and the groups (everybody online, a world, the party of a character, a guild).
import { useEffect, useRef, useState } from 'react';
import { Badge, Button, Checkbox, NumberInput, Select, TextArea, TextInput } from '../components';
import { useCan } from '../stores/session';
import { rest } from '../api/rest';
import type { CharacterListItem, CharacterProfile } from '../gen/dto';
import { searchCharacters } from './api';
import { GuildPicker } from './GuildPicker';
import { useGrant, useGrantStoreApi, type Recipient } from './store';
import { T } from './strings';

/** Online characters first, then by name. */
export function onlineFirst(rows: readonly CharacterListItem[]): CharacterListItem[] {
  return [...rows].sort(
    (a, b) => Number(b.online) - Number(a.online) || a.name.localeCompare(b.name),
  );
}

export function recipientOf(row: CharacterListItem): Recipient {
  return { id: row.id, name: row.name, online: row.online };
}

/** Splits a pasted list (commas, semicolons, tabs, new lines) into names or ids. */
export function parseList(text: string): string[] {
  return [
    ...new Set(
      text
        .split(/[\n\r,;\t]+/)
        .map((t) => t.trim())
        .filter((t) => t !== ''),
    ),
  ];
}

/** Finds the character that an entry of a pasted list means: an exact name (ignoring case), or an id. */
export function pickExact(
  entry: string,
  rows: readonly CharacterListItem[],
): CharacterListItem | undefined {
  const id = /^\d+$/.test(entry) ? Number(entry) : null;
  const lower = entry.toLowerCase();
  return (
    rows.find((r) => r.name.toLowerCase() === lower) ??
    (id === null ? undefined : rows.find((r) => r.id === id))
  );
}

function Typeahead() {
  const add = useGrant((s) => s.addRecipient);
  const [text, setText] = useState('');
  const [rows, setRows] = useState<readonly CharacterListItem[]>([]);
  const [open, setOpen] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  useEffect(() => {
    const q = text.trim();
    if (q.length < 1) return;
    const controller = new AbortController();
    const timer = setTimeout(() => {
      searchCharacters(q, 12, controller.signal).then(
        (page) => {
          if (controller.signal.aborted) return;
          setRows(onlineFirst(page.items));
          setFailure(null);
        },
        (error: unknown) => {
          if (!controller.signal.aborted)
            setFailure(error instanceof Error ? error.message : String(error));
        },
      );
    }, 200);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [text]);
  const shown = text.trim() === '' ? [] : rows;
  const choose = (row: CharacterListItem): void => {
    add(recipientOf(row));
    setText('');
    setRows([]);
  };
  return (
    <div className="grant-typeahead">
      <TextInput
        role="combobox"
        aria-label={T.recipients.search}
        aria-expanded={open && shown.length > 0}
        aria-autocomplete="list"
        placeholder={T.recipients.placeholder}
        value={text}
        onFocus={() => {
          setOpen(true);
        }}
        onBlur={() => {
          setTimeout(() => {
            setOpen(false);
          }, 150);
        }}
        onChange={(e) => {
          setText(e.target.value);
          setOpen(true);
        }}
        onKeyDown={(e) => {
          const first = shown[0];
          if (e.key === 'Enter' && first) {
            e.preventDefault();
            choose(first);
          }
        }}
      />
      {failure && <div className="adm-field__error">{failure}</div>}
      {open && shown.length > 0 && (
        <ul className="grant-suggest" role="listbox" aria-label={T.recipients.suggestions}>
          {shown.map((r) => (
            <li key={r.id} role="option" aria-selected="false">
              <button
                type="button"
                onMouseDown={(e) => {
                  e.preventDefault();
                }}
                onClick={() => {
                  choose(r);
                }}
              >
                <span className={r.online ? 'grant-dot is-online' : 'grant-dot'} aria-hidden="true">
                  {r.online ? '●' : '○'}
                </span>
                <span>{r.name}</span>
                <span className="grant-hint">
                  {r.online ? T.recipients.online : T.recipients.offline} ·{' '}
                  {T.recipients.lv(r.level)} · {r.account}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function PasteList() {
  const api = useGrantStoreApi();
  const [open, setOpen] = useState(false);
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const [missing, setMissing] = useState<readonly string[]>([]);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const resolve = async (): Promise<void> => {
    const entries = parseList(text);
    setBusy(true);
    const lost: string[] = [];
    for (const entry of entries) {
      try {
        const page = await searchCharacters(entry, 20);
        const hit = pickExact(entry, page.items);
        if (hit) api.getState().addRecipient(recipientOf(hit));
        else lost.push(entry);
      } catch {
        lost.push(entry);
      }
    }
    if (!alive.current) return;
    setMissing(lost);
    setBusy(false);
    if (lost.length === 0) {
      setText('');
      setOpen(false);
    } else {
      setText(lost.join('\n'));
    }
  };
  if (!open) {
    return (
      <Button
        size="sm"
        variant="ghost"
        onClick={() => {
          setOpen(true);
        }}
      >
        {T.recipients.paste}
      </Button>
    );
  }
  return (
    <div className="grant-paste">
      <TextArea
        aria-label={T.recipients.pasteLabel}
        rows={4}
        value={text}
        placeholder={T.recipients.pastePlaceholder}
        onChange={(e) => {
          setText(e.target.value);
        }}
      />
      <div className="grant-inline">
        <Button
          size="sm"
          variant="primary"
          loading={busy}
          disabled={parseList(text).length === 0}
          onClick={() => void resolve()}
        >
          {T.recipients.pasteAdd}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            setOpen(false);
            setMissing([]);
          }}
        >
          {T.common.close}
        </Button>
        {missing.length > 0 && (
          <span className="adm-field__error">{T.recipients.notFound(missing.length)}</span>
        )}
      </div>
    </div>
  );
}

/** "Party of ...": the party is the one of a character (`detail.party_id` of the character route; 0 = not in a party). */
function PartyOf() {
  const group = useGrant((s) => s.group);
  const setGroup = useGrant((s) => s.setGroup);
  const [note, setNote] = useState<string | null>(null);
  const [rows, setRows] = useState<readonly CharacterListItem[]>([]);
  const [text, setText] = useState('');
  useEffect(() => {
    const q = text.trim();
    if (q === '') return;
    const controller = new AbortController();
    const timer = setTimeout(() => {
      searchCharacters(q, 8, controller.signal).then(
        (page) => {
          if (!controller.signal.aborted) setRows(onlineFirst(page.items));
        },
        () => undefined,
      );
    }, 250);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [text]);
  const choose = (c: CharacterListItem): void => {
    rest.get<CharacterProfile>(`/admin/api/characters/${String(c.id)}`).then(
      (p) => {
        const party = p.detail.party_id;
        if (party === 0) {
          setNote(T.recipients.notInParty(c.name));
          setGroup(null);
        } else {
          setNote(null);
          setGroup({ kind: 'party', party, label: T.recipients.partyChosen(c.name, party) });
        }
        setText('');
        setRows([]);
      },
      () => {
        setNote(T.recipients.notInParty(c.name));
      },
    );
  };
  return (
    <div className="grant-stack">
      {group?.kind === 'party' ? (
        <Badge tone="accent">{group.label}</Badge>
      ) : (
        <>
          <TextInput
            type="search"
            aria-label={T.recipients.partyOf}
            placeholder={T.recipients.placeholder}
            value={text}
            onChange={(e) => {
              setText(e.target.value);
            }}
          />
          {rows.length > 0 && text.trim() !== '' ? (
            <ul className="adm-charpick__list" role="listbox" aria-label={T.recipients.partyOf}>
              {rows.map((r) => (
                <li key={r.id} role="option" aria-selected={false}>
                  <button
                    type="button"
                    className="adm-charpick__row"
                    onClick={() => {
                      choose(r);
                    }}
                  >
                    <strong>{r.name}</strong>
                  </button>
                </li>
              ))}
            </ul>
          ) : null}
        </>
      )}
      {note ? <span className="adm-field__error">{note}</span> : null}
    </div>
  );
}

/** "Guild of ...": every member of a guild (online first, the server expands it). */
function GuildOf() {
  const group = useGrant((s) => s.group);
  const setGroup = useGrant((s) => s.setGroup);
  return (
    <div className="grant-stack">
      {group?.kind === 'guild' ? (
        <Badge tone="accent">{group.label}</Badge>
      ) : (
        <GuildPicker
          onPick={(g) => {
            setGroup({
              kind: 'guild',
              guild: g.id,
              label: T.recipients.guildChosen(g.named ? g.name : `#${String(g.id)}`),
            });
          }}
        />
      )}
    </div>
  );
}

export function Recipients() {
  const recipients = useGrant((s) => s.recipients);
  const group = useGrant((s) => s.group);
  const target = useGrant((s) => s.target);
  const ground = useGrant((s) => s.ground);
  const removeRecipient = useGrant((s) => s.removeRecipient);
  const setGroup = useGrant((s) => s.setGroup);
  const setTarget = useGrant((s) => s.setTarget);
  const setGround = useGrant((s) => s.setGround);
  const canDrop = useCan('act.items');
  const [chosen, setChosen] = useState<'party' | 'guild' | null>(null);
  const mode =
    target === 'ground' ? 'ground' : group !== null ? group.kind : (chosen ?? 'characters');

  return (
    <section className="grant-recipients" aria-label={T.recipients.title}>
      <h3 className="grant-h">{T.recipients.title}</h3>
      <Select
        aria-label={T.recipients.kind}
        value={mode}
        onChange={(e) => {
          const v = e.target.value;
          if (v === 'ground') {
            setTarget('ground');
            return;
          }
          setTarget('recipients');
          setChosen(v === 'party' || v === 'guild' ? v : null);
          if (v === 'all_online') setGroup({ kind: 'all_online', includeStaff: false });
          else if (v === 'world') setGroup({ kind: 'world', world: 1 });
          else setGroup(null);
        }}
      >
        <option value="characters">{T.recipients.characters}</option>
        <option value="all_online">{T.recipients.allOnline}</option>
        <option value="world">{T.recipients.world}</option>
        <option value="party">{T.recipients.party}</option>
        <option value="guild">{T.recipients.guild}</option>
        {canDrop && <option value="ground">{T.recipients.ground}</option>}
      </Select>

      {mode === 'characters' && (
        <>
          <ul className="grant-chips" aria-label={T.recipients.chosen}>
            {recipients.map((r) => (
              <li key={r.id} className="grant-chip">
                <span
                  className={r.online ? 'grant-dot is-online' : 'grant-dot'}
                  title={r.online ? T.recipients.online : T.recipients.offline}
                >
                  {r.online ? '●' : '○'}
                </span>
                {r.name}
                <button
                  type="button"
                  aria-label={T.recipients.remove(r.name)}
                  onClick={() => {
                    removeRecipient(r.id);
                  }}
                >
                  x
                </button>
              </li>
            ))}
          </ul>
          <Typeahead />
          <PasteList />
        </>
      )}
      {group?.kind === 'all_online' && target === 'recipients' && (
        <div className="grant-stack">
          <Badge tone="warn">{T.recipients.everybody}</Badge>
          <Checkbox
            label={T.recipients.includeStaff}
            checked={group.includeStaff}
            onChange={(e) => {
              setGroup({ kind: 'all_online', includeStaff: e.target.checked });
            }}
          />
        </div>
      )}
      {group?.kind === 'world' && target === 'recipients' && (
        <label className="grant-facet">
          <span>{T.recipients.worldId}</span>
          <NumberInput
            min={0}
            value={group.world}
            onChange={(e) => {
              setGroup({
                kind: 'world',
                world: Math.max(0, Math.trunc(Number(e.target.value)) || 0),
              });
            }}
          />
        </label>
      )}
      {mode === 'party' && target === 'recipients' && <PartyOf />}
      {mode === 'guild' && target === 'recipients' && <GuildOf />}
      {target === 'ground' && (
        <div className="grant-ground" role="group" aria-label={T.recipients.ground}>
          {(['world', 'layer', 'x', 'y', 'z'] as const).map((k) => (
            <label key={k} className="grant-facet">
              <span>{T.recipients.groundField(k)}</span>
              <NumberInput
                value={ground[k]}
                onChange={(e) => {
                  setGround({ [k]: Number(e.target.value) || 0 });
                }}
              />
            </label>
          ))}
          <div className="grant-hint">
            {ground.owner ? T.recipients.groundOwner(ground.owner.name) : T.recipients.groundAnyone}
          </div>
          {ground.owner ? (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                setGround({ owner: null });
              }}
            >
              {T.recipients.groundClearOwner}
            </Button>
          ) : null}
          <div className="grant-hint">{T.recipients.groundNote}</div>
        </div>
      )}
    </section>
  );
}
