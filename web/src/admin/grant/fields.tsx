// The fields of the item builder (Plan/25 section 5.2): one small component per row of the table. Each is shown only when the item has
// the field (`visibilityOf`), shows the range it accepts, and prints the server's message for it under the control.
import { useState } from 'react';
import { Button, Checkbox, Field, NumberInput, Select, TextInput } from '../components';
import { findDstString } from '../../features/tooltip/dst';
import type {
  AbilityDto,
  Element,
  ItemFieldsDto,
  ItemLimits,
  ItemSpec,
  KeepUntil,
  PetRecord,
  RandomOptLine,
  VisSpec,
} from '../gen/dto';
import type { CatalogItem } from './catalog';
import type { GrantData } from './data';
import { GuildPicker } from './GuildPicker';
import { ELEMENT_KEYS, ELEMENTS, PET_KINDS, PET_LEVELS } from './ranges';
import { blankVis, durationText, eggPet, upgradeMax } from './spec';
import { T } from './strings';

export interface FieldsProps {
  readonly spec: ItemSpec;
  readonly patch: (p: Partial<ItemSpec>) => void;
  readonly fields: ItemFieldsDto | null;
  readonly limits: ItemLimits;
  /** The first server message for a field (`upgrade`, `random_options[1].adj`, ...). */
  readonly error: (field: string) => string | undefined;
}

/** `value`, or `fallback` when it is missing or empty. */
export function orElse(value: string | undefined, fallback: string): string {
  return value === undefined || value === '' ? fallback : value;
}

/** `Number(text)` clamped to `[lo, hi]`; an empty or invalid text is `lo`. */
export function clampInt(text: string, lo: number, hi: number): number {
  const n = Math.trunc(Number(text));
  if (!Number.isFinite(n)) return lo;
  return Math.min(hi, Math.max(lo, n));
}

export function CountField({ spec, patch, fields, limits, error }: FieldsProps) {
  const max = Math.min(fields?.stack_max ?? limits.count_max, limits.count_max);
  const err = error('count');
  return (
    <Field label={T.fields.count} hint={T.fields.range(1, max)} {...(err ? { error: err } : {})}>
      <NumberInput
        min={1}
        max={max}
        value={spec.count}
        onChange={(e) => {
          patch({ count: clampInt(e.target.value, 1, max) });
        }}
      />
    </Field>
  );
}

export function UpgradeField({ spec, patch, fields, limits, error }: FieldsProps) {
  const max = upgradeMax(fields, limits);
  const err = error('upgrade');
  return (
    <Field label={T.fields.upgrade} {...(err ? { error: err } : {})}>
      <Select
        value={spec.upgrade}
        onChange={(e) => {
          patch({ upgrade: clampInt(e.target.value, 0, max) });
        }}
      >
        {Array.from({ length: max + 1 }, (_v, i) => (
          <option key={i} value={i}>
            {i === 0 ? T.fields.none : `+${String(i)}`}
          </option>
        ))}
      </Select>
    </Field>
  );
}

export function ElementField({ spec, patch, limits, error }: FieldsProps) {
  const err = error('element') ?? error('element_level');
  return (
    <Field label={T.fields.element} hint={T.fields.elementHint} {...(err ? { error: err } : {})}>
      <div className="grant-inline">
        <Select
          aria-label={T.fields.element}
          value={spec.element ?? ''}
          onChange={(e) => {
            const v = ELEMENT_KEYS.find((k) => k === e.target.value) ?? null;
            patch({ element: v, element_level: v === null ? 0 : Math.max(1, spec.element_level) });
          }}
        >
          <option value="">{T.fields.none}</option>
          {ELEMENTS.map((el, i) => (
            <option key={el.id} value={ELEMENT_KEYS[i]}>
              {el.name}
            </option>
          ))}
        </Select>
        <NumberInput
          aria-label={T.fields.elementLevel}
          min={0}
          max={limits.element_level}
          disabled={spec.element === null || spec.element === undefined}
          value={spec.element_level}
          onChange={(e) => {
            patch({ element_level: clampInt(e.target.value, 0, limits.element_level) });
          }}
        />
      </div>
    </Field>
  );
}

/** The destination parameters of the random-option tables with their names in game wording. */
function dstChoices(data: GrantData | null, current: number): { dst: number; label: string }[] {
  const text = (tid: number): string => data?.index.text(tid) ?? '';
  const all = new Set(data?.optionDsts ?? []);
  all.add(current);
  return [...all]
    .filter((d) => d > 0)
    .map((dst) => {
      const name = findDstString(dst, text);
      return { dst, label: name === '' ? `DST ${String(dst)}` : `${name} (${String(dst)})` };
    })
    .sort((a, b) => a.label.localeCompare(b.label));
}

export function OptionsField({
  spec,
  patch,
  limits,
  error,
  data,
}: FieldsProps & { readonly data: GrantData | null }) {
  const lines = spec.random_options;
  const set = (i: number, line: RandomOptLine): void => {
    patch({ random_options: lines.map((l, j) => (j === i ? line : l)) });
  };
  const first = data?.optionDsts[0] ?? 1;
  return (
    <fieldset className="grant-group">
      <legend>{T.fields.options}</legend>
      {lines.map((line, i) => {
        const err = error(`random_options[${String(i)}]`);
        return (
          <div key={i} className="grant-option">
            <Select
              aria-label={T.fields.optionDst(i + 1)}
              value={line.dst}
              onChange={(e) => {
                set(i, { ...line, dst: clampInt(e.target.value, 1, limits.option_dst_max) });
              }}
            >
              {dstChoices(data, line.dst).map((c) => (
                <option key={c.dst} value={c.dst}>
                  {c.label}
                </option>
              ))}
            </Select>
            <NumberInput
              aria-label={T.fields.optionAdj(i + 1)}
              min={-limits.option_adj_max}
              max={limits.option_adj_max}
              value={line.adj}
              onChange={(e) => {
                set(i, {
                  ...line,
                  adj: clampInt(e.target.value, -limits.option_adj_max, limits.option_adj_max),
                });
              }}
            />
            <Button
              size="sm"
              variant="ghost"
              icon="x"
              aria-label={T.fields.optionRemove(i + 1)}
              onClick={() => {
                patch({ random_options: lines.filter((_l, j) => j !== i) });
              }}
            />
            {err ? (
              <div className="adm-field__error" role="alert">
                {err}
              </div>
            ) : null}
          </div>
        );
      })}
      <div className="grant-inline">
        <Button
          size="sm"
          disabled={lines.length >= limits.option_lines}
          onClick={() => {
            patch({ random_options: [...lines, { dst: first, adj: 1 }] });
          }}
        >
          {T.fields.optionAdd}
        </Button>
        <Checkbox
          label={T.fields.safe}
          checked={spec.safe}
          onChange={(e) => {
            patch({ safe: e.target.checked });
          }}
        />
        <span className="grant-hint">
          {T.fields.range(-limits.option_adj_max, limits.option_adj_max)}
        </span>
      </div>
    </fieldset>
  );
}

function cardsOf(data: GrantData | null, kind3: number): CatalogItem[] {
  if (!data) return [];
  return data.catalog.filter((c) => c.kind3 === kind3).sort((a, b) => a.name.localeCompare(b.name));
}

export function PiercingField({
  spec,
  patch,
  fields,
  limits,
  error,
  data,
  noEffect,
}: FieldsProps & { readonly data: GrantData | null; readonly noEffect: boolean }) {
  const max = Math.min(
    fields?.piercing_max ?? 0,
    Math.max(limits.piercing_weapon, limits.piercing_suit),
  );
  const cards = cardsOf(data, fields?.piercing_card_kind ?? 0);
  const sockets = spec.piercing;
  const err = error('piercing');
  return (
    <fieldset className="grant-group">
      <legend>{T.fields.piercing}</legend>
      <div className="grant-inline">
        <Select
          aria-label={T.fields.sockets}
          value={sockets.length}
          onChange={(e) => {
            const n = clampInt(e.target.value, 0, max);
            patch({ piercing: Array.from({ length: n }, (_v, i) => sockets[i] ?? 0) });
          }}
        >
          {Array.from({ length: max + 1 }, (_v, i) => (
            <option key={i} value={i}>
              {T.fields.socketCount(i)}
            </option>
          ))}
        </Select>
        {noEffect && <span className="grant-chip grant-chip--info">{T.fields.noPiercing}</span>}
      </div>
      {sockets.map((card, i) => (
        <Select
          key={i}
          aria-label={T.fields.socket(i + 1)}
          value={card}
          onChange={(e) => {
            patch({
              piercing: sockets.map((c, j) =>
                j === i ? clampInt(e.target.value, 0, 0xffff_ffff) : c,
              ),
            });
          }}
        >
          <option value={0}>{T.fields.emptySocket}</option>
          {cards.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name} (#{c.id})
            </option>
          ))}
        </Select>
      ))}
      {err ? (
        <div className="adm-field__error" role="alert">
          {err}
        </div>
      ) : null}
    </fieldset>
  );
}

/** The ultimate sockets an upgrade level opens: `+6` opens one, `+10` five. */
export function ultimateSockets(upgrade: number, limits: ItemLimits): number {
  return Math.min(limits.ultimate_gems, Math.max(0, upgrade - 5));
}

export function UltimateField({
  spec,
  patch,
  limits,
  error,
  abilities,
  data,
}: FieldsProps & { readonly abilities: readonly AbilityDto[]; readonly data: GrantData | null }) {
  const n = ultimateSockets(spec.upgrade, limits);
  const err = error('ultimate_gems');
  const text = (tid: number): string => data?.index.text(tid) ?? '';
  return (
    <fieldset className="grant-group">
      <legend>{T.fields.ultimate}</legend>
      {n === 0 ? <div className="grant-hint">{T.fields.ultimateNone}</div> : null}
      {Array.from({ length: n }, (_v, i) => (
        <Select
          key={i}
          aria-label={T.fields.gem(i + 1)}
          value={spec.ultimate_gems[i] ?? 0}
          onChange={(e) => {
            const v = clampInt(e.target.value, 0, 0xffff_ffff);
            patch({
              ultimate_gems: Array.from({ length: n }, (_x, j) =>
                j === i ? v : (spec.ultimate_gems[j] ?? 0),
              ),
            });
          }}
        >
          <option value={0}>{T.fields.emptySocket}</option>
          {abilities.map((a) => (
            <option key={a.id} value={a.id}>
              {orElse(data?.itemName(a.gem), `#${String(a.gem)}`)} (
              {orElse(findDstString(a.dst, text), `DST ${String(a.dst)}`)})
            </option>
          ))}
        </Select>
      ))}
      {err ? (
        <div className="adm-field__error" role="alert">
          {err}
        </div>
      ) : null}
    </fieldset>
  );
}

/** The seconds / unix time editor of an expiry (`item`, `none`, a duration or a date). */
export function KeepUntilEditor({
  value,
  onChange,
  label,
  itemLabel,
}: {
  readonly value: KeepUntil;
  readonly onChange: (v: KeepUntil) => void;
  readonly label: string;
  readonly itemLabel?: string;
}) {
  const mode =
    value === 'item' || value === 'none' ? value : 'duration' in value ? 'duration' : 'at';
  const secs = typeof value === 'object' && 'duration' in value ? value.duration.secs : 86_400;
  const unit = secs % 86_400 === 0 ? 86_400 : secs % 3_600 === 0 ? 3_600 : 60;
  const at = typeof value === 'object' && 'at' in value ? value.at.unix : 0;
  return (
    <div className="grant-inline">
      <Select
        aria-label={label}
        value={mode}
        onChange={(e) => {
          const m = e.target.value;
          if (m === 'item' || m === 'none') onChange(m);
          else if (m === 'duration') onChange({ duration: { secs } });
          else onChange({ at: { unix: at > 0 ? at : Math.floor(Date.now() / 1000) + 7 * 86_400 } });
        }}
      >
        <option value="item">{itemLabel ?? T.fields.keepItem}</option>
        <option value="none">{T.fields.keepNone}</option>
        <option value="duration">{T.fields.keepDuration}</option>
        <option value="at">{T.fields.keepDate}</option>
      </Select>
      {mode === 'duration' && (
        <>
          <NumberInput
            aria-label={T.fields.keepAmount}
            min={1}
            value={Math.max(1, Math.round(secs / unit))}
            onChange={(e) => {
              onChange({ duration: { secs: clampInt(e.target.value, 1, 3650) * unit } });
            }}
          />
          <Select
            aria-label={T.fields.keepUnit}
            value={unit}
            onChange={(e) => {
              onChange({
                duration: { secs: Math.max(1, Math.round(secs / unit)) * Number(e.target.value) },
              });
            }}
          >
            <option value={60}>{T.fields.minutes}</option>
            <option value={3600}>{T.fields.hours}</option>
            <option value={86400}>{T.fields.days}</option>
          </Select>
          <span className="grant-hint">{durationText(secs)}</span>
        </>
      )}
      {mode === 'at' && (
        <TextInput
          type="date"
          aria-label={T.fields.keepDate}
          value={new Date(at * 1000).toISOString().slice(0, 10)}
          onChange={(e) => {
            const ms = Date.parse(`${e.target.value}T23:59:59Z`);
            if (Number.isFinite(ms)) onChange({ at: { unix: Math.floor(ms / 1000) } });
          }}
        />
      )}
    </div>
  );
}

export function ExpiryField({ spec, patch, error }: FieldsProps) {
  const err = error('keep_until');
  return (
    <Field label={T.fields.expiry} {...(err ? { error: err } : {})}>
      <KeepUntilEditor
        label={T.fields.expiry}
        value={spec.keep_until}
        onChange={(v) => {
          patch({ keep_until: v });
        }}
      />
    </Field>
  );
}

export function VisField({
  spec,
  patch,
  limits,
  error,
  data,
}: FieldsProps & { readonly data: GrantData | null }) {
  const choices = data
    ? data.catalog.filter((c) => c.kind3 === VIS_KIND3).sort((a, b) => a.name.localeCompare(b.name))
    : [];
  const rows = spec.vis;
  const set = (i: number, v: VisSpec): void => {
    patch({ vis: rows.map((r, j) => (j === i ? v : r)) });
  };
  const err = error('vis');
  return (
    <fieldset className="grant-group">
      <legend>{T.fields.vis}</legend>
      {rows.map((row, i) => (
        <div key={i} className="grant-option">
          <Select
            aria-label={T.fields.visItem(i + 1)}
            value={row.item_id}
            onChange={(e) => {
              set(i, { ...row, item_id: clampInt(e.target.value, 0, 0xffff_ffff) });
            }}
          >
            {choices.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name} (#{c.id})
              </option>
            ))}
          </Select>
          <KeepUntilEditor
            label={T.fields.visKeep(i + 1)}
            value={row.keep_until}
            onChange={(v) => {
              set(i, { ...row, keep_until: v });
            }}
          />
          <Button
            size="sm"
            variant="ghost"
            icon="x"
            aria-label={T.fields.visRemove(i + 1)}
            onClick={() => {
              patch({ vis: rows.filter((_r, j) => j !== i) });
            }}
          />
        </div>
      ))}
      <Button
        size="sm"
        disabled={rows.length >= limits.vis || choices.length === 0}
        onClick={() => {
          const first = choices[0];
          if (first) patch({ vis: [...rows, blankVis(first.id)] });
        }}
      >
        {T.fields.visAdd}
      </Button>
      {err ? (
        <div className="adm-field__error" role="alert">
          {err}
        </div>
      ) : null}
    </fieldset>
  );
}

/** `IK3_VIS`. */
export const VIS_KIND3 = 138;

export function PetField({ spec, patch, limits, error }: FieldsProps) {
  const pet: PetRecord = spec.pet ?? eggPet(limits);
  const set = (p: Partial<PetRecord>): void => {
    patch({ pet: { ...pet, ...p } });
  };
  const maxExp = pet.level === 0 ? limits.pet_exp_egg : limits.pet_exp;
  const maxEnergy = limits.pet_energy[pet.level] ?? limits.pet_energy[1] ?? 0;
  const err = error('pet');
  return (
    <fieldset className="grant-group">
      <legend>{T.fields.pet}</legend>
      <div className="grant-grid2">
        <Field label={T.fields.petKind}>
          <Select
            value={pet.kind}
            onChange={(e) => {
              set({ kind: clampInt(e.target.value, 0, PET_KINDS.length - 1) });
            }}
          >
            {PET_KINDS.map((k, i) => (
              <option key={k} value={i}>
                {k}
              </option>
            ))}
          </Select>
        </Field>
        <Field label={T.fields.petLevel}>
          <Select
            value={pet.level}
            onChange={(e) => {
              const level = clampInt(e.target.value, 0, PET_LEVELS.length - 1);
              set({ level, energy: Math.min(pet.energy, limits.pet_energy[level] ?? pet.energy) });
            }}
          >
            {PET_LEVELS.map((l, i) => (
              <option key={l} value={i}>
                {l}
              </option>
            ))}
          </Select>
        </Field>
        <Field label={T.fields.petExp} hint={T.fields.range(0, maxExp)}>
          <NumberInput
            min={0}
            max={maxExp}
            value={pet.exp}
            onChange={(e) => {
              set({ exp: clampInt(e.target.value, 0, maxExp) });
            }}
          />
        </Field>
        <Field label={T.fields.petEnergy} hint={T.fields.range(0, maxEnergy)}>
          <NumberInput
            min={0}
            max={maxEnergy}
            value={pet.energy}
            onChange={(e) => {
              set({ energy: clampInt(e.target.value, 0, maxEnergy) });
            }}
          />
        </Field>
        <Field label={T.fields.petLife} hint={T.fields.range(0, limits.pet_life)}>
          <NumberInput
            min={0}
            max={limits.pet_life}
            value={pet.life}
            onChange={(e) => {
              set({ life: clampInt(e.target.value, 0, limits.pet_life) });
            }}
          />
        </Field>
        <Field label={T.fields.petName} hint={T.fields.bytes(pet.name, limits.pet_name_bytes)}>
          <TextInput
            value={pet.name}
            onChange={(e) => {
              set({ name: e.target.value });
            }}
          />
        </Field>
      </div>
      <div className="grant-avail" role="group" aria-label={T.fields.petAvail}>
        {PET_LEVELS.slice(1).map((l, i) => (
          <label key={l} className="grant-avail__cell">
            <span>{l}</span>
            <NumberInput
              aria-label={T.fields.petAvailLevel(l)}
              min={0}
              max={9}
              value={pet.avail_levels[i] ?? 0}
              onChange={(e) => {
                const next = [...pet.avail_levels];
                next[i] = clampInt(e.target.value, 0, 9);
                set({
                  avail_levels: [
                    next[0] ?? 0,
                    next[1] ?? 0,
                    next[2] ?? 0,
                    next[3] ?? 0,
                    next[4] ?? 0,
                    next[5] ?? 0,
                  ],
                });
              }}
            />
          </label>
        ))}
      </div>
      {err ? (
        <div className="adm-field__error" role="alert">
          {err}
        </div>
      ) : null}
    </fieldset>
  );
}

export function ChargedField({ spec, patch }: FieldsProps) {
  return (
    <Checkbox
      label={T.fields.charged}
      checked={spec.charged}
      onChange={(e) => {
        patch({ charged: e.target.checked });
      }}
    />
  );
}

export function BoundField({ spec, patch }: FieldsProps) {
  const value = spec.bound === true ? 'bound' : spec.bound === false ? 'unbound' : 'item';
  return (
    <Field label={T.fields.bound}>
      <Select
        value={value}
        onChange={(e) => {
          patch({
            bound: e.target.value === 'bound' ? true : e.target.value === 'unbound' ? false : null,
          });
        }}
      >
        <option value="item">{T.fields.boundItem}</option>
        <option value="bound">{T.fields.boundYes}</option>
        <option value="unbound">{T.fields.boundNo}</option>
      </Select>
    </Field>
  );
}

export function TextField({ spec, patch, limits, error }: FieldsProps) {
  const err = error('text');
  const text = spec.text ?? '';
  return (
    <Field
      label={T.fields.text}
      hint={T.fields.bytes(text, limits.text_bytes)}
      {...(err ? { error: err } : {})}
    >
      <TextInput
        value={text}
        onChange={(e) => {
          patch({ text: e.target.value === '' ? null : e.target.value });
        }}
      />
    </Field>
  );
}

export function GuildField({ spec, patch, error }: FieldsProps) {
  const err = error('guild_id');
  const [name, setName] = useState<string | null>(null);
  return (
    <Field label={T.fields.guild} {...(err ? { error: err } : {})}>
      <div className="adm-stack adm-stack--tight">
        <NumberInput
          min={0}
          value={spec.guild_id ?? 0}
          onChange={(e) => {
            const v = clampInt(e.target.value, 0, 0xffff_ffff);
            setName(null);
            patch({ guild_id: v === 0 ? null : v });
          }}
        />
        {name ? <span className="adm-muted adm-small">{name}</span> : null}
        <GuildPicker
          label={T.fields.guild}
          onPick={(g) => {
            setName(g.named ? g.name : `#${String(g.id)}`);
            patch({ guild_id: g.id });
          }}
        />
      </div>
    </Field>
  );
}

/** The element key of a spec as the `Select` value (kept for the tests). */
export function elementValue(element: Element | null | undefined): string {
  return element ?? '';
}
