// The basket (Plan/25 section 5.3): the lines to grant, the penya, and the templates and kits that fill it.
import { useEffect, useState } from 'react';
import { Button, NumberInput, Select } from '../components';
import type { TemplateRowDto } from '../gen/dto';
import { listTemplates } from './api';
import { useGrantData, useLimits } from './data';
import { clampInt } from './fields';
import { ItemIcon } from './ItemIcon';
import { extrasOf } from './spec';
import { useGrant } from './store';
import { openSaveTemplate } from './templateDialog';
import { T } from './strings';

/** The templates and kits of the server, loaded once per basket (and again after a save). */
function useTemplates(): { rows: readonly TemplateRowDto[]; reload: () => void } {
  const [rows, setRows] = useState<readonly TemplateRowDto[]>([]);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    let live = true;
    listTemplates().then(
      (r) => {
        if (live) setRows(r);
      },
      () => {
        if (live) setRows([]);
      },
    );
    return () => {
      live = false;
    };
  }, [tick]);
  return {
    rows,
    reload: () => {
      setTick((t) => t + 1);
    },
  };
}

export function Basket() {
  const lines = useGrant((s) => s.lines);
  const gold = useGrant((s) => s.gold);
  const editing = useGrant((s) => s.editing);
  const editLine = useGrant((s) => s.editLine);
  const removeLine = useGrant((s) => s.removeLine);
  const setLineCount = useGrant((s) => s.setLineCount);
  const setGold = useGrant((s) => s.setGold);
  const addSpec = useGrant((s) => s.addSpec);
  const loadKit = useGrant((s) => s.loadKit);
  const data = useGrantData((s) => s.data);
  const limits = useLimits();
  const { rows, reload } = useTemplates();
  const kits = rows.filter((r) => r.kind === 'kit' && r.kit);
  const items = rows.filter((r) => r.kind === 'item' && r.item);

  return (
    <section className="grant-basket" aria-label={T.basket.title}>
      <h3 className="grant-h">{T.basket.title}</h3>
      {lines.length === 0 && gold === 0 && <div className="grant-empty">{T.basket.empty}</div>}
      <ul className="grant-lines">
        {lines.map((line) => {
          const prop = data?.byId.get(line.spec.item_id);
          const name = prop?.name ?? `#${String(line.spec.item_id)}`;
          const extras = extrasOf(line.spec);
          const stack = (prop?.packMax ?? 1) > 1;
          return (
            <li key={line.id} className={`grant-line ${editing === line.id ? 'is-editing' : ''}`}>
              <ItemIcon icon={prop?.icon ?? ''} size={32} />
              <div className="grant-line__text">
                <button
                  type="button"
                  className="grant-link"
                  aria-label={T.basket.edit(name)}
                  onClick={() => {
                    editLine(line.id);
                  }}
                >
                  {name}
                </button>
                {extras.length > 0 && <div className="grant-hint">{extras.join(' · ')}</div>}
              </div>
              {stack ? (
                <NumberInput
                  className="grant-line__count"
                  aria-label={T.basket.count(name)}
                  min={1}
                  max={limits.count_max}
                  value={line.spec.count}
                  onChange={(e) => {
                    setLineCount(line.id, clampInt(e.target.value, 1, limits.count_max));
                  }}
                />
              ) : (
                <span className="grant-line__one">x1</span>
              )}
              <Button
                size="sm"
                variant="ghost"
                icon="x"
                aria-label={T.basket.remove(name)}
                onClick={() => {
                  removeLine(line.id);
                }}
              />
            </li>
          );
        })}
        <li className="grant-line grant-line--gold">
          <span className="grant-icon grant-icon--penya" aria-hidden="true">
            P
          </span>
          <label className="grant-line__text" htmlFor="grant-gold">
            {T.basket.penya}
          </label>
          <NumberInput
            id="grant-gold"
            className="grant-line__count grant-line__gold"
            min={0}
            max={limits.gold_max}
            value={gold === 0 ? '' : gold}
            placeholder="0"
            onChange={(e) => {
              setGold(clampInt(e.target.value, 0, limits.gold_max));
            }}
          />
          {gold > 0 ? (
            <Button
              size="sm"
              variant="ghost"
              icon="x"
              aria-label={T.basket.removePenya}
              onClick={() => {
                setGold(0);
              }}
            />
          ) : (
            <span className="grant-line__spacer" />
          )}
        </li>
      </ul>
      <div className="grant-inline">
        <Select
          aria-label={T.basket.load}
          value=""
          onChange={(e) => {
            const id = Number(e.target.value);
            const row = rows.find((r) => r.id === id);
            if (row?.kit) loadKit(row.kit);
            else if (row?.item) addSpec(row.item, limits.count_max);
          }}
        >
          <option value="">{T.basket.load}</option>
          {kits.length > 0 && (
            <optgroup label={T.basket.kits}>
              {kits.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.name}
                </option>
              ))}
            </optgroup>
          )}
          {items.length > 0 && (
            <optgroup label={T.basket.templates}>
              {items.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.name}
                </option>
              ))}
            </optgroup>
          )}
        </Select>
        <Button
          size="sm"
          disabled={lines.length === 0 && gold === 0}
          onClick={() => {
            openSaveTemplate({
              kind: 'kit',
              kit: { items: lines.map((l) => l.spec), gold },
              onSaved: reload,
            });
          }}
        >
          {T.basket.saveKit}
        </Button>
      </div>
    </section>
  );
}
