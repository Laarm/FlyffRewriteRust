// World & Content -> Rates & events -> Weather (Plan/25 section 7.1; MAY, phase-gated on WP-GM-30): whether a weather effect is on, and the
// controls to clear it or let it rain or snow in a world. Until the weather service exists (`weather.control` is false) the controls are
// disabled and the section says which work package it waits for; the state is always shown.
import { useEffect, useState } from 'react';
import { runConfirmed } from '../../app/mutate';
import { Button, Field, NeedsFeature, Select } from '../../components';
import { loadWorlds } from '../../components/mapMeta';
import type { EventInfoDto, WeatherKindDto } from '../../gen/dto';
import { useCan } from '../../stores/session';
import type { WorldsDoc } from '../../world/metaTypes';
import { setWeather } from './api';
import { T } from './strings';

const W = T.weather;

/** The work package the weather switch waits for. */
export const WEATHER_WP = 'WP-GM-30';

const KINDS: readonly { readonly kind: WeatherKindDto; readonly label: string }[] = [
  { kind: 'none', label: W.clear },
  { kind: 'rain', label: W.rain },
  { kind: 'snow', label: W.snow },
];

export function WeatherSection({
  info,
  onChanged,
}: {
  readonly info: EventInfoDto | null;
  readonly onChanged: () => void;
}) {
  const canEdit = useCan('act.world');
  const [worlds, setWorlds] = useState<WorldsDoc | null>(null);
  const [world, setWorld] = useState<number | null>(null);
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
  if (info === null) return null;
  const control = info.weather.control;
  const worldName =
    worlds?.worlds.find((w) => w.id === world)?.name ?? `World ${String(world ?? 0)}`;

  const set = (kind: WeatherKindDto, label: string): void => {
    if (world === null) return;
    void runConfirmed({
      title: `${label}: ${worldName}?`,
      message: W.noteGlobal,
      confirmLabel: label,
      reason: 'optional',
      run: (given) => setWeather(world, kind, given.reason),
      success: W.done(worldName, label),
    }).then((o) => {
      if (o.ok) onChanged();
    });
  };

  return (
    <section className="content-section" aria-label={W.title}>
      <h3 className="content-h">{W.title}</h3>
      <p>{W.state(info.weather.effect, info.weather.title)}</p>
      {!control && (
        <NeedsFeature wp={WEATHER_WP} title={W.needs}>
          <span className="adm-muted">{W.noteGlobal}</span>
        </NeedsFeature>
      )}
      {canEdit && (
        <div className="content-weather">
          <Field label={W.world}>
            <Select
              disabled={!control}
              value={world === null ? '' : String(world)}
              onChange={(e) => {
                const n = Number(e.target.value);
                setWorld(e.target.value === '' || !Number.isInteger(n) ? null : n);
              }}
            >
              <option value="">{W.pick}</option>
              {worlds?.worlds.map((w) => (
                <option key={w.id} value={String(w.id)}>
                  {w.name}
                </option>
              ))}
            </Select>
          </Field>
          {KINDS.map((k) => (
            <Button
              key={k.kind}
              disabled={!control || world === null}
              onClick={() => {
                set(k.kind, k.label);
              }}
            >
              {k.label}
            </Button>
          ))}
        </div>
      )}
    </section>
  );
}
