// A character type-ahead (Plan/25 section 6.2 step 2, 5.3): `GET /admin/api/characters?q=` as the grant dialog's recipient field does,
// online characters first (●) and offline ones with the time they were last seen. The wizard, the drawer's add-member and master dialogs use
// it. Choosing a row calls `onPick`; the parent shows what was chosen.
import { useEffect, useState } from 'react';
import { useNow } from '../app/polling';
import type { CharacterListItem } from '../gen/dto';
import { searchCharacters } from '../grant/api';
import { formatAgo } from '../views/players/format';
import { useDebounced } from '../views/players/hooks';
import { TextInput } from './fields';

export interface PickedCharacter {
  readonly id: number;
  readonly name: string;
  readonly online: boolean;
}

export interface CharacterPickerProps {
  readonly onPick: (c: PickedCharacter, row: CharacterListItem) => void;
  readonly label: string;
  readonly placeholder?: string;
  /** Characters that are not offered (already chosen). */
  readonly exclude?: readonly number[];
  readonly autoFocus?: boolean;
  /** Only characters that satisfy this are offered (the master change: members only). */
  readonly filter?: (row: CharacterListItem) => boolean;
  readonly emptyText?: string;
}

/** Online characters first, then by name. */
export function onlineFirst(rows: readonly CharacterListItem[]): CharacterListItem[] {
  return [...rows].sort(
    (a, b) => Number(b.online) - Number(a.online) || a.name.localeCompare(b.name),
  );
}

export function CharacterPicker({
  onPick,
  label,
  placeholder = 'Type a character name or id...',
  exclude = [],
  autoFocus = false,
  filter,
  emptyText = 'No character found',
}: CharacterPickerProps) {
  const [text, setText] = useState('');
  const query = useDebounced(text.trim(), 250);
  const [rows, setRows] = useState<readonly CharacterListItem[]>([]);
  const [searched, setSearched] = useState('');
  const now = Math.floor(useNow(30_000) / 1000);
  useEffect(() => {
    if (query === '') return;
    const controller = new AbortController();
    searchCharacters(query, 12, controller.signal).then(
      (page) => {
        if (controller.signal.aborted) return;
        setRows(onlineFirst(page.items));
        setSearched(query);
      },
      () => {
        if (!controller.signal.aborted) {
          setRows([]);
          setSearched(query);
        }
      },
    );
    return () => {
      controller.abort();
    };
  }, [query]);
  const shown = (query === '' ? [] : rows).filter(
    (r) => !exclude.includes(r.id) && r.status !== 'deleted' && (filter ? filter(r) : true),
  );
  return (
    <div className="adm-charpick">
      <TextInput
        type="search"
        aria-label={label}
        placeholder={placeholder}
        value={text}
        autoFocus={autoFocus}
        autoComplete="off"
        onChange={(e) => {
          setText(e.target.value);
        }}
      />
      {query !== '' && searched === query ? (
        <ul className="adm-charpick__list" role="listbox" aria-label={label}>
          {shown.length === 0 ? (
            <li className="adm-muted adm-small">{emptyText}</li>
          ) : (
            shown.map((r) => (
              <li key={r.id} role="option" aria-selected={false}>
                <button
                  type="button"
                  className="adm-charpick__row"
                  data-character={r.id}
                  onClick={() => {
                    onPick({ id: r.id, name: r.name, online: r.online }, r);
                    setText('');
                    setRows([]);
                  }}
                >
                  <span className={r.online ? 'adm-dot is-online' : 'adm-dot'} aria-hidden="true">
                    {r.online ? '●' : '○'}
                  </span>
                  <strong>{r.name}</strong>
                  <span className="adm-muted adm-small">
                    Lv {r.level}
                    {r.online ? ' - online' : ` - last seen ${formatAgo(now, r.last_seen)}`}
                  </span>
                </button>
              </li>
            ))
          )}
        </ul>
      ) : null}
    </div>
  );
}
