// The public face of the guild feature (`guildOf( ctx )`), in a module of its own so that the features that only READ the guild (the tooltip: the name on a guild cloak; the siege windows;
// the renderer's guild marks) can import it without importing the module with its windows, reducers and senders.

import type { ClientContext } from '../../app/context';
import type { GuildBrief } from '../../gen/proto/flyff/v1/social_types_pb';
import type { GuildActions, GuildMemberRow, GuildView } from '../../ui/windows/guild/ports';
import type { ViewSource } from '../common/view';
import type { GuildSenders } from './senders';
import type { GuildStore } from './store';

/** The seams the siege windows install (`setEnv`). */
export interface GuildEnvSeams {
  /** `GUILDCOMBAT_NOT_INVITATION_GUILD` / `1TO1_NOTINVITEGUILD`: the text id that forbids inviting now, `null` for none. */
  siegeBusy: () => number | null;
  /** The own guild has an application in the siege (`g_GuildCombatMng.m_bRequest`): no disbanding, no leaving. */
  siegeApplied: () => boolean;
}

/** The public face of the guild feature of a context. */
export interface GuildFeature {
  readonly store: GuildStore;
  readonly senders: GuildSenders;
  /** The view and the actions of the guild window. */
  readonly view: ViewSource<GuildView>;
  readonly actions: GuildActions;
  ownGuildId(): number;
  /** The name of the own guild (`''` while unnamed or without a guild). */
  ownGuildName(): string;
  /** `CGuild::IsMaster( idPlayer )` of the own guild. */
  isMaster(charId: number): boolean;
  /** The member level of a character of the own guild, `-1` for none. */
  rankOf(charId: number): number;
  /** The members of the own guild in character id order. */
  members(): readonly GuildMemberRow[];
  memberIds(): readonly number[];
  /** The name of any known guild, `null` when unknown. */
  nameOf(guildId: number): string | null;
  briefOf(guildId: number): GuildBrief | null;
  /** The own member level has the power bit (the master always does, PL-092). */
  hasPower(bit: number): boolean;
  /** `g_pPlayer->m_idWar`. */
  selfWar(): number;
  /** A member is online: the messenger overlay (`GuildGameJoin`, `GuildGameLogin`) or the cache. */
  isOnline(charId: number): boolean;
  /** Installs the seams of the siege windows. */
  setEnv(env: Partial<GuildEnvSeams>): void;
  /** Opens the guild window on a tab (hotkey G). */
  openGuild(tab?: number): void;
}

const features = new WeakMap<ClientContext, GuildFeature>();

/** The guild feature of a context; `undefined` before the module registered. */
export function guildOf(ctx: ClientContext): GuildFeature | undefined {
  return features.get(ctx);
}

/** Called by the module once it exists. */
export function setGuildOf(ctx: ClientContext, feature: GuildFeature): void {
  features.set(ctx, feature);
}
