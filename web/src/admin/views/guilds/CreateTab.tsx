// Guilds -> Create guild (Plan/25 section 6.2, the owner MUST): the four steps on one page with a live preview card on the right.
//   1. Name   with the debounced `validate-name` answer under the field
//   2. Master and members   a character picker (online first), "already in guild X" at once, a rank per member with the free places
//   3. Logo   the visual picker (27 logos + None, the GM group) and the flat cloak preview with a male / female toggle
//   4. Settings   level, penya, contribution, notice, "Give the guild cloak to the master"
// "Create guild" asks for the optional reason (and a typed confirmation above the penya threshold) through `runConfirmed`, posts once and
// keeps its idempotency key across retries of the same request.
import { useEffect, useRef, useState } from 'react';
import { navigate } from '../../stores/route';
import { runConfirmed } from '../../app/mutate';
import { isApiError, newIdempotencyKey } from '../../api/rest';
import { CharacterPicker, type PickedCharacter } from '../../components/CharacterPicker';
import { GuildLogo } from '../../components/GuildLogo';
import { LogoPicker } from '../../components/LogoPicker';
import {
  Badge,
  Button,
  Checkbox,
  ErrorState,
  Field,
  NumberInput,
  Select,
  Skeleton,
  TextArea,
  TextInput,
} from '../../components';
import type { GrantOutcome, GuildMeta, GuildRank } from '../../gen/dto';
import { openGrantDialog } from '../../grant';
import { blankSpec } from '../../grant/spec';
import { toast } from '../../stores/toasts';
import { useDebounced } from '../players/hooks';
import { checkGuildName, createGuild, guildOfCharacter } from './api';
import { notifyGuildsChanged } from './changes';
import { cloakItemId, type CloakSex } from './cloak';
import { CloakPreview } from './CloakPreview';
import { G, RANK_ORDER } from './strings';
import { useGuildMeta } from './useGuildMeta';
import {
  INITIAL,
  blockedReason,
  clampGold,
  clampLevel,
  clampPxp,
  maxMembers,
  noticeBytes,
  rankPlaces,
  stepOfField,
  stepsDone,
  toSpec,
  type StepId,
  type WizardMember,
  type WizardState,
} from './wizard';

interface NameStatus {
  readonly text: string;
  readonly ok: boolean | null;
  readonly message: string;
}

/** The wording of the cloak outcome after a creation. */
export function cloakMessage(outcome: GrantOutcome): string {
  if ('delivered' in outcome) return G.create.cloakDelivered;
  if ('mailed' in outcome) return G.create.cloakMailed;
  if ('pending' in outcome) return G.create.cloakPending;
  return G.create.cloakFailed(outcome.failed.message);
}

function StepHead({
  id,
  done,
  title,
}: {
  readonly id: StepId;
  readonly done: boolean;
  readonly title: string;
}) {
  return (
    <h3 className="gld-step__head" id={`gld-step-${id}`}>
      {title} {done ? <Badge tone="ok">ok</Badge> : null}
    </h3>
  );
}

export function CreateTab() {
  const { meta, error } = useGuildMeta();
  if (error !== null) return <ErrorState error={error} title={G.create.title} />;
  if (!meta) return <Skeleton lines={8} height={18} />;
  return <Wizard meta={meta} />;
}

function Wizard({ meta }: { readonly meta: GuildMeta }) {
  const [state, setState] = useState<WizardState>(INITIAL);
  const patch = (p: Partial<WizardState>): void => {
    setState((s) => ({ ...s, ...p }));
  };
  const [sex, setSex] = useState<CloakSex>('male');
  const [busy, setBusy] = useState(false);
  const [serverStep, setServerStep] = useState<{ step: StepId; message: string } | null>(null);
  const [inGuild, setInGuild] = useState<ReadonlyMap<number, string>>(new Map());
  const key = useRef<{ body: string; key: string } | null>(null);

  // 1. the name check, debounced
  const typed = state.name.trim();
  const debounced = useDebounced(typed, 300);
  const [nameStatus, setNameStatus] = useState<NameStatus>({ text: '', ok: null, message: '' });
  useEffect(() => {
    if (debounced === '') return;
    const controller = new AbortController();
    checkGuildName(debounced, controller.signal).then(
      (c) => {
        if (!controller.signal.aborted)
          setNameStatus({ text: debounced, ok: c.ok, message: c.error?.message ?? '' });
      },
      () => undefined,
    );
    return () => {
      controller.abort();
    };
  }, [debounced]);
  const nameCurrent = nameStatus.text === typed && typed !== '';
  const nameOk = nameCurrent ? nameStatus.ok : null;

  // 2. "already in guild X" at once
  const remember = (c: PickedCharacter): void => {
    guildOfCharacter(c.id).then(
      (g) => {
        if (g) setInGuild((m) => new Map(m).set(c.id, g.named ? g.name : G.list.unnamed(g.id)));
      },
      () => undefined,
    );
  };

  const done = stepsDone(state, meta, { ok: nameOk });
  const blocked = blockedReason(state, meta, { ok: nameOk });
  const places = rankPlaces(state, meta);
  const freeOf = (rank: GuildRank): number => places.find((p) => p.rank === rank)?.free ?? 0;
  const limit = maxMembers(state.level, meta);
  const bytes = noticeBytes(state.notice);
  const spec = toSpec(state);

  const addMember = (c: PickedCharacter): void => {
    remember(c);
    const member: WizardMember = { ...c, rank: 'member' };
    patch({ members: [...state.members, member] });
  };
  const setRank = (id: number, rank: GuildRank): void => {
    patch({ members: state.members.map((m) => (m.id === id ? { ...m, rank } : m)) });
  };

  const submit = async (): Promise<void> => {
    if (blocked !== null || !state.master) return;
    const json = JSON.stringify(spec);
    if (key.current?.body !== json) key.current = { body: json, key: newIdempotencyKey() };
    const idemKey = key.current.key;
    const master = state.master;
    setBusy(true);
    setServerStep(null);
    const out = await runConfirmed({
      title: G.create.submit,
      message: `${spec.name} - ${master.name}`,
      confirmLabel: G.create.submit,
      reason: 'optional',
      facts: { gold: spec.gold, targetName: spec.name },
      run: async (given) => {
        try {
          return await createGuild(spec, given, idemKey);
        } catch (e) {
          if (isApiError(e)) {
            const step = stepOfField(e.details?.field ?? undefined);
            if (step) setServerStep({ step, message: e.message });
          }
          throw e;
        }
      },
    });
    setBusy(false);
    if (!out.ok) return;
    const created = out.value;
    notifyGuildsChanged();
    toast.success(G.create.created(spec.name));
    if (created.cloak) {
      if ('failed' in created.cloak) {
        toast.error(new Error(cloakMessage(created.cloak)), G.create.grantAgain);
        const id = cloakItemId(spec.logo);
        if (id !== null) {
          openGrantDialog({
            recipients: [{ id: master.id, name: master.name, online: master.online }],
            spec: { ...blankSpec(id), guild_id: created.id },
          });
        }
      } else {
        toast.info(cloakMessage(created.cloak));
      }
    }
    key.current = null;
    setState(INITIAL);
    setNameStatus({ text: '', ok: null, message: '' });
    setInGuild(new Map());
    navigate({ category: 'guilds', tab: 'guilds', params: { g: String(created.id) } });
  };

  const stepError = (step: StepId): string | undefined =>
    serverStep?.step === step ? serverStep.message : undefined;

  return (
    <div className="gld-create" data-testid="create-tab">
      <form
        className="gld-create__form"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <p className="adm-muted">{G.create.intro}</p>

        <section className="gld-step" aria-labelledby="gld-step-name" data-step="name">
          <StepHead id="name" done={done.name} title={G.create.steps.name} />
          <Field
            label={G.create.name}
            hint={G.create.nameHint}
            {...(stepError('name') ? { error: stepError('name') } : {})}
          >
            <TextInput
              value={state.name}
              maxLength={40}
              data-testid="guild-name"
              autoComplete="off"
              onChange={(e) => {
                patch({ name: e.target.value });
              }}
            />
          </Field>
          <p className="adm-small" role="status" data-testid="name-status" data-ok={String(nameOk)}>
            {typed === ''
              ? ''
              : !nameCurrent
                ? G.create.nameChecking
                : nameOk
                  ? G.create.nameOk
                  : nameStatus.message}
          </p>
        </section>

        <section className="gld-step" aria-labelledby="gld-step-members" data-step="members">
          <StepHead id="members" done={done.members} title={G.create.steps.members} />
          <Field label={G.create.master} hint={G.create.masterHint}>
            {state.master ? (
              <div className="adm-row" data-testid="master-chosen">
                <span className={state.master.online ? 'adm-dot is-online' : 'adm-dot'}>
                  {state.master.online ? '●' : '○'}
                </span>
                <strong>{state.master.name}</strong>
                {inGuild.get(state.master.id) ? (
                  <Badge tone="danger">
                    {G.create.inGuild(inGuild.get(state.master.id) ?? '')}
                  </Badge>
                ) : null}
                <Button
                  variant="ghost"
                  onClick={() => {
                    patch({ master: null });
                  }}
                >
                  {G.picker.clear}
                </Button>
              </div>
            ) : (
              <CharacterPicker
                label={G.create.master}
                exclude={state.members.map((m) => m.id)}
                onPick={(c) => {
                  remember(c);
                  patch({ master: c });
                }}
              />
            )}
          </Field>
          <h4 className="gld-step__sub">{G.create.members}</h4>
          <ul className="gld-members" data-testid="member-list">
            {state.members.map((m) => (
              <li key={m.id} className="adm-row">
                <span className={m.online ? 'adm-dot is-online' : 'adm-dot'}>
                  {m.online ? '●' : '○'}
                </span>
                <strong>{m.name}</strong>
                {inGuild.get(m.id) ? (
                  <Badge tone="danger">{G.create.inGuild(inGuild.get(m.id) ?? '')}</Badge>
                ) : null}
                <Select
                  aria-label={`${G.create.rank}: ${m.name}`}
                  value={m.rank}
                  onChange={(e) => {
                    setRank(m.id, e.target.value as GuildRank);
                  }}
                >
                  {RANK_ORDER.filter((r) => r !== 'master').map((r) => (
                    <option key={r} value={r} disabled={r !== m.rank && freeOf(r) === 0}>
                      {G.create.free(G.rank[r], r === m.rank ? freeOf(r) + 1 : freeOf(r))}
                    </option>
                  ))}
                </Select>
                <Button
                  variant="ghost"
                  onClick={() => {
                    patch({ members: state.members.filter((x) => x.id !== m.id) });
                  }}
                >
                  {G.create.removeMember}
                </Button>
              </li>
            ))}
          </ul>
          <Field
            label={G.create.addMember}
            {...(stepError('members') ? { error: stepError('members') } : {})}
          >
            <CharacterPicker
              label={G.create.addMember}
              exclude={[
                ...state.members.map((m) => m.id),
                ...(state.master ? [state.master.id] : []),
              ]}
              onPick={addMember}
            />
          </Field>
          {meta.cooldowns ? (
            <Checkbox
              label={G.create.cooldown}
              checked={state.overrideCooldown}
              onChange={(e) => {
                patch({ overrideCooldown: e.target.checked });
              }}
            />
          ) : null}
        </section>

        <section className="gld-step" aria-labelledby="gld-step-logo" data-step="logo">
          <StepHead id="logo" done title={G.create.steps.logo} />
          <p className="adm-muted adm-small">{G.create.logoHint}</p>
          <LogoPicker
            value={state.logo}
            onChange={(logo) => {
              patch({ logo, giveCloak: logo === 0 ? false : state.giveCloak });
            }}
            gmHeading={G.create.gmLogos}
            noneLabel={G.create.logoNone}
          />
          <div className="gld-logo-big">
            <GuildLogo logo={state.logo} scale={6} />
            <div className="gld-cloak-box">
              <div className="adm-row" role="group" aria-label={G.create.cloak}>
                {(['male', 'female'] as const).map((s) => (
                  <Button
                    key={s}
                    variant={sex === s ? 'primary' : 'ghost'}
                    aria-pressed={sex === s}
                    onClick={() => {
                      setSex(s);
                    }}
                  >
                    {s === 'male' ? G.create.male : G.create.female}
                  </Button>
                ))}
              </div>
              <CloakPreview logo={state.logo} sex={sex} />
            </div>
          </div>
          {stepError('logo') ? <p className="adm-field__error">{stepError('logo')}</p> : null}
        </section>

        <section className="gld-step" aria-labelledby="gld-step-settings" data-step="settings">
          <StepHead id="settings" done={done.settings} title={G.create.steps.settings} />
          <div className="gld-grid">
            <Field label={G.create.level} hint={G.create.levelLimit(limit)}>
              <NumberInput
                min={1}
                max={50}
                value={state.level}
                data-testid="guild-level"
                onChange={(e) => {
                  patch({ level: clampLevel(Number(e.target.value)) });
                }}
              />
            </Field>
            <Field label={G.create.gold}>
              <NumberInput
                min={0}
                value={state.gold}
                onChange={(e) => {
                  patch({ gold: clampGold(Number(e.target.value), meta) });
                }}
              />
            </Field>
            <Field label={G.create.pxp}>
              <NumberInput
                min={0}
                value={state.pxp}
                onChange={(e) => {
                  patch({ pxp: clampPxp(Number(e.target.value)) });
                }}
              />
            </Field>
          </div>
          <Field
            label={G.create.notice}
            hint={`${String(bytes)} / ${String(meta.notice_max_bytes)} bytes`}
            {...(bytes > meta.notice_max_bytes ? { error: G.create.blocked.notice } : {})}
          >
            <TextArea
              rows={3}
              value={state.notice}
              onChange={(e) => {
                patch({ notice: e.target.value });
              }}
            />
          </Field>
          <Checkbox
            label={G.create.giveCloak}
            checked={state.giveCloak && state.logo !== 0}
            disabled={state.logo === 0}
            onChange={(e) => {
              patch({ giveCloak: e.target.checked });
            }}
          />
          {state.logo === 0 ? <p className="adm-muted adm-small">{G.create.giveCloakNo}</p> : null}
          {stepError('settings') ? (
            <p className="adm-field__error">{stepError('settings')}</p>
          ) : null}
        </section>

        <div className="adm-row">
          <Button
            variant="primary"
            type="submit"
            disabled={blocked !== null || busy}
            data-testid="create-guild"
          >
            {busy ? G.create.creating : G.create.submit}
          </Button>
          {blocked !== null ? (
            <span className="adm-muted adm-small">{G.create.blocked[blocked]}</span>
          ) : null}
        </div>
      </form>

      <aside
        className="gld-create__preview"
        aria-label={G.create.preview.title}
        data-testid="guild-preview"
      >
        <div className="gld-preview-card">
          <GuildLogo logo={state.logo} scale={3} />
          <strong className="gld-preview-card__name">
            {typed === '' ? G.create.preview.unnamed : typed}
          </strong>
          <span className="adm-muted">Lv {state.level}</span>
          <span>{state.master ? state.master.name : G.create.preview.noMaster}</span>
          <span className="adm-muted">
            {G.create.preview.members((state.master ? 1 : 0) + state.members.length, limit)}
          </span>
          {state.logo !== 0 ? <CloakPreview logo={state.logo} sex={sex} scale={1} /> : null}
        </div>
      </aside>
    </div>
  );
}
