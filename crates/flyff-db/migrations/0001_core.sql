-- 0001_core.sql: Core schema: meta, server_kv, runtime_override, accounts, characters, documents, banks (Plan/13 section 5).
-- Append-only contract (docs/contracts.toml db-migrations): never edit after the first release; add a new file instead.

-- ---- Plan/13 5.1 meta, server_kv, runtime_override ----------
CREATE TABLE meta (                      -- integer counters (§15)
  key   TEXT PRIMARY KEY,
  value INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE server_kv (                 -- small JSON state that must survive restarts (§15.2)
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL CHECK (json_valid(value)),
  updated_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE runtime_override (          -- D2: GM/admin-changed config values that survive restarts
  key    TEXT PRIMARY KEY,               -- dotted flyff.toml key, e.g. 'rates.monster_exp', 'events.lua.<name>.enabled'
  value  TEXT NOT NULL CHECK (json_valid(value)),
  set_by TEXT NOT NULL,                  -- 'gm:<account>' | 'admin:<token-name>' | 'cli'
  set_at INTEGER NOT NULL
) WITHOUT ROWID;

-- ---- Plan/13 5.2 accounts, account_bans ----------
CREATE TABLE accounts (
  id               INTEGER PRIMARY KEY AUTOINCREMENT,      -- never reused
  name             TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (length(name) BETWEEN 3 AND 16),  -- rule [a-z][a-z0-9]{2,15} (doc 12)
  password_hash    TEXT NOT NULL,          -- argon2id PHC string over md5_hex_lower("kikugalanet"+pw) (D3/D5, doc 12)
  authority        TEXT NOT NULL DEFAULT 'F' CHECK (length(authority) = 1),  -- m_chLoginAuthority ('F' … 'P')
  created_at       INTEGER NOT NULL,
  created_via      TEXT NOT NULL DEFAULT 'cli' CHECK (created_via IN ('cli','web','bootstrap')),
  registration_ip  TEXT,
  last_login_at    INTEGER,
  last_login_ip    TEXT,
  email            TEXT
);

CREATE TABLE account_bans (               -- replaces ACCOUNT_TBL_DETAIL.BlockTime
  id          INTEGER PRIMARY KEY,
  account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  created_at  INTEGER NOT NULL,
  until       INTEGER,                    -- NULL = permanent
  reason      TEXT NOT NULL DEFAULT '',
  created_by  TEXT NOT NULL,              -- 'cli' | 'gm:<account>'
  lifted_at   INTEGER,
  lifted_by   TEXT
);
CREATE INDEX account_bans_active ON account_bans(account_id) WHERE lifted_at IS NULL;

-- ---- Plan/13 5.3 characters ----------
CREATE TABLE characters (
  -- identity / lifecycle / admin-owned (written only by explicit DbOps) ------------------------------
  id                   INTEGER PRIMARY KEY,          -- m_idPlayer; from meta.next_character_id
  account_id           INTEGER NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
  slot                 INTEGER NOT NULL CHECK (slot BETWEEN 0 AND 2),          -- playerslot
  name                 TEXT    NOT NULL COLLATE NOCASE CHECK (length(name) BETWEEN 1 AND 32),
  state                INTEGER NOT NULL DEFAULT 0 CHECK (state IN (0,1,2)),   -- 0 active, 1 deleted, 2 sealed (isblock F/D/S)
  created_at           INTEGER NOT NULL,
  deleted_at           INTEGER,
  sealed_at            INTEGER,
  login_blocked_until  INTEGER,                      -- BlockTime; NULL = not blocked
  authority            TEXT    NOT NULL DEFAULT 'F' CHECK (length(authority) = 1),   -- m_chAuthority
  -- snapshot-owned (written by every CharacterSnapshot) ----------------------------------------------
  sex                  INTEGER NOT NULL CHECK (sex IN (0,1)),
  mover_index          INTEGER NOT NULL,             -- MI_MALE 11 / MI_FEMALE 12
  skin_set             INTEGER NOT NULL DEFAULT 0,
  hair_mesh            INTEGER NOT NULL DEFAULT 0,
  hair_color           INTEGER NOT NULL DEFAULT 0,   -- ARGB
  head_mesh            INTEGER NOT NULL DEFAULT 0,
  hide_coat_mask       INTEGER NOT NULL DEFAULT 0,   -- bit z = m_abHideCoat[z], z = 0..4 (__HIDE_FASHION)
  scale                REAL    NOT NULL DEFAULT 1.0,
  world_id             INTEGER NOT NULL,
  layer                INTEGER NOT NULL DEFAULT 0,
  pos_x REAL NOT NULL, pos_y REAL NOT NULL, pos_z REAL NOT NULL,
  return_world_id      INTEGER NOT NULL DEFAULT 1,
  return_x REAL NOT NULL DEFAULT 0, return_y REAL NOT NULL DEFAULT 0, return_z REAL NOT NULL DEFAULT 0,
  marking_world_id     INTEGER NOT NULL DEFAULT 0,   -- 0 = no mark
  marking_x REAL NOT NULL DEFAULT 0, marking_y REAL NOT NULL DEFAULT 0, marking_z REAL NOT NULL DEFAULT 0,
  motion               INTEGER NOT NULL DEFAULT 0,
  act_state            INTEGER NOT NULL DEFAULT 0,
  act_state_flags      INTEGER NOT NULL DEFAULT 0,
  hp                   INTEGER NOT NULL,
  mp                   INTEGER NOT NULL,
  fp                   INTEGER NOT NULL,
  job                  INTEGER NOT NULL DEFAULT 0,
  level                INTEGER NOT NULL DEFAULT 1,
  max_level            INTEGER NOT NULL DEFAULT 1,
  exp                  INTEGER NOT NULL DEFAULT 0,
  stat_str             INTEGER NOT NULL DEFAULT 15,
  stat_sta             INTEGER NOT NULL DEFAULT 15,
  stat_dex             INTEGER NOT NULL DEFAULT 15,
  stat_int             INTEGER NOT NULL DEFAULT 15,
  stat_points          INTEGER NOT NULL DEFAULT 0,
  skill_points         INTEGER NOT NULL DEFAULT 0,
  skill_level_total    INTEGER NOT NULL DEFAULT 0,
  gold                 INTEGER NOT NULL DEFAULT 0,
  flight_exp           INTEGER NOT NULL DEFAULT 0,
  tutorial_state       INTEGER NOT NULL DEFAULT 0,
  fuel                 INTEGER NOT NULL DEFAULT -1,  -- -1 = no fuel
  fuel_time_acc        INTEGER NOT NULL DEFAULT 0,
  mode_flags           INTEGER NOT NULL DEFAULT 0,
  fame                 INTEGER NOT NULL DEFAULT 0,
  pk_value             INTEGER NOT NULL DEFAULT 0,
  pk_propensity        INTEGER NOT NULL DEFAULT 0,
  pk_exp               INTEGER NOT NULL DEFAULT 0,
  death_exp            INTEGER NOT NULL DEFAULT 0,
  death_level          INTEGER NOT NULL DEFAULT 0,
  angel_exp            INTEGER NOT NULL DEFAULT 0,
  angel_level          INTEGER NOT NULL DEFAULT 0,
  mute_remaining_s     INTEGER NOT NULL DEFAULT 0,
  messenger_state      INTEGER NOT NULL DEFAULT 0,
  play_time_s          INTEGER NOT NULL DEFAULT 0,
  attendance_bits      INTEGER NOT NULL DEFAULT 0,
  attendance_minutes   INTEGER NOT NULL DEFAULT 0,
  attendance_elapsed   INTEGER NOT NULL DEFAULT 0,
  coupon_count         INTEGER NOT NULL DEFAULT 0,
  honor_title_id       INTEGER,                      -- NULL = none (wire -1)
  summoned_pet_obj_id  INTEGER,                      -- NULL = none (wire NULL_ID)
  rest_points          INTEGER NOT NULL DEFAULT 0,
  last_logout_at       INTEGER NOT NULL DEFAULT 0,
  equipment_preview    TEXT    NOT NULL DEFAULT '[]' CHECK (json_valid(equipment_preview)),
  saved_at             INTEGER                       -- commit time of the last snapshot
);
CREATE UNIQUE INDEX characters_name_live   ON characters(name)             WHERE state <> 1;
CREATE UNIQUE INDEX characters_slot_active ON characters(account_id, slot) WHERE state = 0;
CREATE INDEX        characters_account     ON characters(account_id);
CREATE INDEX        characters_deleted     ON characters(deleted_at)       WHERE state = 1;

-- ---- Plan/13 5.5 character_docs ----------
CREATE TABLE character_docs (            -- one row per character; every column is a JSON document (§6)
  character_id INTEGER PRIMARY KEY REFERENCES characters(id) ON DELETE CASCADE,
  inventory    TEXT NOT NULL CHECK (json_valid(inventory)),   -- InventoryDoc
  pockets      TEXT NOT NULL CHECK (json_valid(pockets)),     -- PocketsDoc
  skills       TEXT NOT NULL CHECK (json_valid(skills)),      -- SkillsDoc
  quests       TEXT NOT NULL CHECK (json_valid(quests)),      -- QuestsDoc
  taskbar      TEXT NOT NULL CHECK (json_valid(taskbar)),     -- TaskbarDoc
  buffs        TEXT NOT NULL CHECK (json_valid(buffs)),       -- BuffsDoc
  sm_timers    TEXT NOT NULL CHECK (json_valid(sm_timers)),   -- SmTimersDoc
  honor        TEXT NOT NULL CHECK (json_valid(honor)),       -- HonorDoc
  settings     TEXT NOT NULL CHECK (json_valid(settings))     -- SettingsDoc
);

-- ---- Plan/13 5.6 banks ----------
CREATE TABLE banks (                     -- one 42-slot bank per character (BANK_TBL + BANK_EXT_TBL)
  character_id INTEGER PRIMARY KEY REFERENCES characters(id) ON DELETE CASCADE,
  pin          TEXT    NOT NULL DEFAULT '0000' CHECK (length(pin) = 4 AND pin GLOB '[0-9][0-9][0-9][0-9]'),
  gold         INTEGER NOT NULL DEFAULT 0,
  container    TEXT    NOT NULL CHECK (json_valid(container))     -- BankDoc
);

-- ---- Plan/13 15.1 meta counters ----------
-- `next_*` = next id to hand out; `*_round` = current round number (not "next"). The `init.*` keys
-- (recorded init-only configuration values) are written by flyff_db::open() on a new database because
-- their values come from the configuration.
INSERT INTO meta (key, value) VALUES
  ('next_character_id',        1),
  ('next_guild_id',            1),
  ('next_mail_id',             1),
  ('next_guild_war_id',        1),
  ('next_item_serial',         1),
  ('next_campus_id',           1),
  ('next_couple_id',           1),
  ('next_siege_id',            1),
  ('next_lord_election_id',    1),
  ('next_guild_furniture_seq', 1),
  ('next_quiz_event_id',       1),
  ('tax_round',                0),
  ('secret_room_round',        0),
  ('rainbow_race_round',       0),
  ('gc1to1_round',             0);
