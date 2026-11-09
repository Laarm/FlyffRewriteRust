// The top bar (Plan/25 section 2.1): brand, the command palette trigger, the live status pills, the acting GM character and the
// identity menu. It is always visible.
import type { ReactNode } from 'react';
import { Countdown } from '../components/Countdown';
import { Icon } from '../components/Icon';
import { Menu, type MenuItem } from '../components/Menu';
import { Select } from '../components/fields';
import { actingFor, useActing } from '../stores/acting';
import { navigate } from '../stores/route';
import { useSession } from '../stores/session';
import { useLive } from '../stores/live';
import {
  formatMs,
  LIVE_STALE_MS,
  useStatus,
  type LiveStatus,
  type ServerStatus,
} from '../stores/status';
import { useUi } from '../stores/ui';
import { useNow, usePolling } from './polling';
import { useStatusFeed } from './useStatusFeed';
import { ProfileDialog } from './Profile';
import { openDialog } from '../stores/dialogs';
import { PasswordDialog } from './PasswordChange';
import { TotpDialog } from './TotpDialog';
import { LoginForm } from './Login';
import { Dialog } from '../components/Dialog';
import { S } from './strings';

function Pill({
  tone = 'neutral',
  title,
  children,
}: {
  readonly tone?: 'neutral' | 'ok' | 'warn' | 'danger';
  readonly title?: string;
  readonly children: ReactNode;
}) {
  return (
    <span className={`adm-pill adm-pill--${tone}`} {...(title === undefined ? {} : { title })}>
      {children}
    </span>
  );
}

/** The numbers the pills show: the live tick while it is fresh, else the polled status. */
export function pillData(
  status: ServerStatus | null,
  live: LiveStatus | null,
  now: number,
): {
  inWorld: number;
  lobby: number;
  tickP50Us: number;
  tickP99Us: number;
  dbDegraded: boolean;
  dbQueue: number;
  shutdownInS: number | null;
  running: boolean;
  state: string;
  alerts: number | null;
  maintenance: boolean | null;
} | null {
  const fresh = live !== null && now - live.at < LIVE_STALE_MS;
  if (fresh) {
    return {
      inWorld: live.in_world,
      lobby: live.lobby,
      tickP50Us: live.tick_p50_us,
      tickP99Us: live.tick_p99_us,
      dbDegraded: live.db_degraded,
      dbQueue: live.db_queue,
      shutdownInS: live.shutdown_in_s,
      running: status?.running ?? true,
      state: status?.state ?? 'running',
      alerts: live.alerts,
      maintenance: live.maintenance,
    };
  }
  if (!status) return null;
  return {
    inWorld: status.in_world,
    lobby: status.lobby,
    tickP50Us: status.tick_p50_us,
    tickP99Us: status.tick_p99_us,
    dbDegraded: status.db_degraded,
    dbQueue: status.db_queue,
    shutdownInS: status.shutdown_in_s,
    running: status.running,
    state: status.state,
    alerts: null,
    maintenance: typeof status.maintenance === 'boolean' ? status.maintenance : null,
  };
}

/** Online count, tick time, database, alerts, maintenance and a shutdown countdown: the 1 Hz socket feed, with the polled status as fallback. */
export function StatusPills() {
  const status = useStatus((s) => s.status);
  const live = useStatus((s) => s.live);
  const error = useStatus((s) => s.error);
  const updatedAt = useStatus((s) => s.updatedAt);
  const polledAlerts = useStatus((s) => s.alerts);
  const alertsAvailable = useStatus((s) => s.alertsAvailable);
  const refreshStatus = useStatus((s) => s.refreshStatus);
  const refreshAlerts = useStatus((s) => s.refreshAlerts);
  const socket = useLive((s) => s.ws);
  useStatusFeed();
  const now = useNow(1000);
  const feed = live !== null && now - live.at < LIVE_STALE_MS;
  // the socket carries the numbers; the polled status stays for the build, the uptime and as the fallback
  usePolling(refreshStatus, feed ? 30_000 : 5000);
  usePolling(refreshAlerts, 30_000, alertsAvailable && !feed);

  const data = pillData(status, live, now);
  const stale = !feed && error !== null && now - updatedAt > 15_000;
  const alerts = feed ? (data?.alerts ?? null) : alertsAvailable ? polledAlerts : null;
  if (!data) {
    return (
      <div className="adm-pills" aria-label="Server status">
        {stale ? <Pill tone="danger">{S.top.unreachable}</Pill> : null}
      </div>
    );
  }
  const p99Slow = data.tickP99Us > 25_000;
  // the countdown counts from when the number was current: the tick's arrival, or the last poll
  const shutdownBase = (feed ? live.at : updatedAt) / 1000;
  return (
    <div
      className="adm-pills"
      aria-label="Server status"
      data-feed={feed ? 'live' : socket === 'open' ? 'waiting' : 'polled'}
    >
      {stale ? <Pill tone="danger">{S.top.unreachable}</Pill> : null}
      <Pill
        tone={data.running ? 'ok' : 'warn'}
        title={`${data.state}, ${String(data.lobby)} in the lobby`}
      >
        <span className="adm-dot" aria-hidden="true" />
        {S.top.online(data.inWorld)}
      </Pill>
      <Pill
        tone={p99Slow ? 'warn' : 'neutral'}
        title={`${S.top.tickTitle}; p99 ${formatMs(data.tickP99Us)}`}
      >
        {S.top.tick(formatMs(data.tickP50Us))}
      </Pill>
      <Pill tone={data.dbDegraded ? 'danger' : 'neutral'} title={S.top.dbQueue(data.dbQueue)}>
        {data.dbDegraded ? S.top.dbDegraded : S.top.dbOk}
      </Pill>
      {alerts !== null ? (
        <Pill tone={alerts > 0 ? 'warn' : 'neutral'}>
          <Icon name="bell" size={12} />
          {S.top.alerts(alerts)}
        </Pill>
      ) : null}
      {data.maintenance !== null ? (
        <Pill tone={data.maintenance ? 'warn' : 'neutral'}>
          {S.top.maintenance(data.maintenance)}
        </Pill>
      ) : null}
      {data.shutdownInS !== null ? (
        <Pill tone="danger">
          {S.top.shutdown} <Countdown until={shutdownBase + data.shutdownInS} />
        </Pill>
      ) : null}
    </div>
  );
}

/** "You: Leadhero" (Plan/25 section 1.5): a staff member's own in-world character, or the owner's pick among the GM characters online. */
export function ActingSelector() {
  const info = useSession((s) => s.info);
  const chosen = useActing((s) => s.chosen);
  const candidates = useActing((s) => s.candidates);
  const choose = useActing((s) => s.choose);
  const loadCandidates = useActing((s) => s.loadCandidates);
  const owner = info !== null && info.kind !== 'staff';
  usePolling(loadCandidates, 15_000, owner);
  const acting = actingFor(info, chosen, candidates);

  if (!info) return null;
  if (!owner) {
    return (
      <div className="adm-acting" title={acting ? S.top.actingLabel : S.top.actingNoneHint}>
        <span className="adm-muted">{S.top.acting}:</span>{' '}
        <strong>{acting ? acting.name : S.top.actingNone}</strong>
      </div>
    );
  }
  return (
    <label
      className="adm-acting"
      title={candidates.length === 0 ? S.top.noCandidates : S.top.actingPick}
    >
      <span className="adm-muted">{S.top.acting}:</span>
      <Select
        aria-label={S.top.actingLabel}
        className="adm-input--sm"
        value={acting ? String(acting.id) : ''}
        onChange={(e) => {
          const c = candidates.find((x) => String(x.id) === e.target.value);
          choose(c ? { id: c.id, name: c.name } : null);
        }}
      >
        <option value="">{S.top.actingNone}</option>
        {candidates.map((c) => (
          <option key={c.id} value={String(c.id)}>
            {c.name} ({c.authority})
          </option>
        ))}
      </Select>
    </label>
  );
}

function identityTitle(kind: string, account: string | null): string {
  if (kind === 'staff') return account ?? S.identity.staff;
  return kind === 'owner_token' ? S.identity.ownerToken : S.identity.ownerLocal;
}

export function IdentityMenu() {
  const info = useSession((s) => s.info);
  const logout = useSession((s) => s.logout);
  const { theme, density, landing, setTheme, setDensity, setLanding } = useUi();
  if (!info) return null;
  const hasCookie = info.idle_expires_at !== null;
  const items: MenuItem[] = [
    {
      id: 'who',
      heading: true,
      label: (
        <>
          <strong>{info.identity}</strong> ({info.authority})
        </>
      ),
    },
    {
      id: 'profile',
      label: S.identity.profile,
      icon: 'user',
      onSelect: () => {
        openDialog((close) => <ProfileDialog info={info} onClose={close} />);
      },
    },
    ...(info.kind === 'staff'
      ? [
          {
            id: 'password',
            label: S.identity.changePassword,
            icon: 'key' as const,
            onSelect: () => {
              openDialog((close) => <PasswordDialog onClose={close} />);
            },
          },
        ]
      : []),
    ...(info.kind === 'staff'
      ? [
          {
            id: 'totp',
            label: S.identity.totp,
            icon: 'lock' as const,
            checked: info.totp_enabled,
            onSelect: () => {
              openDialog((close) => <TotpDialog onClose={close} />);
            },
          },
        ]
      : []),
    ...(info.kind === 'owner_local'
      ? [
          {
            id: 'staff',
            label: S.login.asStaff,
            icon: 'users' as const,
            onSelect: () => {
              openDialog((close) => (
                <Dialog title={S.login.asStaff} onClose={close} size="sm">
                  <p className="adm-muted">{S.login.asStaffHint}</p>
                  <LoginForm onDone={close} />
                </Dialog>
              ));
            },
          },
        ]
      : []),
    { id: 'h-theme', heading: true, label: S.identity.theme, separator: true },
    {
      id: 'theme-system',
      label: S.identity.themeSystem,
      icon: 'monitor',
      checked: theme === 'system',
      onSelect: () => {
        setTheme('system');
      },
    },
    {
      id: 'theme-light',
      label: S.identity.themeLight,
      icon: 'sun',
      checked: theme === 'light',
      onSelect: () => {
        setTheme('light');
      },
    },
    {
      id: 'theme-dark',
      label: S.identity.themeDark,
      icon: 'moon',
      checked: theme === 'dark',
      onSelect: () => {
        setTheme('dark');
      },
    },
    {
      id: 'density',
      label: S.identity.densityCompact,
      icon: density === 'compact' ? 'check' : 'sliders',
      separator: true,
      onSelect: () => {
        setDensity(density === 'compact' ? 'comfortable' : 'compact');
      },
    },
    {
      id: 'landing',
      label: `${S.identity.landing}: ${landing === 'world' ? S.identity.landingWorld : S.identity.landingDashboard}`,
      icon: 'map',
      onSelect: () => {
        setLanding(landing === 'world' ? 'dashboard' : 'world');
      },
    },
    ...(hasCookie
      ? [
          {
            id: 'signout',
            label: S.identity.signOut,
            icon: 'logout' as const,
            separator: true,
            onSelect: () => {
              void logout();
            },
          },
        ]
      : []),
  ];
  return (
    <Menu
      label={S.identity.menu}
      items={items}
      trigger={
        <>
          <Icon name="user" />
          <span className="adm-identity__name">{identityTitle(info.kind, info.account)}</span>
          <span className="adm-badge adm-badge--accent">{info.authority}</span>
          <Icon name="chevronDown" size={12} />
        </>
      }
    />
  );
}

export function TopBar({ onOpenPalette }: { readonly onOpenPalette: () => void }) {
  return (
    <header className="adm-topbar">
      <a
        className="adm-brand"
        href="#/"
        onClick={(e) => {
          e.preventDefault();
          navigate(null);
        }}
      >
        <span className="adm-brand__mark" aria-hidden="true" />
        <span className="adm-brand__text">{S.appName}</span>
      </a>
      <button
        type="button"
        className="adm-search"
        onClick={onOpenPalette}
        aria-label={S.top.searchLabel}
        aria-keyshortcuts="Control+K Meta+K"
      >
        <Icon name="search" />
        <span className="adm-search__text">{S.top.search}</span>
        <kbd>Ctrl K</kbd>
      </button>
      <StatusPills />
      <span className="adm-grow" />
      <ActingSelector />
      <IdentityMenu />
    </header>
  );
}
