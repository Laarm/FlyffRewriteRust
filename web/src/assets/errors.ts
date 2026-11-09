// The error of every asset request (Plan/40 section 9.2, 9.7): the reason tells the caller what to do.

/**
 * * `notFound`: 404/410 or a key without a URL (not client-visible): the asset is invisible/silent, never retried
 *   (KQ-30, KQ-02), logged once per key;
 * * `unauthorized`: 401/403, a gated asset without the cookie: treated like `notFound`;
 * * `bad`: 400, the path was not canonical (a client bug);
 * * `network`: the request failed after the retries; the requester may `acquire` again later;
 * * `server`: 5xx after the retries, or 503 `Retry-After` more than ten times;
 * * `aborted`: cancelled by the requester, a generation bump or a release; not a failure of the asset;
 * * `decode`: the worker could not decode the bytes (deterministic, never retried).
 */
export type AssetErrorReason =
  'notFound' | 'unauthorized' | 'bad' | 'network' | 'server' | 'aborted' | 'decode';

export class AssetError extends Error {
  constructor(
    readonly reason: AssetErrorReason,
    /** `keyString` of the asset (or the URL). */
    readonly key: string,
    message?: string,
    readonly status?: number,
    cause?: unknown,
  ) {
    super(message === undefined ? `${reason}: ${key}` : `${reason}: ${key}: ${message}`, {
      cause,
    });
    this.name = 'AssetError';
  }

  /** `true` for the reasons that are final for the session (the file does not exist for this client). */
  get missing(): boolean {
    return this.reason === 'notFound' || this.reason === 'unauthorized' || this.reason === 'bad';
  }
}

export function isAssetError(e: unknown): e is AssetError {
  return e instanceof AssetError;
}
