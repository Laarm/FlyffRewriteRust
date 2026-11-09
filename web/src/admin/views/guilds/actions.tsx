// The actions on a guild (Plan/25 section 6.3): every one asks its input in a small dialog, then goes through `runConfirmed` (the reason
// where the plan wants one, the typed confirmation above the thresholds or for the irreversible disband) and calls the route with the
// idempotency key made for this operation. The drawer's action bar, the roster menu and the bank cells all use these functions.
import { useState, type ReactNode } from 'react';
import { runConfirmed } from '../../app/mutate';
import { newIdempotencyKey } from '../../api/rest';
import { CharacterPicker, type PickedCharacter } from '../../components/CharacterPicker';
import { LogoPicker } from '../../components/LogoPicker';
import { Button, Dialog, Field, NumberInput, Select, TextArea, TextInput } from '../../components';
import type { GuildDetailView, GuildEdit, GuildMemberView, GuildRank } from '../../gen/dto';
import { openDialog } from '../../stores/dialogs';
import { notifyGuildsChanged } from './changes';
import { addGuildMember, disbandGuild, editGuild, removeGuildMember } from './api';
import { G, RANK_ORDER } from './strings';
import { noticeBytes } from './wizard';

/** What a form dialog collects: a value and whether it may be sent. */
interface FormSpec<T> {
  readonly title: string;
  readonly initial: T;
  readonly submitLabel?: string;
  readonly size?: 'sm' | 'md' | 'lg';
  readonly valid: (v: T) => boolean;
  readonly body: (value: T, set: (v: T) => void) => ReactNode;
}

function FormBody<T>({
  spec,
  close,
  done,
}: {
  readonly spec: FormSpec<T>;
  readonly close: () => void;
  readonly done: (v: T | null) => void;
}) {
  const [value, setValue] = useState<T>(spec.initial);
  const cancel = (): void => {
    close();
    done(null);
  };
  return (
    <Dialog
      title={spec.title}
      size={spec.size ?? 'md'}
      onClose={cancel}
      footer={
        <>
          <Button variant="ghost" onClick={cancel}>
            {G.common.cancel}
          </Button>
          <Button
            variant="primary"
            disabled={!spec.valid(value)}
            onClick={() => {
              close();
              done(value);
            }}
          >
            {spec.submitLabel ?? G.dialogs.save}
          </Button>
        </>
      }
    >
      <form
        className="adm-stack"
        onSubmit={(e) => {
          e.preventDefault();
          if (spec.valid(value)) {
            close();
            done(value);
          }
        }}
      >
        {spec.body(value, setValue)}
      </form>
    </Dialog>
  );
}

/** Opens a form dialog; resolves the collected value, or `null` when it was cancelled. */
export function askForm<T>(spec: FormSpec<T>): Promise<T | null> {
  return new Promise((resolve) => {
    openDialog((close) => <FormBody spec={spec} close={close} done={resolve} />);
  });
}

export interface GuildRef {
  readonly id: number;
  readonly name: string;
}

const displayName = (g: GuildRef): string => (g.name === '' ? G.list.unnamed(g.id) : g.name);

/** Runs one `PATCH` edit under its confirmation. Resolves `true` when it went through. */
export async function applyEdit(
  guild: GuildRef,
  edit: GuildEdit,
  opts: {
    readonly title: string;
    readonly reason?: 'optional' | 'required';
    readonly message?: ReactNode;
    readonly gold?: number;
    readonly irreversible?: boolean;
    readonly items?: number;
    readonly danger?: boolean;
  },
): Promise<boolean> {
  const key = newIdempotencyKey();
  const out = await runConfirmed({
    title: opts.title,
    ...(opts.message === undefined ? {} : { message: opts.message }),
    confirmLabel: G.dialogs.save,
    ...(opts.danger === undefined ? {} : { danger: opts.danger }),
    reason: opts.reason ?? 'optional',
    facts: {
      ...(guild.name === '' ? {} : { targetName: guild.name }),
      ...(opts.gold === undefined ? {} : { gold: opts.gold }),
      ...(opts.irreversible === undefined ? {} : { irreversible: opts.irreversible }),
      ...(opts.items === undefined ? {} : { items: opts.items }),
    },
    run: (given) => editGuild(guild.id, edit, given, key),
    success: (r) => (r.changed ? G.dialogs.changed : G.dialogs.unchanged),
  });
  if (out.ok) notifyGuildsChanged();
  return out.ok;
}

export const rename = async (g: GuildRef): Promise<boolean> => {
  const name = await askForm<string>({
    title: G.dialogs.renameTitle,
    initial: g.name,
    valid: (v) => v.trim().length >= 3 && v.trim() !== g.name,
    body: (v, set) => (
      <Field label={G.dialogs.renameLabel} hint={G.create.nameHint}>
        <TextInput
          value={v}
          maxLength={40}
          autoComplete="off"
          onChange={(e) => {
            set(e.target.value);
          }}
        />
      </Field>
    ),
  });
  return name === null
    ? false
    : await applyEdit(g, { rename: name.trim() }, { title: G.dialogs.renameTitle });
};

export const changeLogo = async (g: GuildRef, current: number): Promise<boolean> => {
  const logo = await askForm<number>({
    title: G.dialogs.logoTitle,
    initial: current === 999 ? 0 : current,
    size: 'lg',
    valid: (v) => v !== current,
    body: (v, set) => (
      <>
        <p className="adm-muted adm-small">{G.dialogs.logoNote}</p>
        <LogoPicker
          value={v}
          onChange={set}
          gmHeading={G.create.gmLogos}
          noneLabel={G.create.logoNone}
        />
      </>
    ),
  });
  return logo === null ? false : await applyEdit(g, { logo }, { title: G.dialogs.logoTitle });
};

export const changeMaster = async (
  g: GuildRef,
  members: readonly GuildMemberView[],
  preselect?: number,
): Promise<boolean> => {
  const candidates = members.filter((m) => m.rank !== 'master');
  if (candidates.length === 0) return false;
  const pick = preselect ?? candidates[0]?.character ?? 0;
  const id = await askForm<number>({
    title: G.dialogs.masterTitle,
    initial: pick,
    valid: (v) => candidates.some((m) => m.character === v),
    body: (v, set) => (
      <>
        <p className="adm-muted adm-small">{G.dialogs.masterNote}</p>
        <Field label={G.dialogs.masterPick}>
          <Select
            value={v}
            onChange={(e) => {
              set(Number(e.target.value));
            }}
          >
            {candidates.map((m) => (
              <option key={m.character} value={m.character}>
                {m.name} ({G.rank[m.rank]})
              </option>
            ))}
          </Select>
        </Field>
      </>
    ),
  });
  return id === null ? false : await applyEdit(g, { master: id }, { title: G.dialogs.masterTitle });
};

async function askNumber(
  title: string,
  label: string,
  initial: number,
  min: number,
  max: number,
  hint?: string,
): Promise<number | null> {
  return await askForm<number>({
    title,
    initial,
    valid: (v) => Number.isInteger(v) && v >= min && v <= max,
    body: (v, set) => (
      <Field label={label} {...(hint ? { hint } : {})}>
        <NumberInput
          min={min}
          max={max}
          value={v}
          onChange={(e) => {
            set(Math.trunc(Number(e.target.value)));
          }}
        />
      </Field>
    ),
  });
}

export const setLevel = async (g: GuildRef, level: number, maxLevel: number): Promise<boolean> => {
  const v = await askNumber(G.dialogs.levelTitle, G.drawer.level, level, 1, maxLevel);
  return v === null ? false : await applyEdit(g, { level: v }, { title: G.dialogs.levelTitle });
};

export const setPxp = async (g: GuildRef, pxp: number): Promise<boolean> => {
  const v = await askNumber(G.dialogs.pxpTitle, G.drawer.pxp, pxp, 0, 0xffff_ffff);
  return v === null ? false : await applyEdit(g, { pxp: v }, { title: G.dialogs.pxpTitle });
};

export const changeGold = async (g: GuildRef, pool: number, goldMax: number): Promise<boolean> => {
  const v = await askNumber(
    G.dialogs.goldTitle,
    G.dialogs.goldLabel,
    0,
    -pool,
    goldMax - pool,
    G.dialogs.goldNow(pool),
  );
  if (v === null || v === 0) return false;
  return await applyEdit(
    g,
    { gold: v },
    { title: G.dialogs.goldTitle, reason: 'required', gold: Math.abs(v) },
  );
};

export const setNotice = async (
  g: GuildRef,
  notice: string,
  maxBytes: number,
): Promise<boolean> => {
  const v = await askForm<string>({
    title: G.dialogs.noticeTitle,
    initial: notice,
    valid: (t) => noticeBytes(t) <= maxBytes && t !== notice,
    body: (t, set) => (
      <Field label={G.drawer.notice} hint={G.dialogs.noticeBytes(noticeBytes(t), maxBytes)}>
        <TextArea
          rows={4}
          value={t}
          onChange={(e) => {
            set(e.target.value);
          }}
        />
      </Field>
    ),
  });
  return v === null ? false : await applyEdit(g, { notice: v }, { title: G.dialogs.noticeTitle });
};

export const addMember = async (g: GuildRef, d: GuildDetailView): Promise<boolean> => {
  interface Pick {
    readonly c: PickedCharacter | null;
    readonly rank: GuildRank;
  }
  const used = (rank: GuildRank): number => d.members.filter((m) => m.rank === rank).length;
  const free = (i: number): number =>
    Math.max(0, (d.rank_caps[i] ?? 0) - used(RANK_ORDER[i] ?? 'member'));
  const room = Math.max(0, d.guild.max_members - d.members.length);
  const pick = await askForm<Pick>({
    title: G.dialogs.addTitle,
    initial: { c: null, rank: 'member' },
    valid: (v) => v.c !== null,
    body: (v, set) => (
      <>
        {v.c ? (
          <p className="adm-row">
            <strong data-testid="add-member-chosen">{v.c.name}</strong>
            <Button
              variant="ghost"
              onClick={() => {
                set({ ...v, c: null });
              }}
            >
              {G.picker.clear}
            </Button>
          </p>
        ) : (
          <CharacterPicker
            label={G.dialogs.addTitle}
            autoFocus
            exclude={d.members.map((m) => m.character)}
            onPick={(c) => {
              set({ ...v, c });
            }}
          />
        )}
        <Field label={G.dialogs.addRank}>
          <Select
            value={v.rank}
            onChange={(e) => {
              set({ ...v, rank: e.target.value as GuildRank });
            }}
          >
            {RANK_ORDER.map((r, i) =>
              r === 'master' ? null : (
                <option key={r} value={r} disabled={Math.min(free(i), room) === 0}>
                  {G.dialogs.addFree(G.rank[r], Math.min(free(i), room))}
                </option>
              ),
            )}
          </Select>
        </Field>
      </>
    ),
  });
  if (pick?.c == null) return false;
  const c = pick.c;
  const out = await runConfirmed({
    title: G.dialogs.addTitle,
    message: `${c.name} - ${G.rank[pick.rank]}`,
    confirmLabel: G.actions.addMember.replace('...', ''),
    reason: 'optional',
    facts: g.name === '' ? {} : { targetName: g.name },
    run: (given) => addGuildMember(g.id, c.id, pick.rank, given),
    success: (r) => (r.changed ? G.dialogs.changed : G.dialogs.unchanged),
  });
  if (out.ok) notifyGuildsChanged();
  return out.ok;
};

export const removeMember = async (g: GuildRef, m: GuildMemberView): Promise<boolean> => {
  const out = await runConfirmed({
    title: G.dialogs.removeTitle(m.name),
    confirmLabel: G.roster.menu.remove.replace(' from guild...', ''),
    reason: 'optional',
    run: (given) => removeGuildMember(g.id, m.character, given),
    success: (r) => (r.changed ? G.dialogs.changed : G.dialogs.unchanged),
  });
  if (out.ok) notifyGuildsChanged();
  return out.ok;
};

export const changeRank = async (
  g: GuildRef,
  d: GuildDetailView,
  m: GuildMemberView,
): Promise<boolean> => {
  const rank = await askForm<GuildRank>({
    title: G.dialogs.rankTitle(m.name),
    initial: m.rank === 'master' ? 'member' : m.rank,
    valid: (v) => v !== m.rank,
    body: (v, set) => (
      <Field label={G.dialogs.addRank}>
        <Select
          value={v}
          onChange={(e) => {
            set(e.target.value as GuildRank);
          }}
        >
          {RANK_ORDER.map((r, i) => {
            if (r === 'master') return null;
            const used = d.members.filter((x) => x.rank === r).length;
            const free = Math.max(0, (d.rank_caps[i] ?? 0) - used);
            return (
              <option key={r} value={r} disabled={free === 0 && r !== m.rank}>
                {G.dialogs.addFree(G.rank[r], free)}
              </option>
            );
          })}
        </Select>
      </Field>
    ),
  });
  return rank === null
    ? false
    : await applyEdit(g, { rank: [m.character, rank] }, { title: G.dialogs.rankTitle(m.name) });
};

export const makeMaster = async (g: GuildRef, m: GuildMemberView): Promise<boolean> =>
  await applyEdit(
    g,
    { master: m.character },
    { title: G.dialogs.makeMasterTitle(m.name), message: G.dialogs.masterNote },
  );

export const disband = async (g: GuildRef, members: number): Promise<boolean> => {
  const out = await runConfirmed({
    title: G.dialogs.disbandTitle(displayName(g)),
    message: `${G.dialogs.disbandMessage} (${String(members)})`,
    confirmLabel: G.actions.disband.replace('...', ''),
    danger: true,
    reason: 'required',
    facts: { irreversible: true, ...(g.name === '' ? {} : { targetName: g.name }) },
    run: (given) => disbandGuild(g.id, given),
    success: G.dialogs.changed,
  });
  if (out.ok) notifyGuildsChanged();
  return out.ok;
};
