// The two-factor dialog of the identity menu (Plan/25 section 1.3 "Optional TOTP", WP-ADMIN-28). Off: "Turn on" asks the server for a
// fresh key (nothing changes yet), shows it as a QR code, as a key in groups of four and as an `otpauth://` link, and a code from the
// app plus the current password finish it. On: a code and the password turn it off. The server enforces everything: the dialog only
// disables what it knows will be refused (the factor is required for remote sign-in and this session is remote).
import { useState, type SyntheticEvent } from 'react';
import { Button } from '../components/Button';
import { Countdown } from '../components/Countdown';
import { Dialog } from '../components/Dialog';
import { Field, TextInput } from '../components/fields';
import type { SessionInfo, TotpSetup } from '../gen/dto';
import { totpErrorText, useSession } from '../stores/session';
import { toast } from '../stores/toasts';
import { copyText } from './clipboard';
import { S } from './strings';
import { groupKey, isCompleteCode, svgDataUrl } from './totp';
import './totp.css';

const T = S.totp;

/** The code and password fields both flows end with. */
function CodeAndPassword({
  code,
  password,
  onCode,
  onPassword,
  autoFocus,
}: {
  readonly code: string;
  readonly password: string;
  readonly onCode: (v: string) => void;
  readonly onPassword: (v: string) => void;
  readonly autoFocus: boolean;
}) {
  return (
    <>
      <Field label={T.code} hint={T.codeHint}>
        <TextInput
          inputMode="numeric"
          autoComplete="one-time-code"
          autoCapitalize="off"
          spellCheck={false}
          {...(autoFocus ? { 'data-autofocus': true } : {})}
          value={code}
          onChange={(e) => {
            onCode(e.target.value);
          }}
        />
      </Field>
      <Field label={T.password}>
        <TextInput
          type="password"
          autoComplete="current-password"
          value={password}
          onChange={(e) => {
            onPassword(e.target.value);
          }}
        />
      </Field>
    </>
  );
}

function ErrorLine({ text }: { readonly text: string | null }) {
  return text ? (
    <div className="adm-alert adm-alert--danger" role="alert">
      {text}
    </div>
  ) : null;
}

/** The setup shown after "Turn on": the QR code, the key, the link, the time left. */
function SetupView({
  setup,
  onExpired,
}: {
  readonly setup: TotpSetup;
  readonly onExpired: () => void;
}) {
  const key = groupKey(setup.secret);
  return (
    <>
      <p className="adm-muted">{T.scan}</p>
      {setup.qr_svg ? (
        <img
          className="adm-totp__qr"
          data-testid="totp-qr"
          alt={T.qrAlt}
          src={svgDataUrl(setup.qr_svg)}
        />
      ) : null}
      <Field label={T.key}>
        <div className="adm-totp__bar">
          <code className="adm-totp__key" data-testid="totp-key">
            {key}
          </code>
          <Button
            size="sm"
            onClick={() => {
              if (copyText(setup.secret)) toast.success(T.keyCopied);
              else toast.info(T.copyFailed);
            }}
          >
            {T.copyKey}
          </Button>
        </div>
      </Field>
      <p className="adm-small">
        <a href={setup.uri} data-testid="totp-link">
          {T.openApp}
        </a>
        <span className="adm-muted">
          {' '}
          · {T.validFor} <Countdown until={setup.expires_at} onDone={onExpired} />
        </span>
      </p>
    </>
  );
}

function TurnOn({ info, onClose }: { readonly info: SessionInfo; readonly onClose: () => void }) {
  const busy = useSession((s) => s.busy);
  const begin = useSession((s) => s.totpBegin);
  const confirm = useSession((s) => s.totpConfirm);
  const [setup, setSetup] = useState<TotpSetup | null>(null);
  // the key whose ten minutes ran out: a new setup is a new key, so it starts fresh
  const [expiredKey, setExpiredKey] = useState<string | null>(null);
  const [code, setCode] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const blocked = info.remote && info.totp_required;
  const expired = setup !== null && expiredKey === setup.secret;

  const start = async (): Promise<void> => {
    setError(null);
    setCode('');
    try {
      setSetup(await begin());
    } catch (e) {
      setSetup(null);
      setError(totpErrorText(e));
    }
  };

  const submit = async (e: SyntheticEvent): Promise<void> => {
    e.preventDefault();
    if (!isCompleteCode(code)) {
      setError(T.notSix);
      return;
    }
    const result = await confirm(code, password);
    if (result === null) {
      toast.success(T.done);
      onClose();
    } else {
      setError(result);
    }
  };

  return (
    <Dialog title={T.title} onClose={onClose} size="sm" busy={busy}>
      {setup === null ? (
        <div className="adm-stack">
          <p className="adm-muted">{T.introOff}</p>
          {blocked ? <div className="adm-alert adm-alert--warn">{T.requiredRemoteOn}</div> : null}
          <ErrorLine text={error} />
          <Button
            variant="primary"
            loading={busy}
            disabled={blocked}
            onClick={() => {
              void start();
            }}
          >
            {T.turnOn}
          </Button>
        </div>
      ) : (
        <form className="adm-stack" onSubmit={(e) => void submit(e)} noValidate>
          <SetupView
            setup={setup}
            onExpired={() => {
              setExpiredKey(setup.secret);
            }}
          />
          {expired ? (
            <div className="adm-alert adm-alert--warn" role="alert">
              {T.expired}
            </div>
          ) : (
            <CodeAndPassword
              code={code}
              password={password}
              onCode={setCode}
              onPassword={setPassword}
              autoFocus={false}
            />
          )}
          <ErrorLine text={error} />
          <div className="adm-totp__bar">
            {expired ? null : (
              <Button
                type="submit"
                variant="primary"
                loading={busy}
                disabled={code.trim() === '' || password === ''}
              >
                {T.confirm}
              </Button>
            )}
            <button
              type="button"
              className="adm-linkbtn"
              onClick={() => {
                void start();
              }}
            >
              {T.startAgain}
            </button>
          </div>
        </form>
      )}
    </Dialog>
  );
}

function TurnOff({ info, onClose }: { readonly info: SessionInfo; readonly onClose: () => void }) {
  const busy = useSession((s) => s.busy);
  const off = useSession((s) => s.totpOff);
  const [code, setCode] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const blocked = info.remote && info.totp_required;

  const submit = async (e: SyntheticEvent): Promise<void> => {
    e.preventDefault();
    if (blocked) return;
    if (!isCompleteCode(code)) {
      setError(T.notSix);
      return;
    }
    const result = await off(code, password);
    if (result === null) {
      toast.success(T.offDone);
      onClose();
    } else {
      setError(result);
    }
  };

  return (
    <Dialog title={T.title} onClose={onClose} size="sm" busy={busy}>
      <form className="adm-stack" onSubmit={(e) => void submit(e)} noValidate>
        <p className="adm-muted">{T.introOn}</p>
        {blocked ? <div className="adm-alert adm-alert--warn">{T.requiredRemoteOff}</div> : null}
        <CodeAndPassword
          code={code}
          password={password}
          onCode={setCode}
          onPassword={setPassword}
          autoFocus
        />
        <ErrorLine text={error} />
        <Button
          type="submit"
          variant="danger"
          loading={busy}
          disabled={blocked || code.trim() === '' || password === ''}
        >
          {T.turnOff}
        </Button>
      </form>
    </Dialog>
  );
}

/** The dialog: turn two-factor on or off, whichever applies. */
export function TotpDialog({ onClose }: { readonly onClose: () => void }) {
  const info = useSession((s) => s.info);
  if (!info) return null;
  return info.totp_enabled ? (
    <TurnOff info={info} onClose={onClose} />
  ) : (
    <TurnOn info={info} onClose={onClose} />
  );
}
