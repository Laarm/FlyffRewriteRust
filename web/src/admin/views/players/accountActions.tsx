// The actions on an account (Plan/25 section 4.7): ban (duration chips, reason), unban, kick all sessions, reset the password (a one-time
// password shown once), set the authority (rank protection: never above your own letter), end the admin sessions of a staff account,
// create an account. Bans, unbans and the authority go through the account routes of 12 section 5.5; the server applies rank
// protection and writes the audit.
import { useState } from 'react';
import { passwordHash } from '../../app/pwhash';
import { runConfirmed } from '../../app/mutate';
import { DURATION_CHIPS } from '../../app/confirm';
import { Button, Dialog, Field, Select, TextInput } from '../../components';
import { AUTHORITY_ORDER, authorityAtLeast } from '../../gen/caps';
import { openDialog } from '../../stores/dialogs';
import { useSession } from '../../stores/session';
import { toast } from '../../stores/toasts';
import { copyText } from '../../app/clipboard';
import { rest } from '../../api/rest';
import {
  banAccount,
  endAdminSession,
  fetchStaff,
  resetPassword,
  setAuthority,
  unbanAccount,
} from './api';
import { reasonLine } from './format';
import { T } from './strings';

const nowSeconds = (): number => Math.floor(Date.now() / 1000);

export async function banWholeAccount(name: string): Promise<boolean> {
  const out = await runConfirmed({
    title: T.accounts.dialogs.banTitle(name),
    confirmLabel: T.accounts.actions.ban.replace('...', ''),
    danger: true,
    reason: 'required',
    durations: DURATION_CHIPS,
    durationRequired: true,
    message: T.dialogs.banMessage,
    facts: { targetName: name },
    run: (given) => {
      const secs = given.duration?.seconds;
      return banAccount(
        name,
        secs === null || secs === undefined ? null : nowSeconds() + secs,
        reasonLine(given.reason),
      );
    },
    success: T.accounts.dialogs.banned(name),
  });
  return out.ok;
}

export async function unban(name: string): Promise<boolean> {
  const out = await runConfirmed({
    title: T.accounts.dialogs.unbanTitle(name),
    confirmLabel: T.accounts.actions.unban,
    run: () => unbanAccount(name),
    success: T.accounts.dialogs.unbanned(name),
  });
  return out.ok;
}

export async function kickAccount(name: string): Promise<boolean> {
  const out = await runConfirmed({
    title: T.accounts.dialogs.kickTitle(name),
    confirmLabel: T.accounts.actions.kickAll.replace('...', ''),
    danger: true,
    reason: 'required',
    run: async (given) => {
      const reason = reasonLine(given.reason);
      const r = await rest.post<{ affected: number }>('/admin/api/kick', {
        account: name,
        ...(reason ? { reason } : {}),
      });
      return r.affected;
    },
    success: (n) => T.accounts.dialogs.kicked(n),
  });
  return out.ok;
}

/** A one-time password: 10 characters of an alphabet without look-alikes (the game limits a password to 15). */
export function oneTimePassword(random: (n: number) => number = randomIndex): string {
  const alphabet = 'abcdefghjkmnpqrstuvwxyzACDEFGHJKLMNPQRTUVWXY23456789';
  let out = '';
  for (let i = 0; i < 10; i += 1) out += alphabet.charAt(random(alphabet.length));
  return out;
}

function randomIndex(n: number): number {
  const buf = new Uint32Array(1);
  crypto.getRandomValues(buf);
  return (buf[0] ?? 0) % n;
}

function ShowOnce({
  name,
  password,
  close,
}: {
  readonly name: string;
  readonly password: string;
  readonly close: () => void;
}) {
  return (
    <Dialog
      title={T.accounts.dialogs.resetDone}
      onClose={close}
      footer={
        <>
          <Button
            onClick={() => {
              if (copyText(password)) toast.success(T.common.copied);
              else toast.info(T.common.copyFailed);
            }}
          >
            {T.common.copy}
          </Button>
          <Button variant="primary" onClick={close}>
            {T.common.close}
          </Button>
        </>
      }
    >
      <p>{T.accounts.dialogs.otpIntro}</p>
      <p>
        <code className="plr-otp" data-testid="otp">
          {password}
        </code>
      </p>
      <p className="adm-muted">{name}</p>
    </Dialog>
  );
}

export async function resetAccountPassword(name: string): Promise<boolean> {
  const out = await runConfirmed({
    title: T.accounts.dialogs.resetTitle(name),
    message: T.accounts.dialogs.resetMessage,
    confirmLabel: T.accounts.actions.resetPassword.replace('...', ''),
    danger: true,
    reason: 'none',
    facts: { irreversible: true, targetName: name },
    run: async () => {
      const otp = oneTimePassword();
      await resetPassword(name, await passwordHash(otp));
      return otp;
    },
  });
  if (!out.ok) return false;
  openDialog((close) => <ShowOnce name={name} password={out.value} close={close} />);
  return true;
}

function AuthorityField({
  own,
  initial,
  onValue,
}: {
  readonly own: string;
  readonly initial: string;
  readonly onValue: (letter: string) => void;
}) {
  const [letter, setLetter] = useState(initial);
  const allowed = ['F', 'G', 'L', 'M', 'N', 'O', 'P'].filter(
    (l) => AUTHORITY_ORDER.includes(l as never) && authorityAtLeast(own, l),
  );
  return (
    <Field label={T.accounts.dialogs.authorityField}>
      <Select
        data-autofocus
        value={letter}
        onChange={(e) => {
          setLetter(e.target.value);
          onValue(e.target.value);
        }}
      >
        {allowed.map((l) => (
          <option key={l} value={l}>
            {T.accounts.dialogs.letters[l] ?? l}
          </option>
        ))}
      </Select>
    </Field>
  );
}

export async function changeAuthority(name: string, current: string): Promise<boolean> {
  const own = useSession.getState().info?.authority ?? 'F';
  const value = { current };
  const out = await runConfirmed({
    title: T.accounts.dialogs.authorityTitle(name),
    message: (
      <AuthorityField
        own={own}
        initial={current}
        onValue={(letter) => {
          value.current = letter;
        }}
      />
    ),
    confirmLabel: T.accounts.actions.setAuthority.replace('...', ''),
    danger: true,
    reason: 'required',
    facts: { targetName: name },
    run: async () => {
      const r = await setAuthority(name, value.current);
      if (r.overridden_by_gm_config) toast.info(T.accounts.dialogs.authorityOverride);
      return value.current;
    },
    success: (letter) => T.accounts.dialogs.authorityDone(name, letter),
  });
  return out.ok;
}

/** Ends the admin sessions of a staff account (`act.accounts`). */
export async function endSessionsOf(name: string): Promise<boolean> {
  const out = await runConfirmed({
    title: T.accounts.actions.endSessions,
    message: name,
    confirmLabel: T.accounts.actions.endSessions,
    danger: true,
    run: async () => {
      const staff = await fetchStaff();
      const row = staff.staff.find((s) => s.account === name);
      const sessions = row?.sessions ?? [];
      for (const s of sessions) await endAdminSession(s.id);
      return sessions.length;
    },
    success: (n) => `Ended ${String(n)} session(s).`,
  });
  return out.ok;
}

function CreateForm({
  own,
  onChange,
}: {
  readonly own: string;
  readonly onChange: (patch: Partial<{ name: string; password: string; level: string }>) => void;
}) {
  const [name, setName] = useState('');
  const [pw, setPw] = useState('');
  const [level, setLevel] = useState('F');
  const allowed = ['F', 'G', 'L', 'M', 'N', 'O', 'P'].filter((l) => authorityAtLeast(own, l));
  return (
    <div className="adm-stack">
      <Field label={T.accounts.dialogs.createName}>
        <TextInput
          data-autofocus
          value={name}
          maxLength={15}
          autoComplete="off"
          onChange={(e) => {
            setName(e.target.value);
            onChange({ name: e.target.value });
          }}
        />
      </Field>
      <Field label={T.accounts.dialogs.createPassword} hint={T.accounts.dialogs.createPasswordHint}>
        <TextInput
          type="password"
          value={pw}
          maxLength={15}
          autoComplete="new-password"
          onChange={(e) => {
            setPw(e.target.value);
            onChange({ password: e.target.value });
          }}
        />
      </Field>
      <Field label={T.accounts.dialogs.createAuthority}>
        <Select
          value={level}
          onChange={(e) => {
            setLevel(e.target.value);
            onChange({ level: e.target.value });
          }}
        >
          {allowed.map((l) => (
            <option key={l} value={l}>
              {T.accounts.dialogs.letters[l] ?? l}
            </option>
          ))}
        </Select>
      </Field>
    </div>
  );
}

/** Creates an account (12 section 5.5; the password is hashed in the browser). */
export async function createAccount(): Promise<boolean> {
  const own = useSession.getState().info?.authority ?? 'F';
  const value = { name: '', password: '', level: 'F' };
  const out = await runConfirmed({
    title: T.accounts.dialogs.createTitle,
    message: (
      <CreateForm
        own={own}
        onChange={(patch) => {
          Object.assign(value, patch);
        }}
      />
    ),
    confirmLabel: T.accounts.dialogs.createTitle,
    run: async () => {
      const name = value.name.trim();
      const bytes = new TextEncoder().encode(value.password).length;
      if (name === '' || bytes < 3 || bytes > 15) throw new Error(T.accounts.dialogs.createInvalid);
      await rest.post('/admin/api/accounts', {
        name,
        pwhash: await passwordHash(value.password),
        auth: value.level,
      });
      return name;
    },
    success: (name) => T.accounts.dialogs.createDone(name),
  });
  return out.ok;
}
