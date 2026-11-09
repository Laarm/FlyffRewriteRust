// `GET /api/bootstrap` consumption (Plan/40 section 3.3, WP-CLIENT-11): the TS view of 12 section 6.2's FLAT
// `BootstrapResponse` and the typed client features. `readBootstrap` reads exactly that shape (a nested `assets{...}`
// object of the 24 draft is ignored); `parseFeatures` validates `ClientFeatureConfig` against the generated type and
// fills documented defaults for what is missing or mistyped (with a warning each).

import { DEFAULT_CLIENT_FEATURES, type ClientFeatureConfig } from '../gen/config/clientFeatures';
import type { PrepareProgress } from '../assets/prepare';
import type { BootstrapAuth, ServerStatus } from '../stores/session';

export interface Bootstrap {
  readonly serverName: string;
  readonly serverBuild: string;
  /** The embedded web build id: the client compares it with `__FLYFF_BUILD_ID__`. */
  readonly buildId: string;
  readonly protoHash: string;
  readonly dataVersion: string;
  readonly bundleManifestUrl: string;
  readonly assetManifestUrl: string;
  /** `!web.public_assets`: gated bundles and assets load only after login (24 section 13). */
  readonly gated: boolean;
  readonly pageTitle: string;
  /** The `ClientFeatureConfig` JSON, to be validated by `parseFeatures`. */
  readonly features: unknown;
  readonly auth: BootstrapAuth;
  readonly status: ServerStatus;
  readonly prepare: PrepareProgress;
  readonly serverTimeMs: number;
  /** IANA name for absolute event times (22 OQ7); `''` = unknown (the browser's zone). */
  readonly serverTimezone: string;
  readonly registerUrl: string;
}

export class BootstrapError extends Error {
  constructor(
    readonly field: string,
    message: string,
  ) {
    super(`/api/bootstrap: ${message}`);
    this.name = 'BootstrapError';
  }
}

export const DEFAULT_AUTH: BootstrapAuth = {
  md5_salt: 'kikugalanet',
  allow_registration: true,
  register_url: '',
  account_min_len: 3,
  account_max_len: 16,
  password_min_len: 3,
  password_max_len: 15,
  delete_requires_password: true,
  token_grace_s: 60,
};

export const IDLE_PREPARE: PrepareProgress = {
  active: false,
  phase: 'idle',
  percent: 100,
  done: 0,
  total: 0,
  kind: '',
  current: '',
  waiting: 0,
};

const STATUSES: readonly ServerStatus[] = ['open', 'maintenance', 'shutting_down', 'full'];

function obj(v: unknown): Record<string, unknown> | null {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
    ? (v as Record<string, unknown>)
    : null;
}

function str(o: Record<string, unknown>, key: string, fallback?: string): string {
  const v = o[key];
  if (typeof v === 'string') return v;
  if (fallback !== undefined) return fallback;
  throw new BootstrapError(key, `"${key}" is missing or not a string`);
}

function bool(o: Record<string, unknown>, key: string, fallback: boolean): boolean {
  const v = o[key];
  return typeof v === 'boolean' ? v : fallback;
}

function int(o: Record<string, unknown>, key: string, fallback: number): number {
  const v = o[key];
  return typeof v === 'number' && Number.isFinite(v) ? v : fallback;
}

function readAuth(raw: unknown): BootstrapAuth {
  const a = obj(raw);
  if (!a) throw new BootstrapError('auth', '"auth" is missing');
  return {
    md5_salt: str(a, 'md5_salt', DEFAULT_AUTH.md5_salt),
    allow_registration: bool(a, 'allow_registration', DEFAULT_AUTH.allow_registration),
    register_url: str(a, 'register_url', ''),
    account_min_len: int(a, 'account_min_len', DEFAULT_AUTH.account_min_len),
    account_max_len: int(a, 'account_max_len', DEFAULT_AUTH.account_max_len),
    password_min_len: int(a, 'password_min_len', DEFAULT_AUTH.password_min_len),
    password_max_len: int(a, 'password_max_len', DEFAULT_AUTH.password_max_len),
    delete_requires_password: bool(
      a,
      'delete_requires_password',
      DEFAULT_AUTH.delete_requires_password,
    ),
    token_grace_s: int(a, 'token_grace_s', DEFAULT_AUTH.token_grace_s),
  };
}

function readPrepare(raw: unknown): PrepareProgress {
  const p = obj(raw);
  if (!p) return IDLE_PREPARE;
  const active = bool(p, 'active', false);
  return {
    active,
    phase: str(p, 'phase', active ? 'prewarm' : 'idle'),
    percent: int(p, 'percent', active ? 0 : 100),
    done: int(p, 'done', 0),
    total: int(p, 'total', 0),
    kind: str(p, 'kind', ''),
    current: str(p, 'current', ''),
    waiting: int(p, 'waiting', 0),
  };
}

/**
 * Reads the flat `BootstrapResponse` of Plan/12 section 6.2 (plus `gated`, `prepare` and `server_timezone`).
 * Throws `BootstrapError` naming the field when something the boot cannot do without is missing: the build id and
 * protocol hash (the version guard), the two manifest URLs and the `auth` object.
 */
export function readBootstrap(json: unknown): Bootstrap {
  const o = obj(json);
  if (!o) throw new BootstrapError('', 'the answer is not a JSON object');
  const status = o['status'];
  return {
    serverName: str(o, 'server_name', ''),
    serverBuild: str(o, 'server_build', ''),
    buildId: str(o, 'build_id'),
    protoHash: str(o, 'proto_hash'),
    dataVersion: str(o, 'data_version', ''),
    bundleManifestUrl: str(o, 'bundle_manifest_url'),
    assetManifestUrl: str(o, 'asset_manifest_url'),
    gated: bool(o, 'gated', false),
    pageTitle: str(o, 'page_title', ''),
    features: o['features'],
    auth: readAuth(o['auth']),
    status: STATUSES.find((s) => s === status) ?? 'open',
    prepare: readPrepare(o['prepare']),
    serverTimeMs: int(o, 'server_time_ms', 0),
    serverTimezone: str(o, 'server_timezone', ''),
    registerUrl: str(obj(o['auth']) ?? {}, 'register_url', ''),
  };
}

/** The result of `parseFeatures`. */
export interface ParsedFeatures {
  readonly config: ClientFeatureConfig;
  /** One line per key that was missing or of the wrong type (the defaults filled in). */
  readonly warnings: readonly string[];
}

/**
 * Validates the `features` object of the bootstrap against `ClientFeatureConfig`: every key of the generated default
 * must be present with the same JSON type; a missing or mistyped key takes its default and produces a warning;
 * unknown keys are ignored (a newer server).
 */
export function parseFeatures(json: unknown): ParsedFeatures {
  const warnings: string[] = [];
  const src = obj(json);
  if (!src) {
    warnings.push('features: missing, using the defaults of this build');
    return { config: DEFAULT_CLIENT_FEATURES, warnings };
  }
  const out: Record<string, Record<string, unknown>> = {};
  for (const [sectionName, defaults] of Object.entries(DEFAULT_CLIENT_FEATURES) as [
    string,
    Record<string, unknown>,
  ][]) {
    const section = obj(src[sectionName]);
    const merged: Record<string, unknown> = {};
    for (const [key, def] of Object.entries(defaults)) {
      const v = section?.[key];
      if (
        v !== undefined &&
        typeof v === typeof def &&
        !(typeof v === 'number' && !Number.isFinite(v))
      ) {
        merged[key] = v;
      } else {
        merged[key] = def;
        warnings.push(
          v === undefined
            ? `features.${sectionName}.${key}: missing, default ${JSON.stringify(def)}`
            : `features.${sectionName}.${key}: expected ${typeof def}, got ${typeof v}; default ${JSON.stringify(def)}`,
        );
      }
    }
    out[sectionName] = merged;
  }
  // `hit_attr_rule` is an enum
  const fidelity = out['fidelity'];
  if (
    fidelity &&
    fidelity['hit_attr_rule'] !== 'legacy' &&
    fidelity['hit_attr_rule'] !== 'window'
  ) {
    warnings.push(
      `features.fidelity.hit_attr_rule: "${String(fidelity['hit_attr_rule'])}" is not legacy|window`,
    );
    fidelity['hit_attr_rule'] = DEFAULT_CLIENT_FEATURES.fidelity.hit_attr_rule;
  }
  return { config: out as unknown as ClientFeatureConfig, warnings };
}
