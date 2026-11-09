// The actions on a character (Plan/25 section 4.3, 4.6, 4.1): kick, mute, ban, login block, message, return to town, heal, resurrect,
// teleport, rename, undelete, show on map and grant. The drawer's action bar, the rows of the Online and Characters tables and the
// lines of the chat monitor all use these functions, so every route gets the same dialog: a reason where the plan asks for one, duration
// chips for mute and ban, a typed confirmation where the server asks for it (`runConfirmed`).
import { useState } from 'react';
import { runConfirmed } from '../../app/mutate';
import { DURATION_CHIPS } from '../../app/confirm';
import { Field, NumberInput, Select, TextArea, TextInput } from '../../components';
import { openGrantDialog } from '../../grant';
import { navigate } from '../../stores/route';
import { toast } from '../../stores/toasts';
import {
  banCharacter,
  healObjects,
  kickCharacter,
  messageCharacters,
  moveCharacter,
  muteCharacter,
  renameCharacter,
  resurrectCharacter,
  setLoginBlock,
  teleportCharacters,
  undeleteCharacter,
  unmuteCharacter,
} from './api';
import { formatDateTime, reasonLine } from './format';
import { T } from './strings';

/** Who an action is about; whatever is unknown is left out. */
export interface Target {
  readonly id: number;
  readonly name: string;
  /** The account name (the account drawer, the ban). */
  readonly account?: string;
  readonly online?: boolean;
  /** The avatar's object id while online (heal, show on map). */
  readonly objId?: number | undefined;
  readonly world?: number;
  readonly layer?: number;
  readonly x?: number;
  readonly z?: number;
  readonly deleted?: boolean;
  readonly blockedUntil?: number | null;
}

const nowSeconds = (): number => Math.floor(Date.now() / 1000);

const untilOf = (seconds: number | null | undefined): number | null =>
  seconds === null || seconds === undefined ? null : nowSeconds() + seconds;

export async function kick(t: Target, quote?: string): Promise<boolean> {
  const out = await runConfirmed({
    title: T.dialogs.kickTitle(t.name),
    confirmLabel: T.actions.kick.replace('...', ''),
    danger: true,
    reason: 'required',
    message: quote ? <Quote text={quote} /> : undefined,
    run: async (given) => {
      const line = reasonLine(given.reason);
      const withQuote = quote ? `${line ?? ''}${line ? ' - ' : ''}${T.chat.quoted(quote)}` : line;
      return await kickCharacter(t.name, withQuote);
    },
    success: (r) => T.dialogs.kickDone(r.affected),
  });
  return out.ok;
}

function Quote({ text }: { readonly text: string }) {
  return <blockquote className="plr-quote">{text}</blockquote>;
}

export async function mute(t: Target, quote?: string): Promise<boolean> {
  const out = await runConfirmed({
    title: T.dialogs.muteTitle(t.name),
    confirmLabel: T.actions.mute.replace('...', ''),
    reason: 'required',
    durations: DURATION_CHIPS,
    durationRequired: true,
    message: quote ? <Quote text={quote} /> : undefined,
    run: (given) => {
      const reason = quote
        ? {
            ...(given.reason ?? {}),
            text: `${given.reason?.text ?? ''}${given.reason?.text ? ' - ' : ''}${T.chat.quoted(quote)}`,
          }
        : given.reason;
      return muteCharacter(t.id, given.duration?.seconds ?? null, {
        ...given,
        ...(reason ? { reason } : {}),
      });
    },
    success: T.dialogs.muted(t.name),
  });
  return out.ok;
}

export async function unmute(t: Target): Promise<boolean> {
  const out = await runConfirmed({
    title: T.actions.unmute,
    message: t.name,
    confirmLabel: T.actions.unmute,
    run: (given) => unmuteCharacter(t.id, given),
    success: T.dialogs.unmuted(t.name),
  });
  return out.ok;
}

export async function ban(t: Target, quote?: string): Promise<boolean> {
  const out = await runConfirmed({
    title: T.dialogs.banTitle(t.name),
    confirmLabel: T.actions.ban.replace('...', ''),
    danger: true,
    reason: 'required',
    durations: DURATION_CHIPS,
    durationRequired: true,
    message: (
      <>
        <p>{T.dialogs.banMessage}</p>
        {quote ? <Quote text={quote} /> : null}
      </>
    ),
    facts: { targetName: t.name },
    run: (given) => {
      const reason = quote
        ? {
            ...(given.reason ?? {}),
            text: `${given.reason?.text ?? ''}${given.reason?.text ? ' - ' : ''}${T.chat.quoted(quote)}`,
          }
        : given.reason;
      return banCharacter(t.id, untilOf(given.duration?.seconds), {
        ...given,
        ...(reason ? { reason } : {}),
      });
    },
    success: T.dialogs.banned(t.account ?? t.name),
  });
  return out.ok;
}

export async function loginBlock(t: Target): Promise<boolean> {
  const out = await runConfirmed({
    title: T.dialogs.loginBlockTitle(t.name),
    confirmLabel: T.actions.loginBlock.replace('...', ''),
    danger: true,
    reason: 'required',
    durations: DURATION_CHIPS,
    durationRequired: true,
    message: T.dialogs.loginBlockMessage,
    run: (given) =>
      setLoginBlock(t.id, { until: untilOf(given.duration?.seconds), lift: false }, given),
    success: T.dialogs.blockSet(t.name),
  });
  return out.ok;
}

export async function liftLoginBlock(t: Target): Promise<boolean> {
  const out = await runConfirmed({
    title: T.dialogs.liftBlockTitle(t.name),
    confirmLabel: T.actions.liftBlock,
    reason: 'required',
    run: (given) => setLoginBlock(t.id, { until: null, lift: true }, given),
    success: T.dialogs.blockLifted(t.name),
  });
  return out.ok;
}

function MessageFields({
  onText,
  onStyle,
}: {
  readonly onText: (text: string) => void;
  readonly onStyle: (style: 'notice' | 'whisper') => void;
}) {
  const [text, setText] = useState('');
  const [style, setStyle] = useState<'notice' | 'whisper'>('notice');
  return (
    <div className="adm-stack">
      <Field label={T.dialogs.messageText}>
        <TextArea
          rows={3}
          maxLength={400}
          data-autofocus
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            onText(e.target.value);
          }}
        />
      </Field>
      <Field label={T.dialogs.messageStyle}>
        <Select
          value={style}
          onChange={(e) => {
            const s = e.target.value === 'whisper' ? 'whisper' : 'notice';
            setStyle(s);
            onStyle(s);
          }}
        >
          <option value="notice">{T.dialogs.messageNotice}</option>
          <option value="whisper">{T.dialogs.messageWhisper}</option>
        </Select>
      </Field>
    </div>
  );
}

/** Sends a notice or a whisper to online characters. */
export async function message(targets: readonly Target[]): Promise<boolean> {
  const value: { text: string; style: 'notice' | 'whisper' } = { text: '', style: 'notice' };
  const who = targets.length === 1 ? (targets[0]?.name ?? '') : `${String(targets.length)} players`;
  const out = await runConfirmed({
    title: T.dialogs.messageTitle(who),
    confirmLabel: 'Send',
    message: (
      <MessageFields
        onText={(text) => {
          value.text = text;
        }}
        onStyle={(style) => {
          value.style = style;
        }}
      />
    ),
    facts: {
      targets: targets.length,
      ...(targets.length === 1 && targets[0] ? { targetName: targets[0].name } : {}),
    },
    run: (given) => {
      if (value.text.trim() === '') throw new Error(T.dialogs.messageEmpty);
      return messageCharacters(
        targets.map((t) => t.id),
        value.text.trim(),
        value.style,
        given,
      );
    },
    success: T.dialogs.messageSent,
  });
  return out.ok;
}

/** Kicks several online players with one reason (typed confirmation above ten). */
export async function kickMany(targets: readonly Target[]): Promise<boolean> {
  const out = await runConfirmed({
    title: T.dialogs.kickTitle(`${String(targets.length)} players`),
    confirmLabel: T.actions.kick.replace('...', ''),
    danger: true,
    reason: 'required',
    facts: { targets: targets.length },
    run: async (given) => {
      const line = reasonLine(given.reason);
      let n = 0;
      for (const t of targets) n += (await kickCharacter(t.name, line)).affected;
      return n;
    },
    success: (n) => `Kicked ${String(n)} player(s).`,
  });
  return out.ok;
}

export async function returnToTown(t: Target): Promise<boolean> {
  const out = await runConfirmed({
    title: T.dialogs.returnTitle(t.name),
    message: T.dialogs.returnMessage,
    confirmLabel: T.actions.returnToTown,
    run: (given) => moveCharacter(t.id, 'revival', given),
    success: (r) => T.dialogs.returned(r.summary),
  });
  return out.ok;
}

export async function resurrect(t: Target): Promise<boolean> {
  const out = await runConfirmed({
    title: T.dialogs.resurrectTitle(t.name),
    confirmLabel: T.actions.resurrect,
    run: (given) => resurrectCharacter(t.id, given),
    success: `${t.name} is alive.`,
  });
  return out.ok;
}

export async function heal(t: Target): Promise<boolean> {
  if (t.objId === undefined) {
    toast.info(T.actions.onlineOnly);
    return false;
  }
  const obj = t.objId;
  const out = await runConfirmed({
    title: T.dialogs.healTitle(t.name),
    confirmLabel: T.actions.heal,
    run: (given) => healObjects([obj], given),
    success: `${t.name} is healed.`,
  });
  return out.ok;
}

interface Where {
  world: string;
  layer: string;
  x: string;
  y: string;
  z: string;
}

function TeleportFields({
  initial,
  onChange,
  hint,
}: {
  readonly initial: Where;
  readonly onChange: (w: Where) => void;
  readonly hint: string | null;
}) {
  const [where, setWhere] = useState<Where>(initial);
  const bind = (k: keyof Where) => ({
    value: where[k],
    onChange: (e: { target: { value: string } }) => {
      const next = { ...where, [k]: e.target.value };
      setWhere(next);
      onChange(next);
    },
  });
  return (
    <div className="adm-stack">
      {hint ? <p className="adm-muted">{hint}</p> : null}
      <div className="adm-row">
        <Field label={T.dialogs.teleportWorld}>
          <NumberInput data-autofocus {...bind('world')} />
        </Field>
        <Field label={T.dialogs.teleportLayer}>
          <NumberInput {...bind('layer')} />
        </Field>
      </div>
      <div className="adm-row">
        <Field label={T.dialogs.teleportX}>
          <NumberInput {...bind('x')} />
        </Field>
        <Field label={T.dialogs.teleportY}>
          <NumberInput {...bind('y')} />
        </Field>
        <Field label={T.dialogs.teleportZ}>
          <NumberInput {...bind('z')} />
        </Field>
      </div>
    </div>
  );
}

/** Teleports an online character to a point of a world (the live map's click-to-teleport is WP-ADMIN-13). */
export async function teleport(t: Target): Promise<boolean> {
  const value: Where = {
    world: t.world === undefined ? '' : String(t.world),
    layer: String(t.layer ?? 0),
    x: t.x === undefined ? '' : String(Math.round(t.x)),
    y: '100',
    z: t.z === undefined ? '' : String(Math.round(t.z)),
  };
  let current: Where = value;
  const out = await runConfirmed({
    title: T.dialogs.teleportTitle(t.name),
    confirmLabel: T.actions.teleport.replace('...', ''),
    message: (
      <TeleportFields
        initial={value}
        hint={t.online === false ? T.actions.onlineOnly : null}
        onChange={(w) => {
          current = w;
        }}
      />
    ),
    facts: { targetName: t.name },
    run: (given) => {
      const [world, layer, x, y, z] = [
        current.world,
        current.layer,
        current.x,
        current.y,
        current.z,
      ].map(Number);
      if (
        world === undefined ||
        layer === undefined ||
        x === undefined ||
        y === undefined ||
        z === undefined ||
        [world, x, z].some((n) => !Number.isFinite(n)) ||
        current.world.trim() === '' ||
        current.x.trim() === '' ||
        current.z.trim() === ''
      ) {
        throw new Error(T.dialogs.teleportInvalid);
      }
      return teleportCharacters(
        [t.id],
        {
          world,
          layer: Number.isFinite(layer) ? layer : 0,
          pos: [x, Number.isFinite(y) ? y : 100, z],
        },
        given,
      );
    },
    success: T.dialogs.teleportDone,
  });
  return out.ok;
}

function TextField({
  label,
  onValue,
  initial = '',
  hint,
}: {
  readonly label: string;
  readonly onValue: (text: string) => void;
  readonly initial?: string;
  readonly hint?: string;
}) {
  const [text, setText] = useState(initial);
  return (
    <Field label={label} {...(hint ? { hint } : {})}>
      <TextInput
        data-autofocus
        value={text}
        maxLength={32}
        onChange={(e) => {
          setText(e.target.value);
          onValue(e.target.value);
        }}
      />
    </Field>
  );
}

export async function rename(t: Target): Promise<boolean> {
  const name = { current: t.name };
  const out = await runConfirmed({
    title: T.dialogs.renameTitle(t.name),
    message: (
      <TextField
        label={T.dialogs.renameField}
        initial={t.name}
        onValue={(text) => {
          name.current = text;
        }}
      />
    ),
    confirmLabel: T.actions.rename.replace('...', ''),
    danger: true,
    reason: 'required',
    facts: { irreversible: true, targetName: t.name },
    run: (given) => renameCharacter(t.id, { name: name.current.trim() }, given),
    success: () => T.dialogs.renameDone(name.current.trim()),
  });
  return out.ok;
}

export async function undelete(t: Target): Promise<boolean> {
  const slot = { current: '0' };
  const out = await runConfirmed({
    title: T.dialogs.undeleteTitle(t.name),
    message: (
      <TextField
        label={T.dialogs.undeleteSlot}
        initial="0"
        hint="0 to 2"
        onValue={(text) => {
          slot.current = text;
        }}
      />
    ),
    confirmLabel: T.actions.undelete.replace('...', ''),
    reason: 'required',
    facts: { targetName: t.name },
    run: (given) =>
      undeleteCharacter(
        t.id,
        { slot: Math.max(0, Math.min(2, Math.trunc(Number(slot.current)) || 0)) },
        given,
      ),
    success: T.dialogs.undeleteDone,
  });
  return out.ok;
}

/** Opens the live map on the character (WP-ADMIN-09 builds the view; the link follows plan 2.3). */
export function showOnMap(t: Target): void {
  const params: Record<string, string> = {};
  if (t.world !== undefined) params['w'] = String(t.world);
  if (t.layer !== undefined) params['l'] = String(t.layer);
  if (t.x !== undefined) params['x'] = String(Math.round(t.x));
  if (t.z !== undefined) params['z'] = String(Math.round(t.z));
  if (t.objId !== undefined) params['sel'] = `p:${String(t.objId)}`;
  navigate({ category: 'world', tab: 'map', params });
}

/** Opens the grant dialog for the character. */
export function grantTo(t: Target): void {
  openGrantDialog({ recipients: [{ id: t.id, name: t.name, online: t.online === true }] });
}

export const blockedText = (until: number | null | undefined): string =>
  until === null || until === undefined
    ? T.characters.blockedForever
    : T.characters.blockedUntil(formatDateTime(until));
