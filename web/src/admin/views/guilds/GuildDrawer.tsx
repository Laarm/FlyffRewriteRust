// The guild drawer (Plan/25 section 6.1, 6.3): a header with the large logo, the name, the level, the notice and the online count; the action
// bar (act.guild); the tabs Roster, Bank, Bank log, Wars, House (Phase C) and History. While it is visible it polls `GET /guilds/{id}`
// every 3 s, so the roster, the pools and the bank (changed cells flash) follow the game.
import { useCallback, useEffect, useRef, useState } from 'react';
import { withParams } from '../../app/route';
import type { DrawerBodyProps } from '../../app/views';
import { GuildLogo } from '../../components/GuildLogo';
import {
  Badge,
  Button,
  ErrorState,
  Skeleton,
  Tabs,
  tabButtonId,
  tabPanelId,
  type TabItem,
} from '../../components';
import type { GuildDetailView } from '../../gen/dto';
import { getRoute, navigate } from '../../stores/route';
import { useCan } from '../../stores/session';
import { changedSlots } from '../players/containers';
import { formatDateTime, formatNumber } from '../players/format';
import {
  addMember,
  changeGold,
  changeLogo,
  changeMaster,
  disband,
  rename,
  setLevel,
  setNotice,
  setPxp,
  type GuildRef,
} from './actions';
import { fetchGuild } from './api';
import {
  BankPanel,
  HistoryPanel,
  HousePanel,
  LogPanel,
  RosterPanel,
  WarsPanel,
  bankModel,
} from './panels';
import { G } from './strings';
import { useGuildMeta } from './useGuildMeta';

type TabId = 'roster' | 'bank' | 'log' | 'wars' | 'house' | 'history';
const TAB_IDS: readonly TabId[] = ['roster', 'bank', 'log', 'wars', 'house', 'history'];

const isTab = (v: string | undefined): v is TabId =>
  v !== undefined && (TAB_IDS as readonly string[]).includes(v);

export const POLL_MS = 3000;

interface Live {
  readonly detail: GuildDetailView | null;
  readonly error: unknown;
  /** Display slot -> change counter of the bank cells that changed between two polls. */
  readonly flash: ReadonlyMap<number, number>;
  readonly reload: () => void;
}

/** The guild as live state: loaded once, then polled while the page is visible. */
export function useGuildLive(id: number, valid: boolean): Live {
  const [detail, setDetail] = useState<GuildDetailView | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [flash, setFlash] = useState<ReadonlyMap<number, number>>(new Map());
  const last = useRef<GuildDetailView | null>(null);
  const [epoch, setEpoch] = useState(0);
  const reload = useCallback(() => {
    setEpoch((n) => n + 1);
  }, []);
  useEffect(() => {
    if (!valid) return;
    let live = true;
    let controller = new AbortController();
    const load = (): void => {
      controller.abort();
      controller = new AbortController();
      fetchGuild(id, controller.signal).then(
        (d) => {
          if (!live) return;
          const prev = last.current ? bankModel(last.current) : undefined;
          const next = bankModel(d);
          if (prev && next) {
            const changed = changedSlots(prev, next);
            if (changed.length > 0) {
              setFlash((f) => {
                const m = new Map(f);
                for (const s of changed) m.set(s, (m.get(s) ?? 0) + 1);
                return m;
              });
            }
          }
          last.current = d;
          setDetail(d);
          setError(null);
        },
        (e: unknown) => {
          if (live && !(e instanceof DOMException)) setError(e);
        },
      );
    };
    load();
    const timer = setInterval(() => {
      if (document.visibilityState !== 'hidden') load();
    }, POLL_MS);
    return () => {
      live = false;
      controller.abort();
      clearInterval(timer);
    };
  }, [id, valid, epoch]);
  return { detail, error, flash, reload };
}

export function GuildDrawer({ record, controls }: DrawerBodyProps) {
  const id = Number(record.id);
  const valid = Number.isInteger(id) && id > 0;
  const canGuild = useCan('act.guild');
  const canItems = useCan('act.items');
  const canInventory = useCan('view.inventory');
  const { meta } = useGuildMeta();
  const { detail, error, flash, reload } = useGuildLive(id, valid);
  const [tab, setTabState] = useState<TabId>(() => {
    const t = getRoute()?.params['gt'];
    return isTab(t) ? t : 'roster';
  });
  const setTab = (t: TabId): void => {
    setTabState(t);
    const route = getRoute();
    if (route)
      navigate(withParams(route, { gt: t === 'roster' ? undefined : t }), { replace: true });
  };
  const name = detail
    ? detail.guild.named
      ? detail.guild.name
      : G.list.unnamed(detail.guild.id)
    : `#${record.id}`;
  useEffect(() => {
    controls.setHeader(name, detail ? `Lv ${String(detail.guild.level)}` : undefined);
  }, [controls, name, detail?.guild.level]); // eslint-disable-line react-hooks/exhaustive-deps
  // a disbanded guild (404 on a poll) closes nothing by itself: the message says so
  if (!valid) return <p className="adm-alert adm-alert--danger">{G.drawer.notFound(record.id)}</p>;
  if (error !== null && detail === null) return <ErrorState error={error} onRetry={reload} />;
  if (!detail) return <Skeleton lines={8} height={18} />;
  const g = detail.guild;
  const ref: GuildRef = { id: g.id, name: g.named ? g.name : '' };
  const act = (run: () => Promise<boolean>) => (): void => {
    void run().then((ok) => {
      if (ok) reload();
    });
  };
  const tabs: TabItem[] = [
    { id: 'roster', label: G.drawer.tabs.roster, badge: g.members },
    { id: 'bank', label: G.drawer.tabs.bank },
    { id: 'log', label: G.drawer.tabs.log },
    { id: 'wars', label: G.drawer.tabs.wars },
    { id: 'house', label: G.drawer.tabs.house },
    { id: 'history', label: G.drawer.tabs.history },
  ];
  const active: TabId = tabs.some((t) => t.id === tab) ? tab : 'roster';
  const maxLevel = meta?.levels.length ?? 50;
  const goldMax = meta?.gold_max ?? 0xffff_ffff;
  const noticeMax = meta?.notice_max_bytes ?? 127;
  return (
    <div className="plr-drawer gld-drawer" data-testid="guild-drawer" data-guild={id}>
      <div className="plr-head gld-head">
        <div className="plr-head__who">
          <GuildLogo logo={g.logo} scale={3} />
          <div className="plr-head__text">
            <strong className="plr-head__name" data-testid="guild-name-head">
              {name}
            </strong>
            <span className="adm-muted">
              {G.drawer.level} {g.level} - {G.drawer.online(g.online, g.members)} ({g.max_members})
            </span>
          </div>
          <span className="plr-head__badges">
            {g.war ? (
              <Badge tone="warn">{`${G.drawer.war}: ${g.war.enemy_name || `#${String(g.war.enemy_id)}`}`}</Badge>
            ) : null}
          </span>
        </div>
        <p className="gld-notice" data-testid="guild-notice">
          {detail.notice === '' ? (
            <span className="adm-muted">{G.drawer.noNotice}</span>
          ) : (
            detail.notice
          )}
        </p>
        <p className="adm-muted adm-small">
          {G.drawer.gold} {formatNumber(g.gold)} - {G.drawer.pxp} {formatNumber(g.pxp)} -{' '}
          {G.drawer.created} {formatDateTime(g.created_at)}
        </p>
        {canGuild ? (
          <div className="adm-row gld-actions" role="toolbar" aria-label="Guild actions">
            <Button size="sm" onClick={act(() => rename(ref))}>
              {G.actions.rename}
            </Button>
            <Button size="sm" onClick={act(() => changeLogo(ref, g.logo))}>
              {G.actions.logo}
            </Button>
            <Button size="sm" onClick={act(() => changeMaster(ref, detail.members))}>
              {G.actions.master}
            </Button>
            <Button size="sm" onClick={act(() => setLevel(ref, g.level, maxLevel))}>
              {G.actions.level}
            </Button>
            <Button size="sm" onClick={act(() => setPxp(ref, g.pxp))}>
              {G.actions.pxp}
            </Button>
            <Button
              size="sm"
              disabled={!canItems}
              onClick={act(() => changeGold(ref, g.gold, goldMax))}
            >
              {G.actions.gold}
            </Button>
            <Button size="sm" onClick={act(() => setNotice(ref, detail.notice, noticeMax))}>
              {G.actions.notice}
            </Button>
            <Button size="sm" onClick={act(() => addMember(ref, detail))}>
              {G.actions.addMember}
            </Button>
            <Button size="sm" variant="danger" onClick={act(() => disband(ref, g.members))}>
              {G.actions.disband}
            </Button>
          </div>
        ) : null}
      </div>
      <Tabs
        tabs={tabs}
        value={active}
        onChange={(t) => {
          setTab(t as TabId);
        }}
        idPrefix="gld"
        label="Guild"
      />
      <div role="tabpanel" id={tabPanelId('gld')} aria-labelledby={tabButtonId('gld', active)}>
        {active === 'roster' ? <RosterPanel detail={detail} guild={ref} canAct={canGuild} /> : null}
        {active === 'bank' ? (
          <BankPanel detail={detail} guild={ref} flash={flash} canInventory={canInventory} />
        ) : null}
        {active === 'log' ? <LogPanel id={id} canInventory={canInventory} /> : null}
        {active === 'wars' ? <WarsPanel detail={detail} /> : null}
        {active === 'house' ? <HousePanel /> : null}
        {active === 'history' ? <HistoryPanel id={id} /> : null}
      </div>
    </div>
  );
}
