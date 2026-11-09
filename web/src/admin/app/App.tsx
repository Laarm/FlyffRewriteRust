// The app root: asks the server who is signed in and shows the login, the forced password change (the 428 flow) or the shell.
import { useEffect, useRef } from 'react';
import { DialogHost } from '../components/DialogHost';
import { ToastHost } from '../components/Toast';
import { Skeleton } from '../components/Spinner';
import { closeAllDialogs } from '../stores/dialogs';
import { closeAdminSocket } from '../stores/live';
import { startRouteListener } from '../stores/route';
import { useSession } from '../stores/session';
import { LoginView } from './Login';
import { ForcedPasswordView } from './PasswordChange';
import { Shell } from './Shell';
import { S } from './strings';

export function App() {
  const phase = useSession((s) => s.phase);
  const forced = useSession((s) => s.info?.must_change_password === true);
  const init = useSession((s) => s.init);

  useEffect(() => {
    void init();
    return startRouteListener();
  }, [init]);

  // the socket belongs to the identity it was opened with: a sign-in as staff, a sign-out to the owner of the host or a takeover after an ended session
  // needs a new one (the views subscribe again, `stores/live.ts` epoch)
  const identity = useSession((s) => s.info?.identity ?? null);
  const known = useRef<string | null>(null);
  useEffect(() => {
    if (known.current !== null && identity !== null && known.current !== identity)
      closeAdminSocket();
    if (identity !== null) known.current = identity;
  }, [identity]);

  // a lost session takes its dialogs with it (a half-filled confirmation must not outlive the login that allowed it)
  useEffect(() => {
    if (phase === 'anonymous') {
      closeAllDialogs();
      closeAdminSocket();
    }
  }, [phase]);

  return (
    <>
      {phase === 'loading' ? (
        <main className="adm-center" aria-busy="true">
          <div className="adm-card adm-card--narrow">
            <div className="adm-brand adm-brand--large">
              <span className="adm-brand__mark" aria-hidden="true" />
              {S.appName}
            </div>
            <Skeleton lines={2} />
          </div>
        </main>
      ) : phase === 'anonymous' ? (
        <LoginView />
      ) : forced ? (
        <ForcedPasswordView />
      ) : (
        <Shell />
      )}
      <DialogHost />
      <ToastHost />
    </>
  );
}
