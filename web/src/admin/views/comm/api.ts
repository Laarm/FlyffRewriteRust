// The REST calls of the Communication tabs (Plan/25 section 8, 11.3): notices now and scheduled, the system mail history.
import { mutation, rest } from '../../api/rest';
import type {
  BroadcastBody,
  BroadcastResult,
  NoticeBody,
  NoticeList,
  NoticeRow,
  Reason,
  SystemMailList,
} from '../../gen/dto';

/** `GET /admin/api/notices`. */
export function listNotices(signal?: AbortSignal): Promise<NoticeList> {
  return rest.get<NoticeList>('/admin/api/notices', signal ? { signal } : {});
}

/** `POST /admin/api/notices`. */
export function createNotice(body: NoticeBody, reason?: Reason): Promise<NoticeRow> {
  return rest.post<NoticeRow>('/admin/api/notices', mutation(body, reason));
}

/** `PUT /admin/api/notices/{id}`: replaces every editable field. */
export function updateNotice(id: number, body: NoticeBody, reason?: Reason): Promise<NoticeRow> {
  return rest.put<NoticeRow>(`/admin/api/notices/${String(id)}`, mutation(body, reason));
}

/** `DELETE /admin/api/notices/{id}`. */
export function deleteNotice(id: number, reason?: Reason): Promise<{ id: number }> {
  return rest.delete<{ id: number }>(`/admin/api/notices/${String(id)}`, mutation({}, reason));
}

/** `POST /admin/api/broadcast`. */
export function broadcast(
  body: BroadcastBody,
  reason?: Reason,
  confirm?: string,
): Promise<BroadcastResult> {
  return rest.post<BroadcastResult>('/admin/api/broadcast', mutation(body, reason, confirm));
}

/** `GET /admin/api/mail?limit=&character=`: the system mails, newest first. */
export function systemMails(
  limit: number,
  character: number | null,
  signal?: AbortSignal,
): Promise<SystemMailList> {
  return rest.get<SystemMailList>('/admin/api/mail', {
    query: { limit, character: character ?? undefined },
    ...(signal ? { signal } : {}),
  });
}

/** The body that recreates a row (what an edit or a pause sends back). */
export function bodyOf(row: NoticeRow): NoticeBody {
  return {
    text: row.text,
    style: row.style,
    target: row.target,
    start_at: row.start_at,
    repeat: row.repeat,
    end_at: row.end_at,
    paused: row.paused,
  };
}
