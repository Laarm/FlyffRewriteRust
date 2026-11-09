// Players -> Moderation -> Chat (Plan/25 section 4.9): the live chat feed of `ChatSubscribe` with filters for channel, world, character
// and text, a pause, the last 2,000 lines, and per-line actions (Kick, Mute, Ban with the line quoted in the reason).
//
// * `view.chat` (M) sees say and shout; `view.chat.private` (P) adds party, guild, whisper and messenger. The server decides; this
//   screen only offers what the session may select.
// * A subscription with private channels is audited (`gm.snoop` on subscribe and every 10 minutes); the banner says so as long as it is
//   open. The text before the word filter changed it (`original_text`) is shown to P sessions only.
// * The lines are what the players saw: the tap sits after the ingress pipeline (23 section 6.1).
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import type { ChatChannel } from '../../../gen/proto/admin';
import { Badge, Button, Checkbox, Field, Menu, NumberInput, TextInput } from '../../components';
import { adminSocket, useLive } from '../../stores/live';
import { useCan, useSession } from '../../stores/session';
import { ban, kick, mute, type Target } from './characterActions';
import {
  CHANNEL_LABEL,
  sharedBuffer,
  MAX_LINES,
  clock,
  defaultFilter,
  includesPrivate,
  isPrivate,
  matches,
  selectable,
  type ChatEntry,
  type MonitorFilter,
} from './chatLog';
import { openCharacter } from './rowMenu';
import { T } from './strings';

/** Every channel a monitor can select (the subscription names its channels from this list). */
const SELECTABLE_ALL = selectable(true);

function targetOf(e: ChatEntry): Target {
  return { id: e.fromId, name: e.from, online: true, world: e.world };
}

function LineActions({ entry }: { readonly entry: ChatEntry }) {
  const canKick = useCan('act.kick');
  const canMute = useCan('act.mute');
  const canBan = useCan('act.ban');
  const t = targetOf(entry);
  const quote = entry.text;
  const items = [
    {
      id: 'open',
      label: T.chat.openCharacter,
      onSelect: () => {
        openCharacter(entry.fromId);
      },
    },
    ...(canKick
      ? [{ id: 'kick', label: T.chat.kick, separator: true, onSelect: () => void kick(t, quote) }]
      : []),
    ...(canMute ? [{ id: 'mute', label: T.chat.mute, onSelect: () => void mute(t, quote) }] : []),
    ...(canBan ? [{ id: 'ban', label: T.chat.ban, onSelect: () => void ban(t, quote) }] : []),
  ];
  return (
    <Menu
      label={`${T.chat.lineMenu}: ${entry.from}`}
      trigger={<span aria-hidden="true">...</span>}
      items={items}
    />
  );
}

export function ChatLineView({ entry }: { readonly entry: ChatEntry }) {
  return (
    <li
      className={`plr-chat__line plr-chat__line--${isPrivate(entry.channel) ? 'private' : 'public'}`}
      data-channel={entry.channel}
      data-testid="chat-line"
    >
      <time className="plr-chat__time" dateTime={new Date(entry.ts).toISOString()}>
        {clock(entry.ts)}
      </time>
      <Badge tone={isPrivate(entry.channel) ? 'warn' : 'accent'}>
        {CHANNEL_LABEL[entry.channel] ?? '?'}
      </Badge>
      <button
        type="button"
        className="adm-linkbtn plr-chat__from"
        onClick={() => {
          openCharacter(entry.fromId);
        }}
      >
        {entry.from}
      </button>
      {entry.to !== '' ? (
        <span className="adm-muted">
          {T.chat.to} {entry.to}
        </span>
      ) : null}
      <span className="plr-chat__text">
        {entry.text}
        {entry.original !== '' ? (
          <span className="plr-chat__original" title={T.chat.original}>
            {' '}
            [{T.chat.original}: {entry.original}]
          </span>
        ) : null}
      </span>
      <span className="plr-chat__world adm-muted adm-small">w{entry.world}</span>
      <LineActions entry={entry} />
    </li>
  );
}

export function ChatMonitor() {
  const canChat = useCan('view.chat');
  const canPrivate = useCan('view.chat.private');
  const epoch = useLive((s) => s.epoch);
  const [filter, setFilter] = useState<MonitorFilter>(() => defaultFilter(canPrivate));
  const identity = useSession((s) => s.info?.identity ?? '');
  const buffer = sharedBuffer(identity);
  const snap = useSyncExternalStore(buffer.subscribe, buffer.getSnapshot);
  const list = useRef<HTMLUListElement>(null);
  const stick = useRef(true);
  const channelsKey = [...filter.channels].sort((a, b) => a - b).join(',');
  const { world, character, text } = filter;
  useEffect(() => {
    if (!canChat) return;
    const ws = adminSocket();
    const sub = ws.subscribeChat(
      {
        channels: SELECTABLE_ALL.filter((c) => channelsKey.split(',').includes(String(c))),
        world,
        characterId: character,
        text: text.trim(),
      },
      (line) => {
        buffer.push(line);
      },
      {
        onError: (e) => {
          buffer.refused(e.message);
        },
        onResync: () => {
          buffer.resynced();
        },
      },
    );
    buffer.opened();
    return () => {
      sub.close();
    };
  }, [canChat, epoch, channelsKey, world, character, text, buffer]);

  const visible = useMemo(() => snap.lines.filter((e) => matches(filter, e)), [snap.lines, filter]);

  useEffect(() => {
    const el = list.current;
    if (el && stick.current && !snap.paused) el.scrollTop = el.scrollHeight;
  }, [visible, snap.paused]);

  if (!canChat) return <p className="adm-muted">{T.chat.needsChat}</p>;

  const set = (patch: Partial<MonitorFilter>): void => {
    setFilter((f) => ({ ...f, ...patch }));
  };
  const toggle = (c: ChatChannel, on: boolean): void => {
    const next = new Set(filter.channels);
    if (on) next.add(c);
    else next.delete(c);
    // a monitor without a channel shows nothing: keep one
    if (next.size > 0) set({ channels: next });
  };
  const privateOpen = includesPrivate(filter);

  return (
    <div className="plr-section plr-chat" data-testid="chat-monitor">
      {privateOpen ? (
        <div className="adm-banner adm-banner--danger" role="status" data-testid="private-banner">
          {T.chat.privateBanner}
        </div>
      ) : null}
      {!canPrivate ? <p className="adm-muted adm-small">{T.chat.publicOnly}</p> : null}
      {snap.refusal ? (
        <p className="adm-alert adm-alert--danger">{T.chat.refused(snap.refusal)}</p>
      ) : null}
      <div className="plr-bar-top">
        <fieldset className="plr-channels">
          <legend className="adm-field__label">Channels</legend>
          {selectable(canPrivate).map((c) => (
            <Checkbox
              key={c}
              label={CHANNEL_LABEL[c] ?? String(c)}
              checked={filter.channels.has(c)}
              onChange={(e) => {
                toggle(c, e.target.checked);
              }}
            />
          ))}
        </fieldset>
        <Field label={T.chat.filterWorld}>
          <NumberInput
            min={0}
            value={filter.world === 0 ? '' : filter.world}
            onChange={(e) => {
              set({ world: Math.max(0, Math.trunc(Number(e.target.value)) || 0) });
            }}
          />
        </Field>
        <Field label={T.chat.filterCharacter}>
          <NumberInput
            min={0}
            value={filter.character === 0 ? '' : filter.character}
            onChange={(e) => {
              set({ character: Math.max(0, Math.trunc(Number(e.target.value)) || 0) });
            }}
          />
        </Field>
        <Field label={T.chat.filterText}>
          <TextInput
            type="search"
            value={filter.text}
            maxLength={64}
            onChange={(e) => {
              set({ text: e.target.value });
            }}
          />
        </Field>
        <div className="adm-row">
          {snap.paused ? (
            <Button
              variant="primary"
              onClick={() => {
                buffer.resume();
              }}
            >
              {T.chat.resume}
            </Button>
          ) : (
            <Button
              onClick={() => {
                buffer.pause();
              }}
            >
              {T.chat.pause}
            </Button>
          )}
          <Button
            variant="ghost"
            onClick={() => {
              buffer.clear();
            }}
          >
            {T.chat.clear}
          </Button>
        </div>
      </div>
      {snap.paused ? (
        <p className="adm-alert adm-alert--warn" role="status">
          {T.chat.paused(snap.waiting)}
        </p>
      ) : null}
      {snap.restarted ? (
        <p className="adm-alert adm-alert--warn" role="status">
          {T.chat.sinceReconnect}{' '}
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              buffer.dismissRestart();
            }}
          >
            {T.common.close}
          </Button>
        </p>
      ) : null}
      <p className="adm-muted adm-small" aria-live="off">
        {T.chat.buffer(Math.min(snap.lines.length, MAX_LINES))}
      </p>
      <ul
        ref={list}
        className="plr-chat__list"
        aria-label={T.chat.title}
        aria-live="off"
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
        }}
      >
        {visible.length === 0 ? (
          <li className="plr-chat__empty adm-muted">
            {!snap.connected
              ? T.chat.connecting
              : snap.lines.length === 0
                ? T.chat.empty
                : T.chat.emptyFiltered}
          </li>
        ) : (
          visible.map((e) => <ChatLineView key={e.key} entry={e} />)
        )}
      </ul>
    </div>
  );
}
