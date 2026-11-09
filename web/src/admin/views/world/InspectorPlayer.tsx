// The player card of the inspector (Plan/25 section 3.10): job icon, name, level, job, guild and party; the HP, MP and FP bars, the position, the state,
// the PK status, the mode flags and when the player entered the world; the account name with `view.players`, the address with `view.pii`; and the
// actions the capabilities allow. The numbers come from `GET /admin/api/objects/{obj}` (2 s), the character and the session from their own routes.
import { useStore } from 'zustand';
import { useNow } from '../../app/polling';
import { useCan } from '../../stores/session';
import type { MapSlotProps } from '../../world/api';
import { Flag } from '../../world/types';
import { JobIcon } from '../players/JobIcon';
import { formatAgo } from '../players/format';
import { jobName } from '../players/jobs';
import { playerActions, useActionEnv, type PlayerRef } from './InspectorActions';
import { useEntity, useObjectDetail, usePlayerExtra } from './InspectorData';
import { ActionButtons, Bar, CardHead, Facts } from './InspectorParts';
import { I } from './InspectorStrings';
import { openTeleportToLocation } from './InspectorLocation';
import { GuildChip } from '../guilds/GuildChip';

/** `m_dwMode` bits the card names. */
export const MODE = {
  MATCHLESS: 0x1,
  TRANSPARENT: 0x2,
  DONMOVE: 0x8,
  MATCHLESS2: 0x20,
  NO_ATTACK: 0x40,
} as const;

/** The names of the mode bits that are set, in the order of the plan (invisible, undying, frozen), then the attack ban. */
export function modeNames(bits: number): string[] {
  const out: string[] = [];
  if ((bits & MODE.TRANSPARENT) !== 0) out.push(I.modes.invisible);
  if ((bits & (MODE.MATCHLESS | MODE.MATCHLESS2)) !== 0) out.push(I.modes.undying);
  if ((bits & MODE.DONMOVE) !== 0) out.push(I.modes.frozen);
  if ((bits & MODE.NO_ATTACK) !== 0) out.push(I.modes.noAttack);
  return out;
}

/** `u32::MAX`: the mute that never ends. */
const FOREVER = 4_294_967_295;

/**
 * Seconds a character stays muted (`Infinity` for ever, 0 not muted). The character answer of an ONLINE player carries `mute_until` but a
 * `mute_remaining_s` of 0, so the end time is compared with the clock; a missing end time falls back to the remaining seconds.
 */
export function muteLeft(
  detail: { readonly mute_until: number; readonly mute_remaining_s: number } | undefined,
  nowSeconds: number,
): number {
  if (!detail) return 0;
  if (detail.mute_until >= FOREVER) return Infinity;
  if (detail.mute_until > 0) return Math.max(0, detail.mute_until - nowSeconds);
  return detail.mute_remaining_s;
}

/** The state words of a card: the server's flag names, or "alive". */
export function stateText(flagNames: readonly string[]): string {
  const names = flagNames.filter((n) => n !== 'pk' && n !== 'gm');
  return names.length === 0
    ? (I.states['alive'] ?? 'alive')
    : names.map((n) => I.states[n] ?? n).join(', ');
}

export function PlayerCard({ api, id }: MapSlotProps & { readonly id: number }) {
  const env = useActionEnv(api);
  const canPlayers = useCan('view.players');
  const entity = useEntity(api, id);
  const { detail, gone } = useObjectDetail(id);
  const characterId = detail?.character_id ?? entity?.characterId ?? 0;
  const { profile, session } = usePlayerExtra(characterId, canPlayers);
  const following = useStore(api.store, (s) => s.follow?.objId === id);
  const now = Math.floor(useNow(30_000) / 1000);

  const name = detail?.name ?? entity?.name ?? `#${String(id)}`;
  const world = detail?.world ?? env.api.store.getState().worldId;
  const layer = detail?.layer ?? env.api.store.getState().layer;
  const x = detail?.x ?? entity?.x ?? 0;
  const z = detail?.z ?? entity?.z ?? 0;
  const ref: PlayerRef = {
    id,
    kind: 1,
    name,
    world,
    layer,
    x,
    z,
    characterId,
    ...(profile ? { account: profile.character.account } : {}),
  };
  const muteSeconds = muteLeft(profile?.detail, now);
  const actions = playerActions(env, ref, {
    following,
    muted: muteSeconds > 0,
    teleportToLocation: (players) => {
      openTeleportToLocation(env, players);
    },
  });
  const level = detail?.level ?? entity?.level ?? 0;
  const job = entity?.sub ?? profile?.character.job ?? 0;
  const guildId = entity?.guildId ?? profile?.detail.guild_id ?? 0;
  const partyId = profile?.detail.party_id ?? 0;
  const pk = (detail?.flags ?? entity?.flags ?? 0) & Flag.PK;
  const modes = modeNames(detail?.mode_flags ?? 0);
  return (
    <section
      className="insp-card"
      data-testid="inspector-card"
      data-kind="player"
      aria-label={name}
    >
      <CardHead
        icon={<JobIcon job={job} size={28} />}
        title={name}
        badge={I.kinds[1]}
        subtitle={
          <>
            <span>{I.hover.lv(level)}</span>
            <span>{jobName(job)}</span>
            {guildId > 0 ? <GuildChip id={guildId} prefix={`${I.facts.guild} `} /> : null}
            {partyId > 0 ? <span>{`${I.facts.party} #${String(partyId)}`}</span> : null}
          </>
        }
      />
      {gone ? <p className="insp-note insp-note--warn">{I.gone}</p> : null}
      {detail ? (
        <div className="insp-bars">
          <Bar label={I.facts.hp} value={detail.hp} max={detail.hp_max} tone="hp" />
          <Bar label={I.facts.mp} value={detail.mp} max={detail.mp_max} tone="mp" />
          <Bar label={I.facts.fp} value={detail.fp} max={detail.fp_max} tone="fp" />
        </div>
      ) : null}
      <Facts
        rows={[
          {
            label: I.facts.world,
            value: `${env.worldName(world)}${layer === 0 ? '' : ` (${I.facts.layer} ${String(layer)})`}`,
            id: 'insp-world',
          },
          {
            label: I.facts.position,
            value: detail
              ? `${detail.x.toFixed(1)}, ${detail.y.toFixed(1)}, ${detail.z.toFixed(1)}`
              : `${x.toFixed(1)}, ${z.toFixed(1)}`,
            id: 'insp-pos',
          },
          detail && {
            label: I.facts.state,
            value: stateText(detail.flag_names),
          },
          { label: I.facts.pk, value: pk !== 0 ? I.facts.yes : I.facts.no },
          modes.length > 0 && { label: I.facts.flags, value: modes.join(', ') },
          muteSeconds > 0 && {
            label: I.facts.muted,
            value: Number.isFinite(muteSeconds)
              ? `${String(Math.ceil(muteSeconds / 60))} min`
              : I.facts.permanent,
          },
          session && {
            label: I.facts.since,
            value: formatAgo(now, session.in_world_since),
          },
          profile && {
            label: I.facts.account,
            value: profile.character.account,
            id: 'insp-account',
          },
          session?.ip !== undefined && { label: I.facts.ip, value: session.ip, id: 'insp-ip' },
        ]}
      />
      <ActionButtons actions={actions} />
    </section>
  );
}
