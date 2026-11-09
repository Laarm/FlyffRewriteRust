-- 0007_houses_events.sql: Guild house, housing, rainbow race and quiz schema (Plan/13 section 12).
-- Append-only contract (docs/contracts.toml db-migrations): never edit after the first release; add a new file instead.

-- ---- Plan/13 12 ----------
CREATE TABLE guild_houses (               -- tblGuildHouse (small house only, ref/15 §15)
  guild_id     INTEGER PRIMARY KEY REFERENCES guilds(id) ON DELETE CASCADE,
  world_id     INTEGER NOT NULL,          -- WI_GUILDHOUSE_SMALL 212
  upkeep_until INTEGER NOT NULL DEFAULT 0,   -- 0 = expired
  created_at   INTEGER NOT NULL
);
CREATE TABLE guild_house_furniture (      -- tblGuildHouse_Furniture
  seq          INTEGER PRIMARY KEY,       -- from meta.next_guild_furniture_seq (FIX: legacy read MAX(SEQ) from a log table)
  guild_id     INTEGER NOT NULL REFERENCES guild_houses(guild_id) ON DELETE CASCADE,
  item_id      INTEGER NOT NULL,
  installed    INTEGER NOT NULL DEFAULT 0 CHECK (installed IN (0,1)),
  pos_x REAL NOT NULL DEFAULT 0, pos_y REAL NOT NULL DEFAULT 0, pos_z REAL NOT NULL DEFAULT 0,
  angle        REAL    NOT NULL DEFAULT 0,
  keep_until   INTEGER NOT NULL DEFAULT 0,   -- 0 = countdown not started (starts at first setup)
  listed_at    INTEGER NOT NULL,
  installed_at INTEGER
);
CREATE INDEX guild_house_furniture_guild ON guild_house_furniture(guild_id);

CREATE TABLE housing_furniture (          -- tblHousing (mini room, ref/15 §14); an item id appears at most once per owner
  character_id INTEGER NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
  item_id      INTEGER NOT NULL,
  keep_until   INTEGER NOT NULL,
  installed    INTEGER NOT NULL DEFAULT 0 CHECK (installed IN (0,1)),
  pos_x REAL NOT NULL DEFAULT 0, pos_y REAL NOT NULL DEFAULT 0, pos_z REAL NOT NULL DEFAULT 0,
  angle        REAL    NOT NULL DEFAULT 0,
  listed_at    INTEGER NOT NULL,
  PRIMARY KEY (character_id, item_id)
) WITHOUT ROWID;
CREATE TABLE housing_visitors (           -- tblHousing_Visit: friends allowed to enter
  character_id INTEGER NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
  visitor_id   INTEGER NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
  PRIMARY KEY (character_id, visitor_id)
) WITHOUT ROWID;

CREATE TABLE rainbow_race_entries (       -- RAINBOWRACE_TBL (ref/15 §12)
  round        INTEGER NOT NULL,          -- nTimes, meta.rainbow_race_round
  character_id INTEGER NOT NULL,
  state        TEXT    NOT NULL CHECK (state IN ('N','R','W','L','F')),   -- applied, finished (rank reserved), winner/final, lost, failed/no-show
  ranking      INTEGER NOT NULL DEFAULT 0,
  applied_at   INTEGER NOT NULL,
  finished_at  INTEGER,
  PRIMARY KEY (round, character_id)
) WITHOUT ROWID;

CREATE TABLE quiz_questions (             -- tblQuiz + tblQuizAnswer (content; the shipped DB has no rows)
  id            INTEGER PRIMARY KEY,
  quiz_type     INTEGER NOT NULL CHECK (quiz_type IN (1,2)),   -- TYPE_OX 1, TYPE_4C 2
  question      TEXT    NOT NULL,         -- ≤ 1024 bytes
  choices       TEXT    NOT NULL DEFAULT '[]' CHECK (json_valid(choices)),   -- ≤ 4 strings (4C)
  answer        INTEGER NOT NULL,         -- OX: 1 = O, 2 = X; 4C: 1..4
  prize_item_id INTEGER NOT NULL DEFAULT 0,
  prize_count   INTEGER NOT NULL DEFAULT 0,
  enabled       INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),   -- m_chState 'T'
  used_at       INTEGER                   -- set when asked (usp_Quiz_Update)
);
