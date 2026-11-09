// The guild routes of the console (Plan/25 section 6, 11.3; WP-ADMIN-23). Every mutation takes the `Mutation<T>` envelope; the two
// star routes (create, edit) carry the `Idempotency-Key` the dialog made once, so a retry is the same operation.
import type { ConfirmResult } from '../../components/ConfirmDialog';
import { fetchPage, mutation, rest } from '../../api/rest';
import type {
  AddMemberBody,
  GuildChanged,
  GuildCreated,
  GuildDetailView,
  GuildEdit,
  GuildLogRow,
  GuildMeta,
  GuildNameCheck,
  GuildRank,
  GuildRow,
  GuildSpec,
  GuildWarRow,
  Page,
} from '../../gen/dto';

const BASE = '/admin/api/guilds';

export const fetchGuildMeta = (): Promise<GuildMeta> => rest.get<GuildMeta>(`${BASE}/meta`);

export const fetchGuild = (id: number, signal?: AbortSignal): Promise<GuildDetailView> =>
  rest.get<GuildDetailView>(`${BASE}/${String(id)}`, signal ? { signal } : {});

export const checkGuildName = (name: string, signal?: AbortSignal): Promise<GuildNameCheck> =>
  rest.get<GuildNameCheck>(`${BASE}/validate-name`, {
    query: { name },
    ...(signal ? { signal } : {}),
  });

/** The guild a character belongs to (the wizard's "already in guild X"), `null` for none. */
export async function guildOfCharacter(
  character: number,
  signal?: AbortSignal,
): Promise<GuildRow | null> {
  const page = await fetchPage<GuildRow>(BASE, { member: character, limit: 1 }, null, signal);
  return page.items[0] ?? null;
}

export const fetchGuildLog = (
  id: number,
  kind: string | null,
  before: string | null,
): Promise<Page<GuildLogRow>> =>
  fetchPage<GuildLogRow>(
    `${BASE}/${String(id)}/log`,
    { ...(kind ? { kind } : {}), limit: 100 },
    before,
  );

export const fetchGuildWars = (id: number, before: string | null): Promise<Page<GuildWarRow>> =>
  fetchPage<GuildWarRow>(`${BASE}/${String(id)}/wars`, { limit: 50 }, before);

export const createGuild = (
  spec: GuildSpec,
  given: ConfirmResult,
  key: string,
): Promise<GuildCreated> =>
  rest.post<GuildCreated>(BASE, mutation(spec, given.reason, given.confirm), {
    idempotencyKey: key,
  });

export const editGuild = (
  id: number,
  edit: GuildEdit,
  given: ConfirmResult,
  key: string,
): Promise<GuildChanged> =>
  rest.patch<GuildChanged>(`${BASE}/${String(id)}`, mutation(edit, given.reason, given.confirm), {
    idempotencyKey: key,
  });

export const addGuildMember = (
  id: number,
  character: number,
  rank: GuildRank,
  given: ConfirmResult,
): Promise<GuildChanged> => {
  const body: AddMemberBody = { character, rank };
  return rest.post<GuildChanged>(
    `${BASE}/${String(id)}/members`,
    mutation(body, given.reason, given.confirm),
  );
};

export const removeGuildMember = (
  id: number,
  character: number,
  given: ConfirmResult,
): Promise<GuildChanged> =>
  rest.delete<GuildChanged>(
    `${BASE}/${String(id)}/members/${String(character)}`,
    mutation({}, given.reason, given.confirm),
  );

export const disbandGuild = (id: number, given: ConfirmResult): Promise<GuildChanged> =>
  rest.delete<GuildChanged>(`${BASE}/${String(id)}`, mutation({}, given.reason, given.confirm));
