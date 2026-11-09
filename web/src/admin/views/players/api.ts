// The players views' calls of the admin API (Plan/25 section 4, 8, 11.3): one typed function per route. Mutations take what
// `runConfirmed` collected (a reason, a typed confirmation, a duration) and send the `Mutation<T>` envelope; the routes marked with a
// star in 11.3 are sent with an `Idempotency-Key`.
import { mutation, rest } from '../../api/rest';
import type { ConfirmResult } from '../../components/ConfirmDialog';
import type {
  AccountView,
  ActionDone,
  AltClustersView,
  AltsView,
  BanView,
  CharacterListItem,
  CharacterProfile,
  ContainerLoc,
  ContainersView,
  GoldBody,
  IpBlocksView,
  MailboxView,
  MuteView,
  NoteBody,
  NoteView,
  Page,
  PetBody,
  PositionTo,
  RemoveItemBody,
  RenameBody,
  StaffView,
  UndeleteBody,
  AdminSessionRow,
} from '../../gen/dto';

const enc = encodeURIComponent;
const characterPath = (id: number): string => `/admin/api/characters/${String(id)}`;
const accountPath = (name: string): string => `/admin/api/accounts/${enc(name)}`;

/** The `{reason, confirm}` part of a confirmed mutation. */
const around = (given: ConfirmResult): [ConfirmResult['reason'], ConfirmResult['confirm']] => [
  given.reason,
  given.confirm,
];

function envelope<T>(body: T, given: ConfirmResult) {
  const [reason, confirm] = around(given);
  return mutation(body, reason, confirm);
}

// ---- reads --------------------------------------------------------------------------------------------------------------------

export const fetchProfile = (id: number): Promise<CharacterProfile> =>
  rest.get<CharacterProfile>(characterPath(id));

export const fetchContainers = (id: number): Promise<ContainersView> =>
  rest.get<ContainersView>(`${characterPath(id)}/items`);

export const fetchMail = (id: number): Promise<MailboxView> =>
  rest.get<MailboxView>(`${characterPath(id)}/mail`);

export const fetchAccount = (name: string): Promise<AccountView> =>
  rest.get<AccountView>(accountPath(name));

export const fetchAlts = (name: string, days?: number): Promise<AltsView> =>
  rest.get<AltsView>(`${accountPath(name)}/alts`, days === undefined ? {} : { query: { days } });

export const fetchAltClusters = (days?: number): Promise<AltClustersView> =>
  rest.get<AltClustersView>(
    '/admin/api/moderation/alts',
    days === undefined ? {} : { query: { days } },
  );

export const fetchNotes = (kind: 'account' | 'character', id: number): Promise<Page<NoteView>> =>
  rest.get<Page<NoteView>>('/admin/api/notes', { query: { subject_kind: kind, subject_id: id } });

export const fetchStaff = (): Promise<StaffView> => rest.get<StaffView>('/admin/api/staff');

export const fetchAdminSessions = (): Promise<Page<AdminSessionRow>> =>
  rest.get<Page<AdminSessionRow>>('/admin/api/admin-sessions');

export const fetchIpBlocks = (): Promise<IpBlocksView> =>
  rest.get<IpBlocksView>('/admin/api/ip-blocks');

export type { BanView, CharacterListItem, MuteView };

// ---- character actions -------------------------------------------------------------------------------------------------------

export const removeItem = (
  id: number,
  body: RemoveItemBody,
  given: ConfirmResult,
): Promise<ActionDone> =>
  rest.post<ActionDone>(`${characterPath(id)}/items/remove`, envelope(body, given), {
    idempotent: true,
  });

export const adjustGold = (id: number, body: GoldBody, given: ConfirmResult): Promise<ActionDone> =>
  rest.post<ActionDone>(`${characterPath(id)}/gold`, envelope(body, given), { idempotent: true });

export const petAction = (id: number, body: PetBody, given: ConfirmResult): Promise<ActionDone> =>
  rest.post<ActionDone>(`${characterPath(id)}/pet`, envelope(body, given), { idempotent: true });

export const moveCharacter = (
  id: number,
  to: PositionTo,
  given: ConfirmResult,
): Promise<ActionDone> =>
  rest.post<ActionDone>(`${characterPath(id)}/position`, envelope({ to }, given), {
    idempotent: true,
  });

export const renameCharacter = (
  id: number,
  body: RenameBody,
  given: ConfirmResult,
): Promise<ActionDone> =>
  rest.post<ActionDone>(`${characterPath(id)}/rename`, envelope(body, given), { idempotent: true });

export const undeleteCharacter = (
  id: number,
  body: UndeleteBody,
  given: ConfirmResult,
): Promise<ActionDone> =>
  rest.post<ActionDone>(`${characterPath(id)}/undelete`, envelope(body, given), {
    idempotent: true,
  });

/** `until` is unix seconds, `null` blocks for ever; `lift` removes the block. */
export const setLoginBlock = (
  id: number,
  body: { until: number | null; lift: boolean },
  given: ConfirmResult,
): Promise<ActionDone> =>
  rest.put<ActionDone>(
    `${characterPath(id)}/login-block`,
    envelope(body.until === null ? { lift: body.lift } : body, given),
  );

export const banCharacter = (
  id: number,
  until: number | null,
  given: ConfirmResult,
): Promise<ActionDone> =>
  rest.post<ActionDone>(
    `${characterPath(id)}/ban`,
    envelope(until === null ? {} : { until }, given),
  );

// ---- the shared action route (`POST /admin/api/actions/{action}`) -------------------------------------------------------------

function action<T>(name: string, body: unknown, given: ConfirmResult): Promise<T> {
  return rest.post<T>(`/admin/api/actions/${name}`, envelope(body, given), { idempotent: true });
}

export const muteCharacter = (
  character: number,
  seconds: number | null,
  given: ConfirmResult,
): Promise<unknown> =>
  action('mute', seconds === null ? { character } : { character, seconds }, given);

export const unmuteCharacter = (character: number, given: ConfirmResult): Promise<unknown> =>
  action('unmute', { character }, given);

export const messageCharacters = (
  characters: readonly number[],
  text: string,
  style: 'notice' | 'whisper',
  given: ConfirmResult,
): Promise<unknown> => action('message', { characters, text, style }, given);

export const resurrectCharacter = (character: number, given: ConfirmResult): Promise<unknown> =>
  action('resurrect', { character }, given);

export const healObjects = (objs: readonly number[], given: ConfirmResult): Promise<unknown> =>
  action('heal', { objs }, given);

export const teleportCharacters = (
  characters: readonly number[],
  where: { world: number; layer: number; pos: [number, number, number] },
  given: ConfirmResult,
): Promise<unknown> => action('teleport', { characters, ...where, label: 'point' }, given);

/** The older kick route: by character name or account name, the reason as one string. */
export const kickCharacter = (
  name: string,
  reason: string | undefined,
): Promise<{ affected: number }> =>
  rest.post<{ affected: number }>('/admin/api/kick', {
    character: name,
    ...(reason ? { reason } : {}),
  });

// ---- accounts (the routes of 12 section 5.5) ----------------------------------------------------------------------------------

export const banAccount = (
  name: string,
  until: number | null,
  reason: string | undefined,
): Promise<unknown> =>
  rest.post<unknown>(`${accountPath(name)}/ban`, {
    ...(until === null ? {} : { until }),
    ...(reason ? { reason } : {}),
  });

export const unbanAccount = (name: string): Promise<unknown> =>
  rest.delete<unknown>(`${accountPath(name)}/ban`);

export const resetPassword = (name: string, pwhash: string): Promise<unknown> =>
  rest.put<unknown>(`${accountPath(name)}/password`, { pwhash });

export const setAuthority = (
  name: string,
  level: string,
): Promise<{ overridden_by_gm_config: boolean }> =>
  rest.put<{ overridden_by_gm_config: boolean }>(`${accountPath(name)}/authority`, { level });

// ---- notes, IP blocks, staff sessions -----------------------------------------------------------------------------------------

export const addNote = (body: NoteBody, given: ConfirmResult): Promise<NoteView> =>
  rest.post<NoteView>('/admin/api/notes', envelope(body, given));

export const deleteNote = (id: number): Promise<unknown> =>
  rest.delete<unknown>(`/admin/api/notes/${String(id)}`);

export const addIpBlock = (
  cidr: string,
  until: number | null,
  given: ConfirmResult,
): Promise<unknown> =>
  rest.post<unknown>(
    '/admin/api/ip-blocks',
    envelope(until === null ? { cidr } : { cidr, until }, given),
  );

export const removeIpBlock = (id: number): Promise<unknown> =>
  rest.delete<unknown>(`/admin/api/ip-blocks/${String(id)}`);

export const endAdminSession = (id: string): Promise<unknown> =>
  rest.delete<unknown>(`/admin/api/admin-sessions/${enc(id)}`);

export type { ContainerLoc };
