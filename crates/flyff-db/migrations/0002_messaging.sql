-- 0002_messaging.sql: Messaging and economy schema (Plan/13 section 7).
-- Append-only contract (docs/contracts.toml db-migrations): never edit after the first release; add a new file instead.

-- ---- Plan/13 7 ----------
CREATE TABLE friends (                    -- tblMessenger (__RT_1025): two rows per friendship (ref/14 §9)
  character_id INTEGER NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
  friend_id    INTEGER NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
  blocked      INTEGER NOT NULL DEFAULT 0 CHECK (blocked IN (0,1)),   -- bBlock on MY entry only
  created_at   INTEGER NOT NULL,
  PRIMARY KEY (character_id, friend_id),
  CHECK (character_id <> friend_id)
) WITHOUT ROWID;
CREATE INDEX friends_by_friend ON friends(friend_id);

CREATE TABLE notes (                      -- TAG_TBL offline notes, ≤ 20 undelivered per receiver (game-enforced)
  id           INTEGER PRIMARY KEY,
  to_id        INTEGER NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
  from_id      INTEGER NOT NULL,          -- no FK (sender may be purged)
  text         TEXT    NOT NULL,          -- ≤ 255 bytes
  created_at   INTEGER NOT NULL,
  delivered_at INTEGER
);
CREATE INDEX notes_undelivered ON notes(to_id) WHERE delivered_at IS NULL;

CREATE TABLE party_names (                -- PARTY_TBL: stored advanced-party name of a leader (ref/14 §4.3)
  character_id INTEGER PRIMARY KEY REFERENCES characters(id) ON DELETE CASCADE,
  name         TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (length(name) BETWEEN 1 AND 32)
);

CREATE TABLE mails (                      -- MAIL_TBL (ref/04 §9, ref/05 §8)
  id            INTEGER PRIMARY KEY,      -- from meta.next_mail_id (MailService)
  receiver_id   INTEGER NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
  sender_id     INTEGER NOT NULL DEFAULT 0,   -- 0 = system; no FK
  title         TEXT    NOT NULL,         -- ≤ 32 bytes (MAX_MAILTITLE)
  body          TEXT    NOT NULL,         -- ≤ 256 bytes (MAX_MAILTEXT)
  gold          INTEGER NOT NULL DEFAULT 0,   -- set to 0 when taken
  item          TEXT CHECK (item IS NULL OR json_valid(item)),   -- StoredItem; set to NULL when taken
  created_at    INTEGER NOT NULL,         -- expiry = created_at + 15 days (MAX_KEEP_MAX_DAY)
  read_at       INTEGER,
  gold_taken_at INTEGER,
  item_taken_at INTEGER,
  deleted_at    INTEGER                   -- soft delete (legacy byRead = 90)
);
CREATE INDEX mails_box     ON mails(receiver_id) WHERE deleted_at IS NULL;
CREATE INDEX mails_expiry  ON mails(created_at)  WHERE deleted_at IS NULL;
CREATE INDEX mails_deleted ON mails(deleted_at)  WHERE deleted_at IS NOT NULL;

CREATE TABLE pending_grants (             -- replaces ITEM_SEND_TBL, MAIL_STR_REALTIME, GUILD_STR U7 salaries
  id           INTEGER PRIMARY KEY,
  character_id INTEGER NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
  kind         TEXT    NOT NULL CHECK (kind IN ('item','gold')),
  item         TEXT CHECK (item IS NULL OR json_valid(item)),   -- StoredItem template (serial/obj_id assigned at delivery)
  gold         INTEGER NOT NULL DEFAULT 0,
  source       TEXT    NOT NULL,          -- 'cli' | 'gm:<account>' | 'admin_api' | 'guild_salary:<guild_id>' | 'event:<name>'
  note         TEXT    NOT NULL DEFAULT '',
  created_at   INTEGER NOT NULL,
  delivered_at INTEGER,
  CHECK ((kind = 'item' AND item IS NOT NULL) OR (kind = 'gold' AND gold > 0))
);
CREATE INDEX pending_grants_open ON pending_grants(character_id) WHERE delivered_at IS NULL;

CREATE TABLE pending_removals (           -- replaces ITEM_REMOVE_TBL and tblRemoveItemFromGuildBank (GM confiscation)
  id            INTEGER PRIMARY KEY,
  character_id  INTEGER REFERENCES characters(id) ON DELETE CASCADE,
  guild_id      INTEGER,                  -- no FK (guilds table is created by 0003; GuildService validates)
  location      TEXT NOT NULL CHECK (location IN ('inventory','bank','pocket','guild_bank')),
  item_id       INTEGER NOT NULL DEFAULT 0,   -- 0 = penya
  serial        INTEGER,                  -- optional exact match
  upgrade_level INTEGER, element INTEGER, element_level INTEGER,   -- NULL = any
  count         INTEGER NOT NULL DEFAULT 1 CHECK (count > 0),       -- item count, or penya amount when item_id = 0
  requested_by  TEXT NOT NULL,
  reason        TEXT NOT NULL DEFAULT '',
  created_at    INTEGER NOT NULL,
  done_at       INTEGER,
  removed_count INTEGER,
  CHECK ((character_id IS NULL) <> (guild_id IS NULL)),
  CHECK ((location = 'guild_bank') = (guild_id IS NOT NULL))
);
CREATE INDEX pending_removals_open ON pending_removals(character_id) WHERE done_at IS NULL;
