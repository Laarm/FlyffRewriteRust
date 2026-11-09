// A guild type-ahead (Plan/25 section 5.3 "guild of ...", the guild-cloak field of the builder): `GET /admin/api/guilds?q=` over the live
// guild list, by name or id. It shows text only (no logo picture), so the grant dialog, which the shell may reach, stays free of the game images.
import { useEffect, useState } from 'react';
import { fetchPage } from '../api/rest';
import { TextInput } from '../components';
import type { GuildRow } from '../gen/dto';
import { useDebounced } from '../views/players/hooks';
import { T } from './strings';

export interface GuildPickerProps {
  readonly onPick: (g: GuildRow) => void;
  readonly label?: string;
}

export function GuildPicker({ onPick, label = T.recipients.guild }: GuildPickerProps) {
  const [text, setText] = useState('');
  const q = useDebounced(text.trim(), 250);
  const [found, setFound] = useState<{
    readonly q: string;
    readonly rows: readonly GuildRow[];
  } | null>(null);
  useEffect(() => {
    if (q === '') return;
    const controller = new AbortController();
    fetchPage<GuildRow>('/admin/api/guilds', { q, limit: 12 }, null, controller.signal).then(
      (page) => {
        if (!controller.signal.aborted) setFound({ q, rows: page.items });
      },
      () => {
        if (!controller.signal.aborted) setFound({ q, rows: [] });
      },
    );
    return () => {
      controller.abort();
    };
  }, [q]);
  const rows = q !== '' && found?.q === q ? found.rows : null;
  return (
    <div className="adm-charpick">
      <TextInput
        type="search"
        aria-label={label}
        placeholder={T.recipients.guildPlaceholder}
        value={text}
        autoComplete="off"
        onChange={(e) => {
          setText(e.target.value);
        }}
      />
      {rows ? (
        <ul className="adm-charpick__list" role="listbox" aria-label={label}>
          {rows.length === 0 ? (
            <li className="adm-muted adm-small">{T.recipients.guildNone}</li>
          ) : (
            rows.map((g) => (
              <li key={g.id} role="option" aria-selected={false}>
                <button
                  type="button"
                  className="adm-charpick__row"
                  data-guild={g.id}
                  onClick={() => {
                    onPick(g);
                    setText('');
                    setFound(null);
                  }}
                >
                  <strong>{g.named ? g.name : `(unnamed #${String(g.id)})`}</strong>
                  <span className="adm-muted adm-small">
                    Lv {g.level} - {g.members} members{g.master ? ` - master ${g.master.name}` : ''}
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
