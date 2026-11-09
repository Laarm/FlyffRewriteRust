// The Overview tab of the character drawer (Plan/25 section 4.3): level, EXP, job, stats, HP/MP/FP, the active buffs with their time
// left, penya, the position, the guild, the party, play time, creation and last login, authority, mode flags and the mute. For an
// online character the numbers are live (the `CharacterWatch` details every 2 s); an offline one shows the last save.
import { useMemo } from 'react';
import { MODE } from '../../../gen/gm';
import { Badge, Button } from '../../components';
import { navigate } from '../../stores/route';
import { useNow } from '../../app/polling';
import type { BuffEntry, CharacterDetailView } from '../../gen/dto';
import { useCharacter } from './context';
import {
  authorityName,
  formatDateTime,
  formatNumber,
  formatPermille,
  formatPlayTime,
  formatPos,
  formatSpan,
} from './format';
import { JobIcon } from './JobIcon';
import { jobName } from './jobs';
import { usePlayerGame, useBundle } from './gamedata';
import { RowLookup } from '../../../data/lookup';
import { infoOf } from './ItemCell';
import { showOnMap, type Target } from './characterActions';
import { T } from './strings';
import { GuildChip } from '../guilds/GuildChip';

/** The names of the set bits of `m_dwMode` that the console knows. */
export function modeNames(mode: number): string[] {
  const names: [number, string][] = [
    [MODE.MATCHLESS, 'undying'],
    [MODE.MATCHLESS2, 'undying II'],
    [MODE.TRANSPARENT, 'invisible'],
    [MODE.ONEKILL, 'one-hit kills'],
    [MODE.DONMOVE, 'frozen'],
    [MODE.NO_ATTACK, 'no attack'],
    [MODE.ITEM, 'no items'],
    [MODE.COMMUNITY, 'community'],
    [MODE.TALK, 'cannot talk'],
    [MODE.SAYTALK, 'cannot say'],
    [MODE.SHOUTTALK, 'cannot shout'],
    [MODE.EXPUP_STOP, 'no EXP gain'],
  ];
  return names.filter(([bit]) => (mode & bit) === bit).map(([, n]) => n);
}

/** `kind` and `id` of a buff key `(kind << 16) | id` (0 item, 1 skill, 2 pet, 3 absolute-expiry item, 4 equipment). */
export function splitBuff(key: number): { kind: number; id: number } {
  return { kind: key >>> 16, id: key & 0xffff };
}

function Bar({
  label,
  value,
  max,
}: {
  readonly label: string;
  readonly value: number;
  readonly max: number | null;
}) {
  const pct = max && max > 0 ? Math.min(100, Math.round((value / max) * 100)) : null;
  return (
    <div className="plr-bar" data-testid={`bar-${label.toLowerCase()}`}>
      <span className="plr-bar__label">{label}</span>
      <span className="plr-bar__track" aria-hidden="true">
        <span
          className={`plr-bar__fill plr-bar__fill--${label.toLowerCase()}`}
          style={{ width: `${String(pct ?? 0)}%` }}
        />
      </span>
      <span className="plr-bar__value">
        {formatNumber(value)}
        {max !== null ? ` / ${formatNumber(max)}` : ''}
      </span>
    </div>
  );
}

function Row({ label, children }: { readonly label: string; readonly children: React.ReactNode }) {
  return (
    <>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </>
  );
}

/** The buffs with their names; the game data and the skill bundle load only when there is a buff to name. */
function BuffList({ buffs }: { readonly buffs: readonly BuffEntry[] }) {
  if (buffs.length === 0) return <p className="adm-muted">{T.overview.noBuffs}</p>;
  return <BuffItems buffs={buffs} />;
}

function BuffItems({ buffs }: { readonly buffs: readonly BuffEntry[] }) {
  const game = usePlayerGame();
  const skills = useBundle('skills');
  const lookup = useMemo(
    () => (skills.data ? new RowLookup(skills.data.skills) : null),
    [skills.data],
  );
  return (
    <ul className="plr-buffs" aria-label={T.overview.buffs}>
      {buffs.map((b) => {
        const { kind, id } = splitBuff(b.buff);
        const name =
          kind === 1 || kind === 2
            ? (lookup?.get(id)?.name ?? `skill ${String(id)}`)
            : infoOf(game.data, id).name;
        return (
          <li key={b.buff} className="plr-buff">
            <span className="plr-buff__name">{name}</span>
            {b.level > 0 && kind === 1 ? <Badge>Lv {b.level}</Badge> : null}
            <span className="adm-muted adm-small">
              {b.remaining_ms > 0 ? T.overview.buffLeft(formatSpan(b.remaining_ms / 1000)) : ''}
            </span>
          </li>
        );
      })}
    </ul>
  );
}

export function Overview() {
  const { live, id } = useCharacter();
  const d: CharacterDetailView | null = live.detail;
  const profile = live.profile;
  const now = Math.floor(useNow(1000) / 1000);
  if (!d || !profile) return null;
  const row = profile.character;
  const muted = d.mute_until > now ? d.mute_until - now : d.mute_remaining_s;
  const target: Target = {
    id,
    name: d.name,
    online: live.online,
    world: d.world,
    layer: d.layer,
    x: d.x,
    z: d.z,
  };
  return (
    <div className="plr-section" data-testid="overview">
      <div className="plr-bars">
        <Bar label="HP" value={d.hp} max={d.hp_max} />
        <Bar label="MP" value={d.mp} max={d.mp_max} />
        <Bar label="FP" value={d.fp} max={d.fp_max} />
      </div>
      <dl className="adm-dl plr-dl">
        <Row label={T.overview.level}>
          {d.level}{' '}
          <span className="adm-muted">
            ({T.overview.exp} {formatPermille(d.exp_permille)})
          </span>
        </Row>
        <Row label={T.overview.job}>
          <span className="adm-row">
            <JobIcon job={d.job} /> {jobName(d.job)}
          </span>
        </Row>
        <Row label={T.overview.sex}>{d.sex === 1 ? T.overview.female : T.overview.male}</Row>
        <Row label={T.overview.stats}>
          STR {d.str}, STA {d.sta}, DEX {d.dex}, INT {d.int}
          <span className="adm-muted">
            {' '}
            ({T.overview.freeStats} {d.stat_points}, {T.overview.freeSkills} {d.skill_points})
          </span>
        </Row>
        <Row label={T.overview.penya}>{formatNumber(d.gold)}</Row>
        <Row label={T.overview.position}>
          <span className="adm-row">
            <span>
              {T.overview.world} {d.world}, layer {d.layer}, {formatPos(d.x, d.z)}
            </span>
            {live.online ? (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  showOnMap(target);
                }}
              >
                Show on map
              </Button>
            ) : null}
          </span>
        </Row>
        <Row label={T.overview.guild}>
          {d.guild_id === 0 ? (
            T.overview.noGuild
          ) : (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                navigate({ category: 'guilds', tab: null, params: { g: String(d.guild_id) } });
              }}
            >
              <GuildChip id={d.guild_id} />
            </Button>
          )}
        </Row>
        <Row label={T.overview.party}>
          {d.party_id === 0 ? T.overview.noParty : `#${String(d.party_id)}`}
        </Row>
        <Row label={T.overview.playTime}>{formatPlayTime(d.play_time_s)}</Row>
        <Row label={T.overview.created}>{formatDateTime(row.created_at)}</Row>
        <Row label={live.online ? T.overview.lastLogin : T.overview.lastSeen}>
          {live.online ? formatDateTime(d.last_logout_at) : formatDateTime(row.last_seen)}
        </Row>
        <Row label={T.overview.authority}>
          <Badge tone={row.authority === 'F' ? 'neutral' : 'accent'}>
            {authorityName(row.authority)}
          </Badge>
        </Row>
        <Row label={T.overview.account}>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              navigate({ category: 'players', tab: 'accounts', params: { a: row.account } });
            }}
          >
            {row.account}
          </Button>
        </Row>
        <Row label={T.overview.modes}>
          {modeNames(d.mode_flags).length === 0 ? '-' : modeNames(d.mode_flags).join(', ')}
        </Row>
        <Row label={T.overview.mute}>
          {muted > 0 ? (
            <Badge tone="warn">{T.overview.mutedFor(formatSpan(muted))}</Badge>
          ) : (
            T.overview.notMuted
          )}
        </Row>
      </dl>
      <section className="plr-section">
        <h3>{T.overview.buffs}</h3>
        <BuffList buffs={d.buffs} />
      </section>
      {!live.online ? <p className="adm-muted adm-small">{T.overview.savedValues}</p> : null}
      <p className="adm-muted adm-small">{T.overview.mapPreview}</p>
    </div>
  );
}
