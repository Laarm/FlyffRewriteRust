// The character drawer (Plan/25 section 4.3): a sticky header with the sticky action bar, the offline/deleted/blocked banners, the pinned
// notes, and the tabs Overview, Items, Skills, Quests, Social, History and Notes. For an online character everything is live (the
// `CharacterWatch` of the admin socket); an offline one shows the saved state and says so.
import { useEffect, useMemo, useState } from 'react';
import { copyText } from '../../app/clipboard';
import { useNow } from '../../app/polling';
import { withParams } from '../../app/route';
import type { DrawerBodyProps } from '../../app/views';
import {
  Badge,
  Button,
  ErrorState,
  Menu,
  Skeleton,
  Tabs,
  tabButtonId,
  tabPanelId,
  type MenuItem,
  type TabItem,
} from '../../components';
import { getRoute, navigate } from '../../stores/route';
import { useCan } from '../../stores/session';
import { toast } from '../../stores/toasts';
import {
  ban,
  grantTo,
  heal,
  kick,
  liftLoginBlock,
  loginBlock,
  message,
  mute,
  rename,
  resurrect,
  returnToTown,
  showOnMap,
  teleport,
  undelete,
  unmute,
  type Target,
} from './characterActions';
import { CharacterProvider } from './context';
import { formatDateTime } from './format';
import { ItemsTab } from './ItemsTab';
import { JobIcon } from './JobIcon';
import { jobName } from './jobs';
import { useCharacterLive } from './live';
import { HistoryTab, NotesTab, QuestsTab, SkillsTab, SocialTab } from './MiscTabs';
import { NoteList } from './NotesPanel';
import { Overview } from './Overview';
import { useRoster, useRosterFeed } from './roster';
import { T } from './strings';
import { GuildChip } from '../guilds/GuildChip';

type TabId = 'overview' | 'items' | 'skills' | 'quests' | 'social' | 'history' | 'notes';
const TAB_IDS: readonly TabId[] = [
  'overview',
  'items',
  'skills',
  'quests',
  'social',
  'history',
  'notes',
];

function isTab(v: string | undefined): v is TabId {
  return v !== undefined && (TAB_IDS as readonly string[]).includes(v);
}

export function CharacterDrawer({ record, controls }: DrawerBodyProps) {
  const id = Number(record.id);
  const valid = Number.isInteger(id) && id > 0;
  const canInventory = useCan('view.inventory');
  const live = useCharacterLive(valid ? id : 0, canInventory && valid);
  const [tab, setTabState] = useState<TabId>(() => {
    const t = getRoute()?.params['ct'];
    return isTab(t) ? t : 'overview';
  });
  const setTab = (t: TabId): void => {
    setTabState(t);
    const route = getRoute();
    if (route)
      navigate(withParams(route, { ct: t === 'overview' ? undefined : t }), { replace: true });
  };
  useRosterFeed(valid);
  const rosterRow = useRoster((s) => s.rows.get(id));

  const detail = live.detail;
  const profile = live.profile;
  const name = detail?.name ?? profile?.character.name ?? `#${record.id}`;
  useEffect(() => {
    controls.setHeader(
      name,
      detail ? `Lv ${String(detail.level)} ${jobName(detail.job)}` : undefined,
    );
  }, [controls, name, detail?.level, detail?.job]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    controls.setWide(tab === 'items');
  }, [controls, tab]);

  const value = useMemo(() => ({ id, name, live }), [id, name, live]);

  if (!valid) return <p className="adm-alert adm-alert--danger">{T.drawer.notFound(record.id)}</p>;
  if (live.status === 'loading') return <Skeleton lines={8} height={18} />;
  if (live.status === 'error' || !profile || !detail) {
    return (
      <ErrorState
        error={live.error ?? new Error(T.drawer.notFound(record.id))}
        onRetry={live.reload}
      />
    );
  }
  const row = profile.character;
  const target: Target = {
    id,
    name,
    account: row.account,
    online: live.online,
    objId: rosterRow?.objId,
    world: detail.world,
    layer: detail.layer,
    x: detail.x,
    z: detail.z,
    deleted: row.status === 'deleted',
    blockedUntil: row.login_blocked_until,
  };
  const tabs: TabItem[] = [
    { id: 'overview', label: T.drawer.tabs.overview },
    ...(canInventory ? [{ id: 'items', label: T.drawer.tabs.items }] : []),
    { id: 'skills', label: T.drawer.tabs.skills },
    { id: 'quests', label: T.drawer.tabs.quests },
    { id: 'social', label: T.drawer.tabs.social },
    { id: 'history', label: T.drawer.tabs.history },
    {
      id: 'notes',
      label: T.drawer.tabs.notes,
      ...(profile.notes.length > 0 ? { badge: profile.notes.length } : {}),
    },
  ];
  const active: TabId = tabs.some((t) => t.id === tab) ? tab : 'overview';
  const pinnedNotes = profile.notes.filter((n) => n.pinned);

  return (
    <CharacterProvider value={value}>
      <div className="plr-drawer" data-testid="character-drawer" data-character={id}>
        <div className="plr-head">
          <div className="plr-head__who">
            <JobIcon job={detail.job} size={32} />
            <div className="plr-head__text">
              <strong className="plr-head__name">{name}</strong>
              <span className="adm-muted">
                Lv {detail.level} {jobName(detail.job)}
                {detail.guild_id !== 0 ? (
                  <>
                    , guild <GuildChip id={detail.guild_id} />
                  </>
                ) : null}
              </span>
            </div>
            <span className="plr-head__badges">
              {live.online ? (
                <Badge tone="ok">{T.common.online}</Badge>
              ) : (
                <Badge>{T.common.offline}</Badge>
              )}
              {row.status === 'deleted' ? <Badge tone="danger">{T.common.deleted}</Badge> : null}
              {row.status === 'sealed' ? <Badge tone="warn">{T.common.sealed}</Badge> : null}
              {row.login_blocked_until !== null ? (
                <Badge tone="warn">{T.common.blocked}</Badge>
              ) : null}
              {live.socket === 'live' && live.online ? (
                <span className="plr-live" title="Live" aria-label="Live">
                  <span className="plr-live__dot" />
                </span>
              ) : null}
            </span>
          </div>
          <ActionBar target={target} detail={detail} canMore />
        </div>

        {!live.online && row.status !== 'deleted' ? (
          <div className="adm-alert adm-alert--warn" role="status" data-testid="offline-banner">
            {profile.detail.saved_at
              ? T.drawer.offlineBanner(formatDateTime(profile.detail.saved_at))
              : T.drawer.offlineNoSave}
          </div>
        ) : null}
        {row.status === 'deleted' ? (
          <div className="adm-alert adm-alert--danger" role="status">
            {T.drawer.deletedBanner(row.purge_in_days)}
          </div>
        ) : null}
        {row.login_blocked_until !== null ? (
          <div className="adm-alert adm-alert--warn" role="status">
            {T.drawer.blockedBanner(formatDateTime(row.login_blocked_until))}
          </div>
        ) : null}
        {pinnedNotes.length > 0 ? (
          <div className="adm-alert adm-alert--warn" role="note" aria-label={T.drawer.pinnedNotes}>
            <NoteList notes={pinnedNotes} onChanged={live.reload} />
          </div>
        ) : null}
        {profile.pending_grants + profile.pending_removals > 0 ? (
          <p className="adm-muted adm-small">
            {T.drawer.pending(profile.pending_grants, profile.pending_removals)}
          </p>
        ) : null}

        <Tabs
          tabs={tabs}
          value={active}
          onChange={(t) => {
            setTab(t as TabId);
          }}
          idPrefix="plr-char"
          label="Character"
        />
        <div
          role="tabpanel"
          id={tabPanelId('plr-char')}
          aria-labelledby={tabButtonId('plr-char', active)}
          className="plr-panel"
        >
          {active === 'overview' ? <Overview /> : null}
          {active === 'items' ? <ItemsTab /> : null}
          {active === 'skills' ? <SkillsTab /> : null}
          {active === 'quests' ? <QuestsTab /> : null}
          {active === 'social' ? <SocialTab /> : null}
          {active === 'history' ? <HistoryTab /> : null}
          {active === 'notes' ? <NotesTab /> : null}
        </div>
      </div>
    </CharacterProvider>
  );
}

/** The action bar: Show on map, Teleport, Grant item, Message, Kick, Mute, Ban and the More menu (25 section 4.3). */
export function ActionBar({
  target,
  detail,
  canMore,
}: {
  readonly target: Target;
  readonly detail: { readonly mute_until: number; readonly mute_remaining_s: number } | null;
  readonly canMore: boolean;
}) {
  const can = {
    map: useCan('view.map'),
    teleport: useCan('act.teleport.others'),
    unstuck: useCan('act.unstuck'),
    grant: useCan('act.items'),
    message: useCan('act.message.player'),
    kick: useCan('act.kick'),
    mute: useCan('act.mute'),
    ban: useCan('act.ban'),
    combat: useCan('act.combat'),
    accounts: useCan('act.accounts'),
  };
  const online = target.online === true;
  const now = Math.floor(useNow(1000) / 1000);
  const muted = detail !== null && (detail.mute_until > now || detail.mute_remaining_s > 0);
  const items: MenuItem[] = [];
  if (can.unstuck || can.accounts || can.ban || can.combat) {
    if (can.unstuck) {
      items.push(
        { id: 'g1', label: T.actions.groupLocation, heading: true },
        { id: 'town', label: T.actions.returnToTown, onSelect: () => void returnToTown(target) },
      );
    }
    if (can.ban) {
      items.push(
        { id: 'g2', label: T.actions.groupModeration, heading: true, separator: true },
        target.blockedUntil !== null && target.blockedUntil !== undefined
          ? { id: 'lift', label: T.actions.liftBlock, onSelect: () => void liftLoginBlock(target) }
          : { id: 'block', label: T.actions.loginBlock, onSelect: () => void loginBlock(target) },
      );
    }
    if (can.combat) {
      items.push(
        { id: 'g3', label: T.actions.groupStatus, heading: true, separator: true },
        {
          id: 'heal',
          label: T.actions.heal,
          disabled: !online || target.objId === undefined,
          ...(online ? {} : { hint: T.actions.onlineOnly }),
          onSelect: () => void heal(target),
        },
        {
          id: 'res',
          label: T.actions.resurrect,
          disabled: !online,
          ...(online ? {} : { hint: T.actions.onlineOnly }),
          onSelect: () => void resurrect(target),
        },
      );
    }
    if (can.accounts) {
      items.push(
        { id: 'g4', label: T.actions.groupIdentity, heading: true, separator: true },
        { id: 'rename', label: T.actions.rename, onSelect: () => void rename(target) },
      );
      if (target.deleted)
        items.push({
          id: 'undelete',
          label: T.actions.undelete,
          onSelect: () => void undelete(target),
        });
    }
  }
  items.push(
    { id: 'g5', label: T.common.more, heading: true, separator: items.length > 0 },
    {
      id: 'copyid',
      label: T.actions.copyId,
      onSelect: () => {
        if (copyText(String(target.id))) toast.success(T.common.copied);
        else toast.info(T.common.copyFailed);
      },
    },
  );
  if (target.account) {
    const account = target.account;
    items.push({
      id: 'account',
      label: T.actions.openAccount,
      onSelect: () => {
        navigate({ category: 'players', tab: 'accounts', params: { a: account } });
      },
    });
  }
  return (
    <div className="plr-actions" role="toolbar" aria-label="Character actions">
      {can.map ? (
        <Button
          size="sm"
          disabled={!online}
          title={online ? undefined : T.actions.onlineOnly}
          onClick={() => {
            showOnMap(target);
          }}
        >
          {T.actions.showOnMap}
        </Button>
      ) : null}
      {can.teleport ? (
        <Button
          size="sm"
          disabled={!online}
          title={online ? undefined : T.actions.onlineOnly}
          onClick={() => void teleport(target)}
        >
          {T.actions.teleport}
        </Button>
      ) : null}
      {can.grant ? (
        <Button
          size="sm"
          variant="primary"
          onClick={() => {
            grantTo(target);
          }}
        >
          {T.actions.grant}
        </Button>
      ) : null}
      {can.message ? (
        <Button
          size="sm"
          disabled={!online}
          title={online ? undefined : T.actions.onlineOnly}
          onClick={() => void message([target])}
        >
          {T.actions.message}
        </Button>
      ) : null}
      {can.kick ? (
        <Button
          size="sm"
          variant="danger"
          disabled={!online}
          title={online ? undefined : T.actions.onlineOnly}
          onClick={() => void kick(target)}
        >
          {T.actions.kick}
        </Button>
      ) : null}
      {can.mute ? (
        muted ? (
          <Button size="sm" disabled={!online} onClick={() => void unmute(target)}>
            {T.actions.unmute}
          </Button>
        ) : (
          <Button
            size="sm"
            disabled={!online}
            title={online ? undefined : T.actions.onlineOnly}
            onClick={() => void mute(target)}
          >
            {T.actions.mute}
          </Button>
        )
      ) : null}
      {can.ban ? (
        <Button size="sm" variant="danger" onClick={() => void ban(target)}>
          {T.actions.ban}
        </Button>
      ) : null}
      {canMore ? (
        <Menu label={T.actions.more} trigger={<span>{T.actions.more}</span>} items={items} />
      ) : null}
    </div>
  );
}
