// The password change (Plan/25 section 1.3): the forced full-page flow while `must_change_password` is set (every route but the
// session routes answers `428 password_change_required`, so nothing else works until it is done), and the same form as a dialog from
// the identity menu. The old and the new password are hashed in the browser; the server stores argon2id of the new hash.
import { useState, type SyntheticEvent } from 'react';
import { Button } from '../components/Button';
import { Dialog } from '../components/Dialog';
import { Field, TextInput } from '../components/fields';
import { toast } from '../stores/toasts';
import { useSession } from '../stores/session';
import { isValidPassword } from './pwhash';
import { S } from './strings';

/** What is wrong with the three fields, or `null`. */
export function passwordProblem(current: string, next: string, repeat: string): string | null {
  if (!isValidPassword(next)) return S.password.invalid;
  if (next !== repeat) return S.password.mismatch;
  if (next === current) return S.password.same;
  return null;
}

export function PasswordForm({ onDone }: { readonly onDone?: () => void }) {
  const changePassword = useSession((s) => s.changePassword);
  const busy = useSession((s) => s.busy);
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [repeat, setRepeat] = useState('');
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: SyntheticEvent): Promise<void> => {
    e.preventDefault();
    const problem = passwordProblem(current, next, repeat);
    if (problem) {
      setError(problem);
      return;
    }
    const result = await changePassword(current, next);
    if (result === null) {
      toast.success(S.password.done);
      onDone?.();
    } else {
      setError(result);
    }
  };

  return (
    <form className="adm-stack" onSubmit={(e) => void submit(e)} noValidate>
      <Field label={S.password.current}>
        <TextInput
          type="password"
          autoComplete="current-password"
          data-autofocus
          value={current}
          onChange={(e) => {
            setCurrent(e.target.value);
          }}
        />
      </Field>
      <Field label={S.password.next} hint={S.password.hint}>
        <TextInput
          type="password"
          autoComplete="new-password"
          value={next}
          onChange={(e) => {
            setNext(e.target.value);
          }}
        />
      </Field>
      <Field label={S.password.repeat}>
        <TextInput
          type="password"
          autoComplete="new-password"
          value={repeat}
          onChange={(e) => {
            setRepeat(e.target.value);
          }}
        />
      </Field>
      {error ? (
        <div className="adm-alert adm-alert--danger" role="alert">
          {error}
        </div>
      ) : null}
      <Button
        type="submit"
        variant="primary"
        loading={busy}
        disabled={current === '' || next === '' || repeat === ''}
      >
        {S.password.submit}
      </Button>
    </form>
  );
}

/** The forced flow: the whole page, nothing else reachable (the shell is not rendered at all). */
export function ForcedPasswordView() {
  const logout = useSession((s) => s.logout);
  return (
    <main className="adm-center">
      <div className="adm-card adm-card--narrow" data-testid="forced-password">
        <div className="adm-brand adm-brand--large">
          <span className="adm-brand__mark" aria-hidden="true" />
          {S.appName}
        </div>
        <h1>{S.password.forcedTitle}</h1>
        <p className="adm-muted">{S.password.forcedBody}</p>
        <PasswordForm />
        <button
          type="button"
          className="adm-linkbtn"
          onClick={() => {
            void logout();
          }}
        >
          {S.identity.signOut}
        </button>
      </div>
    </main>
  );
}

/** The dialog opened from the identity menu. */
export function PasswordDialog({ onClose }: { readonly onClose: () => void }) {
  return (
    <Dialog title={S.password.title} onClose={onClose} size="sm">
      <PasswordForm onDone={onClose} />
    </Dialog>
  );
}
