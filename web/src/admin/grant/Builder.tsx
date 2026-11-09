// The builder column (Plan/25 section 5.2): the item being built with only the fields it has, the server's verdict on every change and the
// exact game tooltip of the result.
import { useEffect } from 'react';
import { Badge, Button } from '../components';
import { useGrant } from './store';
import { useGrantData, useLimits, useMeta } from './data';
import { ItemIcon } from './ItemIcon';
import { Preview } from './ItemPreview';
import {
  BoundField,
  ChargedField,
  CountField,
  ElementField,
  ExpiryField,
  GuildField,
  OptionsField,
  PetField,
  PiercingField,
  TextField,
  UltimateField,
  UpgradeField,
  VisField,
  ultimateSockets,
  type FieldsProps,
} from './fields';
import { openSaveTemplate } from './templateDialog';
import { cleanSpec, eggPet, fieldsFor, visibilityOf } from './spec';
import { errorFor, useValidation } from './useValidation';
import { T } from './strings';
import type { ItemSpec } from '../gen/dto';

export function Builder() {
  const spec = useGrant((s) => s.spec);
  const editing = useGrant((s) => s.editing);
  const patchSpec = useGrant((s) => s.patchSpec);
  const addSpec = useGrant((s) => s.addSpec);
  const setSpec = useGrant((s) => s.setSpec);
  const data = useGrantData((s) => s.data);
  const meta = useMeta((s) => s.meta);
  const limits = useLimits();
  const verdict = useValidation(spec);

  // the server's profile of the item is better than the metadata's once it answers, but both come from the same rules
  const fields = verdict.result?.fields ?? (spec ? fieldsFor(meta, spec.item_id) : null);
  const vis = visibilityOf(fields);
  const prop = spec ? data?.byId.get(spec.item_id) : undefined;

  // an egg starts as an egg
  const needsPet = vis.pet && spec !== null && !spec.pet;
  useEffect(() => {
    if (needsPet) patchSpec({ pet: eggPet(limits) });
  }, [needsPet, patchSpec, limits]);

  if (!spec) {
    return (
      <section className="grant-col grant-builder" aria-label={T.builder.title}>
        <div className="grant-empty">{T.builder.pick}</div>
      </section>
    );
  }

  const patch = (p: Partial<ItemSpec>): void => {
    let next = p;
    if (fields?.ultimate === true && p.upgrade !== undefined) {
      const n = ultimateSockets(p.upgrade, limits);
      next = {
        ...p,
        ultimate_gems: Array.from({ length: n }, (_v, i) => spec.ultimate_gems[i] ?? 0),
      };
    }
    patchSpec(next);
  };
  const result = verdict.result;
  const error = (field: string): string | undefined => errorFor(result?.errors, field);
  const fp: FieldsProps = { spec, patch, fields, limits, error };
  const name =
    [result?.name, prop?.name].find((n) => n !== undefined && n !== '') ??
    `#${String(spec.item_id)}`;
  const ok = !verdict.pending && result?.ok === true;
  const generic = (result?.errors ?? []).filter(
    (i) =>
      ![
        'count',
        'upgrade',
        'element',
        'element_level',
        'random_options',
        'piercing',
        'ultimate_gems',
        'vis',
        'pet',
        'keep_until',
        'text',
        'guild_id',
      ].some((f) => i.field === f || i.field.startsWith(`${f}[`) || i.field.startsWith(`${f}.`)),
  );

  return (
    <section className="grant-col grant-builder" aria-label={T.builder.title}>
      <header className="grant-builder__head">
        <ItemIcon icon={prop?.icon ?? ''} size={48} label={name} />
        <div>
          <h3 className="grant-builder__name">{name}</h3>
          <div className="grant-hint">
            #{spec.item_id}
            {prop ? ` · ${T.builder.level(prop.level)}` : ''}
          </div>
        </div>
        <div className="grant-builder__badges">
          {result &&
            !verdict.pending &&
            (result.ok ? (
              <Badge tone="ok">{T.builder.valid}</Badge>
            ) : (
              <Badge tone="danger">{T.builder.invalid}</Badge>
            ))}
          {verdict.pending && <Badge>{T.builder.checking}</Badge>}
        </div>
      </header>

      <div className="grant-form">
        {vis.count && <CountField {...fp} />}
        {vis.upgrade && <UpgradeField {...fp} />}
        {vis.element && <ElementField {...fp} />}
        {vis.bound && <BoundField {...fp} />}
        {vis.expiry && <ExpiryField {...fp} />}
        {vis.text && <TextField {...fp} />}
        {vis.guild && <GuildField {...fp} />}
      </div>
      {vis.options && <OptionsField {...fp} data={data} />}
      {vis.piercing && (
        <PiercingField {...fp} data={data} noEffect={meta?.features.no_piercing === true} />
      )}
      {vis.ultimate && <UltimateField {...fp} abilities={meta?.abilities ?? []} data={data} />}
      {vis.vis && <VisField {...fp} data={data} />}
      {vis.pet && <PetField {...fp} />}
      {vis.charged && <ChargedField {...fp} />}

      {verdict.failure && (
        <div className="grant-note grant-note--warn" role="alert">
          {T.builder.validationFailed(verdict.failure)}
        </div>
      )}
      {generic.length > 0 && (
        <ul className="grant-issues" aria-label={T.builder.problems}>
          {generic.map((i, n) => (
            <li key={n} className="grant-issues__error">
              {i.message}
            </li>
          ))}
        </ul>
      )}
      {(result?.warnings ?? []).length > 0 && (
        <ul className="grant-issues" aria-label={T.builder.warnings}>
          {(result?.warnings ?? []).map((i, n) => (
            <li key={n} className="grant-issues__warn">
              {i.message}
            </li>
          ))}
        </ul>
      )}

      <Preview item={result?.item ?? null} />

      <div className="grant-actions">
        <Button
          variant="primary"
          disabled={!ok}
          onClick={() => {
            addSpec(cleanSpec(spec, fields), limits.count_max);
            if (editing === null) setSpec(null);
          }}
        >
          {editing === null ? T.builder.add : T.builder.update}
        </Button>
        <Button
          disabled={!ok}
          onClick={() => {
            openSaveTemplate({ kind: 'item', item: cleanSpec(spec, fields), name });
          }}
        >
          {T.builder.saveTemplate}
        </Button>
        <Button
          variant="ghost"
          onClick={() => {
            setSpec(null);
          }}
        >
          {T.common.close}
        </Button>
      </div>
    </section>
  );
}
