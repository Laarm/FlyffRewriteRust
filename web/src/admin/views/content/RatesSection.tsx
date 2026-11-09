// World & Content -> Rates & events -> Rates (Plan/25 section 7.1): each rate with the value of the data, of flyff.toml, the runtime override and
// the effective value, edited with a slider and a number input held to the range of 23 section 8.5, with "Reset". The routes are the existing
// `GET /admin/api/rates` and `PUT /admin/api/rates/{key}`.
import { useState } from 'react';
import { runConfirmed } from '../../app/mutate';
import {
  Badge,
  Button,
  ErrorState,
  NumberInput,
  Skeleton,
  Table,
  type Column,
} from '../../components';
import { useCan } from '../../stores/session';
import { useSnapshot } from '../dashboard/useSnapshot';
import { listRates, setRate } from './api';
import {
  allowed,
  clampRate,
  formatRate,
  fromSlider,
  highest,
  layerOf,
  lowest,
  RATE_INFO,
  type RateRow,
  SLIDER_STEPS,
  toSlider,
} from './rates';
import { T } from './strings';

const R = T.rates;

function labelOf(r: RateRow): string {
  return RATE_INFO[r.key]?.label ?? r.key;
}

/** The slider and the number input of one rate with "Apply". */
export function RateEditor({
  row,
  onChanged,
}: {
  readonly row: RateRow;
  readonly onChanged: () => void;
}) {
  const [text, setText] = useState(formatRate(row.effective));
  const label = labelOf(row);
  const parsed = text.trim() === '' ? Number.NaN : Number(text);
  const valid = allowed(row, parsed);
  const changed = valid && parsed !== row.effective;

  const apply = (): void => {
    const value = clampRate(row, parsed);
    void runConfirmed({
      title: R.confirmTitle(label, formatRate(value)),
      message: R.confirmBody,
      confirmLabel: R.apply,
      run: () => setRate(row.key, value),
      success: R.applied(label, formatRate(value)),
    }).then((o) => {
      if (o.ok) {
        setText(formatRate(o.value.effective));
        onChanged();
      }
    });
  };

  return (
    <div className="content-rate">
      <input
        type="range"
        className="content-slider"
        min={0}
        max={SLIDER_STEPS}
        step={1}
        aria-label={R.sliderLabel(label)}
        value={toSlider(row, valid ? parsed : row.effective)}
        onChange={(e) => {
          setText(formatRate(fromSlider(row, Number(e.target.value))));
        }}
      />
      <NumberInput
        className="content-rate__input"
        aria-label={R.inputLabel(label)}
        min={lowest(row)}
        max={highest(row)}
        step="any"
        value={text}
        onChange={(e) => {
          setText(e.target.value);
        }}
        aria-invalid={text.trim() !== '' && !valid}
      />
      <Button variant="primary" size="sm" disabled={!changed} onClick={apply}>
        {R.apply}
      </Button>
      {text.trim() !== '' && !valid && <span className="adm-field__error">{R.outOfRange}</span>}
    </div>
  );
}

export function RatesSection() {
  const canEdit = useCan('act.world');
  const snap = useSnapshot((signal) => listRates(signal), 'rates', 10_000);

  const reset = (row: RateRow): void => {
    const label = labelOf(row);
    void runConfirmed({
      title: R.resetTitle(label),
      message: R.resetBody,
      confirmLabel: R.reset,
      run: () => setRate(row.key, null),
      success: (r) => R.wasReset(label, formatRate(r.effective)),
    }).then((o) => {
      if (o.ok) snap.reload();
    });
  };

  const columns: Column<RateRow>[] = [
    {
      id: 'rate',
      header: R.rate,
      cell: (r) => (
        <span title={RATE_INFO[r.key]?.hint}>
          <strong>{labelOf(r)}</strong>
          <br />
          <code className="adm-muted">{r.config_key}</code>
        </span>
      ),
      text: (r) => labelOf(r),
      width: '200px',
    },
    {
      id: 'default',
      header: R.dataDefault,
      cell: (r) => formatRate(r.data_default),
      text: (r) => r.data_default,
      align: 'right',
      width: '70px',
    },
    {
      id: 'toml',
      header: R.toml,
      cell: (r) => (r.toml === null ? <span className="adm-muted">-</span> : formatRate(r.toml)),
      text: (r) => r.toml ?? '',
      align: 'right',
      width: '80px',
    },
    {
      id: 'override',
      header: R.override,
      cell: (r) =>
        r.overridden === null ? (
          <span className="adm-muted">-</span>
        ) : (
          <Badge tone="warn">{formatRate(r.overridden)}</Badge>
        ),
      text: (r) => r.overridden ?? '',
      align: 'right',
      width: '90px',
    },
    {
      id: 'effective',
      header: R.effective,
      cell: (r) => (
        <span title={`from ${R.layers[layerOf(r)]}`}>
          <strong className="content-effective">{formatRate(r.effective)}</strong>
        </span>
      ),
      text: (r) => r.effective,
      align: 'right',
      width: '90px',
    },
    {
      id: 'range',
      header: 'Range',
      cell: (r) => (
        <span className="adm-muted">
          {R.range(formatRate(r.min ?? 0), formatRate(highest(r)), r.min_exclusive === true)}
        </span>
      ),
      text: (r) => `${String(r.min ?? 0)}-${String(highest(r))}`,
      width: '150px',
    },
  ];
  if (canEdit) {
    columns.push(
      {
        id: 'set',
        header: R.setTo,
        cell: (r) => (
          <RateEditor key={`${r.key}:${String(r.effective)}`} row={r} onChanged={snap.reload} />
        ),
        width: '380px',
      },
      {
        id: 'reset',
        header: '',
        cell: (r) =>
          r.overridden === null ? null : (
            <Button
              size="sm"
              title={R.resetHint}
              aria-label={`${R.reset} ${labelOf(r)}`}
              onClick={() => {
                reset(r);
              }}
            >
              {R.reset}
            </Button>
          ),
        width: '80px',
      },
    );
  }

  return (
    <section className="content-section" aria-label={R.title}>
      <h3 className="content-h">{R.title}</h3>
      <p className="adm-muted">{R.hint}</p>
      {snap.error !== null && snap.data === null ? (
        <ErrorState error={snap.error} onRetry={snap.reload} />
      ) : snap.data === null ? (
        <Skeleton lines={7} />
      ) : (
        <Table label={R.title} columns={columns} rows={snap.data} rowKey={(r) => r.key} />
      )}
    </section>
  );
}
