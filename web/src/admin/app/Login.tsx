// The login view (Plan/25 section 1.3): shown by the SPA when no identity is known (a remote caller with `admin.allow_remote`, or a
// host with `admin.owner_loopback = false`). Staff sign in with their game account; the owner may exchange the content of
// `data/admin.token` for a cookie session. The password is hashed here (app/pwhash.ts): the plaintext never reaches the server.
import { useState, type SyntheticEvent } from 'react';
import { Button } from '../components/Button';
import { Field, TextInput } from '../components/fields';
import { useSession } from '../stores/session';
import { S } from './strings';

/** The sign-in form (used by the full-page view and by the "Sign in as staff" dialog). */
export function LoginForm({ onDone }: { readonly onDone?: () => void }) {
  const busy = useSession((s) => s.busy);
  const error = useSession((s) => s.loginError);
  const notice = useSession((s) => s.notice);
  const login = useSession((s) => s.login);
  const exchangeToken = useSession((s) => s.exchangeToken);
  const [account, setAccount] = useState('');
  const [password, setPassword] = useState('');
  const [code, setCode] = useState('');
  const [token, setToken] = useState('');
  const [byToken, setByToken] = useState(false);

  const submit = async (e: SyntheticEvent): Promise<void> => {
    e.preventDefault();
    const ok = byToken ? await exchangeToken(token) : await login(account, password, code);
    if (ok) {
      setPassword('');
      setCode('');
      setToken('');
      onDone?.();
    }
  };

  return (
    <form className="adm-stack" onSubmit={(e) => void submit(e)} noValidate>
      {notice ? <div className="adm-alert adm-alert--warn">{notice}</div> : null}
      {byToken ? (
        <Field label={S.login.token} hint={S.login.tokenHint}>
          <TextInput
            type="password"
            autoComplete="off"
            data-autofocus
            value={token}
            onChange={(e) => {
              setToken(e.target.value);
            }}
          />
        </Field>
      ) : (
        <>
          <Field label={S.login.account} hint={S.login.staffHint}>
            <TextInput
              autoComplete="username"
              autoCapitalize="off"
              spellCheck={false}
              data-autofocus
              value={account}
              onChange={(e) => {
                setAccount(e.target.value);
              }}
            />
          </Field>
          <Field label={S.login.password}>
            <TextInput
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(e) => {
                setPassword(e.target.value);
              }}
            />
          </Field>
          <Field label={S.login.code} hint={S.login.codeHint}>
            <TextInput
              inputMode="numeric"
              autoComplete="one-time-code"
              autoCapitalize="off"
              spellCheck={false}
              value={code}
              onChange={(e) => {
                setCode(e.target.value);
              }}
            />
          </Field>
        </>
      )}
      {error ? (
        <div className="adm-alert adm-alert--danger" role="alert">
          {error}
        </div>
      ) : null}
      <Button
        type="submit"
        variant="primary"
        loading={busy}
        disabled={byToken ? token.trim() === '' : account.trim() === '' || password === ''}
      >
        {byToken ? S.login.tokenSubmit : S.login.submit}
      </Button>
      <button
        type="button"
        className="adm-linkbtn"
        onClick={() => {
          setByToken((v) => !v);
        }}
      >
        {byToken ? S.login.useAccount : S.login.useToken}
      </button>
    </form>
  );
}

/** The full-page login. */
export function LoginView() {
  return (
    <main className="adm-center">
      <div className="adm-card adm-card--narrow">
        <div className="adm-brand adm-brand--large">
          <span className="adm-brand__mark" aria-hidden="true" />
          {S.appName}
        </div>
        <h1>{S.login.title}</h1>
        <LoginForm />
        <p className="adm-muted adm-small">
          <a href="/admin/lite">{S.rescue.link}</a>
        </p>
      </div>
    </main>
  );
}
