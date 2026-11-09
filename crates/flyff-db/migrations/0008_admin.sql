-- 0008_admin.sql: Admin console schema (Plan/25 section 11.6, WP-ADMIN-15).
-- Append-only contract (docs/contracts.toml db-migrations): never edit after the first release; add a new file instead.

-- ---- Plan/25 11.6 ----------
CREATE TABLE admin_bookmarks (            -- shared quick locations of the live map (25 3.13)
  id INTEGER PRIMARY KEY, grp TEXT NOT NULL, name TEXT NOT NULL, world INTEGER NOT NULL,
  layer_policy TEXT NOT NULL DEFAULT 'fixed0',      -- 'fixed0' | 'pick_live'
  x REAL NOT NULL, z REAL NOT NULL, zoom REAL, sort INTEGER NOT NULL DEFAULT 0,
  created_by TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE admin_templates (            -- item templates and kits of the grant dialog (25 5.4)
  id INTEGER PRIMARY KEY, kind TEXT NOT NULL CHECK (kind IN ('item','kit')),
  name TEXT NOT NULL COLLATE NOCASE, spec TEXT NOT NULL /* JSON: ItemSpec | Kit */, created_by TEXT NOT NULL,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, UNIQUE (kind, name));
CREATE TABLE admin_notes (                -- moderation notes (25 4.8)
  id INTEGER PRIMARY KEY, subject_kind TEXT NOT NULL CHECK (subject_kind IN ('account','character')),
  subject_id INTEGER NOT NULL, text TEXT NOT NULL, pinned INTEGER NOT NULL DEFAULT 0, author TEXT NOT NULL,
  created_at INTEGER NOT NULL);
CREATE INDEX admin_notes_subject ON admin_notes (subject_kind, subject_id);
CREATE TABLE scheduled_notices (          -- scheduled and recurring notices (25 8.1)
  id INTEGER PRIMARY KEY, text TEXT NOT NULL, style TEXT NOT NULL, target TEXT NOT NULL,
  start_at INTEGER NOT NULL, repeat_min INTEGER, daily_at TEXT, end_at INTEGER, paused INTEGER NOT NULL DEFAULT 0,
  created_by TEXT NOT NULL, created_at INTEGER NOT NULL, last_fired_at INTEGER);
CREATE TABLE admin_totp (                 -- optional TOTP secret of a staff account (25 1.3)
  account_id INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
  secret BLOB NOT NULL, enabled_at INTEGER NOT NULL);
CREATE TABLE admin_idempotency (          -- Idempotency-Key rows of repeatable admin mutations (25 11.6), purged after 24 h
  key TEXT PRIMARY KEY, actor TEXT NOT NULL, route TEXT NOT NULL, body_sha256 BLOB NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('in_flight','done','failed')), result TEXT, created_at INTEGER NOT NULL);
CREATE TABLE admin_ip_blocks (            -- live IP blocks merged into the net.ip_block set (25 4.8)
  id INTEGER PRIMARY KEY, cidr TEXT NOT NULL UNIQUE, reason TEXT NOT NULL, until INTEGER,
  created_by TEXT NOT NULL, created_at INTEGER NOT NULL);
ALTER TABLE accounts ADD COLUMN must_change_password INTEGER NOT NULL DEFAULT 0;
ALTER TABLE pending_grants ADD COLUMN cancelled_at INTEGER;
ALTER TABLE pending_grants ADD COLUMN cancelled_by TEXT;
ALTER TABLE pending_removals ADD COLUMN cancelled_at INTEGER;
ALTER TABLE pending_removals ADD COLUMN cancelled_by TEXT;

-- ---- indexes of the retention job and the admin lists (additions to the plan's statements) ----------
CREATE INDEX admin_idempotency_created ON admin_idempotency (created_at);
CREATE INDEX pending_grants_cancelled ON pending_grants (cancelled_at) WHERE cancelled_at IS NOT NULL;
CREATE INDEX pending_removals_cancelled ON pending_removals (cancelled_at) WHERE cancelled_at IS NOT NULL;
