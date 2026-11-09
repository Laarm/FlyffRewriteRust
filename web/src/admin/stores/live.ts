// The live socket of this console tab (Plan/25 section 11.4: ONE `/admin/ws` per tab, protocol `flyff.admin.v1`). `adminSocket()` creates and connects it on first use and
// every view shares it (`adminSocket().subscribeMap(...)`, `subscribeChat(...)`, ...); the shell's own subscription is the 1 Hz status of the top bar. A session that ends
// (ban, authority or password change, expiry: close code 4001 / `SessionEnded`) sends the console back to the login with the reason; signing out or losing the session closes it.
import { create } from 'zustand';
import { AdminWs, endReasonText, type WsState } from '../api/ws';
import { useSession } from './session';
import { toast } from './toasts';

interface LiveState {
  /** The state of the tab's socket (`idle` before the first use and after it was closed). */
  readonly ws: WsState;
  /**
   * Counts the sockets closed so far. A view that holds subscriptions lists it among the dependencies of its effect, so that it subscribes again on the
   * next socket (a session that ended and was taken over by the owner of the host, a sign-in as staff).
   */
  readonly epoch: number;
}

export const useLive = create<LiveState>(() => ({ ws: 'idle', epoch: 0 }));

let factory: () => AdminWs = () => new AdminWs();
let socket: AdminWs | null = null;
let listeners: (() => void)[] = [];

/** The tab's socket, created and connected on first call. */
export function adminSocket(): AdminWs {
  if (socket) return socket;
  const ws = factory();
  socket = ws;
  useLive.setState({ ws: ws.state });
  listeners = [
    ws.onState((s) => {
      useLive.setState({ ws: s });
    }),
    ws.onSessionEnded((reason) => {
      void sessionEnded(endReasonText(reason));
    }),
  ];
  ws.connect();
  return ws;
}

/** Closes the tab's socket for good (sign-out, a lost session). The next `adminSocket()` makes a new one. */
export function closeAdminSocket(): void {
  if (!socket) return;
  for (const off of listeners) off();
  listeners = [];
  socket.close();
  socket = null;
  useLive.setState((s) => ({ ws: 'idle', epoch: s.epoch + 1 }));
}

/**
 * The server ended the session (a ban, a change of authority or password, expiry). Ask who we are now: on the host itself that is the owner again
 * (the console goes on, with a toast naming the new identity); anywhere else the answer is 401 and the login shows with the reason. The dead socket is
 * replaced by the shell when the identity changed (`App`), and here when it did not.
 */
async function sessionEnded(text: string): Promise<void> {
  const before = useSession.getState().info?.identity ?? null;
  useSession.setState({ notice: text });
  await useSession.getState().init();
  const s = useSession.getState();
  if (s.phase !== 'ready' || (s.info?.identity ?? null) === before) closeAdminSocket();
  if (s.phase === 'ready') {
    useSession.setState({ notice: null });
    toast.info(`${text} You are ${s.info?.identity ?? 'signed in'} now.`);
  }
}

/** Tests: replaces how the socket is made (`null` restores the real one) and drops the current one. */
export function setSocketFactoryForTests(make: (() => AdminWs) | null): void {
  closeAdminSocket();
  factory = make ?? (() => new AdminWs());
}
