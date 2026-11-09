// A small item chooser for the tabs that name one item (the ownership search, the economy census): type a name or `#id`, pick from the
// matches. It reads the same game data as the catalogue.
import { useEffect, useMemo, useState } from 'react';
import { TextInput } from '../components';
import { filterCatalog, idQuery, NO_FILTER } from './catalog';
import { ensureGrantData, useGrantData } from './data';
import { ItemIcon } from './ItemIcon';

export interface ItemPickerProps {
  readonly value: number | null;
  readonly onChange: (id: number | null) => void;
  readonly label: string;
  readonly placeholder?: string;
}

export function ItemPicker({ value, onChange, label, placeholder }: ItemPickerProps) {
  const data = useGrantData((s) => s.data);
  const [text, setText] = useState('');
  const [open, setOpen] = useState(false);
  useEffect(() => {
    void ensureGrantData();
  }, []);
  const matches = useMemo(
    () =>
      data && text.trim() !== '' ? filterCatalog(data.catalog, { ...NO_FILTER, text }, 8) : [],
    [data, text],
  );
  const current = value === null ? undefined : data?.byId.get(value);
  const choose = (id: number): void => {
    onChange(id);
    setText('');
    setOpen(false);
  };
  return (
    <div className="grant-typeahead">
      <TextInput
        role="combobox"
        aria-label={label}
        aria-expanded={open && matches.length > 0}
        aria-autocomplete="list"
        placeholder={current ? `${current.name} (#${String(current.id)})` : (placeholder ?? '')}
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
          if (e.target.value.trim() === '') onChange(null);
        }}
        onKeyDown={(e) => {
          if (e.key !== 'Enter') return;
          e.preventDefault();
          const id = idQuery(text);
          const first = matches[0];
          if (id !== null) choose(id);
          else if (first) choose(first.id);
        }}
      />
      {open && matches.length > 0 && (
        <ul className="grant-suggest" role="listbox" aria-label={`${label} matches`}>
          {matches.map((m) => (
            <li key={m.id} role="option" aria-selected={m.id === value}>
              <button
                type="button"
                onMouseDown={(e) => {
                  e.preventDefault();
                }}
                onClick={() => {
                  choose(m.id);
                }}
              >
                <ItemIcon icon={m.icon} size={24} />
                <span>{m.name}</span>
                <span className="grant-hint">#{m.id}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
