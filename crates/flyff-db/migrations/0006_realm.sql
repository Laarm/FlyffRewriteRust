-- 0006_realm.sql: Lord, tax and secret room schema (Plan/13 section 11).
-- Append-only contract (docs/contracts.toml db-migrations): never edit after the first release; add a new file instead.

-- ---- Plan/13 11 ----------
CREATE TABLE lord_elections (             -- tblLordElection + tblLord (ref/15 §9)
  id          INTEGER PRIMARY KEY,        -- idElection, from meta.next_lord_election_id
  state       INTEGER NOT NULL CHECK (state BETWEEN 0 AND 3),   -- 0 eReady, 1 eCandidacy, 2 eVote, 3 eExpired
  begin_at    INTEGER NOT NULL DEFAULT 0, -- tBegin (start of candidacy), 0 = not scheduled
  requirement INTEGER NOT NULL DEFAULT 0, -- vote requirement
  lord_id     INTEGER,                    -- winner, set at EndVote (NULL = no lord for that term)
  ended_at    INTEGER
);
CREATE TABLE lord_candidates (            -- tblLordCandidates
  election_id  INTEGER NOT NULL REFERENCES lord_elections(id) ON DELETE CASCADE,
  character_id INTEGER NOT NULL,
  deposit      INTEGER NOT NULL,
  votes        INTEGER NOT NULL DEFAULT 0,
  pledge       TEXT    NOT NULL DEFAULT '',   -- ≤ 255 bytes
  left_out     INTEGER NOT NULL DEFAULT 0 CHECK (left_out IN (0,1)),   -- IsLeftOut 'D'
  created_at   INTEGER NOT NULL,          -- tCreate (tie-break)
  PRIMARY KEY (election_id, character_id)
) WITHOUT ROWID;
CREATE TABLE lord_votes (                 -- tblLordElector: one vote per elector per election
  election_id INTEGER NOT NULL REFERENCES lord_elections(id) ON DELETE CASCADE,
  elector_id  INTEGER NOT NULL,
  voted_at    INTEGER NOT NULL,
  PRIMARY KEY (election_id, elector_id)
) WITHOUT ROWID;
CREATE TABLE lord_events (                -- tblLordEvent: running global EXP/drop boosts
  character_id INTEGER PRIMARY KEY,       -- creator
  ticks_left   INTEGER NOT NULL,          -- minutes left (starts at 60)
  exp_factor   REAL    NOT NULL,
  item_factor  REAL    NOT NULL,
  created_at   INTEGER NOT NULL
);
CREATE TABLE lord_skills (                -- tblLordSkill: cooldowns
  skill_id     INTEGER PRIMARY KEY,
  cooldown_min INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE tax_owners (                 -- TAX_INFO_TBL (ref/15 §11)
  round         INTEGER NOT NULL,         -- nTimes, meta.tax_round
  continent     INTEGER NOT NULL CHECK (continent IN (15, 240, 255)),   -- CONT_EAST, CONT_WEST, CONT_ALL (lord)
  owner_id      INTEGER,                  -- guild id (15/240) or character id (255); NULL = no owner
  next_owner_id INTEGER,
  rate_set      INTEGER NOT NULL DEFAULT 0 CHECK (rate_set IN (0,1)),
  changed_at    INTEGER NOT NULL,
  PRIMARY KEY (round, continent)
) WITHOUT ROWID;
CREATE TABLE tax_details (                -- TAX_DETAIL_TBL
  round     INTEGER NOT NULL,
  continent INTEGER NOT NULL,
  kind      INTEGER NOT NULL CHECK (kind IN (0,1,2)),   -- TAX_SALES, TAX_PURCHASE, TAX_ADMISSION
  rate      INTEGER NOT NULL DEFAULT 0,
  next_rate INTEGER NOT NULL DEFAULT 0,
  tx_count  INTEGER NOT NULL DEFAULT 0,
  gold      INTEGER NOT NULL DEFAULT 0,
  perin     INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (round, continent, kind)
) WITHOUT ROWID;

CREATE TABLE secret_room_tenders (        -- SECRET_ROOM_TBL (ref/15 §10)
  round       INTEGER NOT NULL,           -- nTimes, meta.secret_room_round
  continent   INTEGER NOT NULL CHECK (continent IN (15, 240)),
  guild_id    INTEGER NOT NULL,
  penya       INTEGER NOT NULL,
  state       TEXT    NOT NULL CHECK (state IN ('T','C','F','W','L')),   -- tendered, cancelled, failed ranking, won, lost
  world_id    INTEGER NOT NULL DEFAULT 0,
  war_state   INTEGER NOT NULL DEFAULT 0,
  kills       INTEGER NOT NULL DEFAULT 0,
  tendered_at INTEGER NOT NULL,
  PRIMARY KEY (round, continent, guild_id)
) WITHOUT ROWID;
CREATE TABLE secret_room_lineups (        -- SECRET_ROOM_MEMBER_TBL
  round        INTEGER NOT NULL,
  continent    INTEGER NOT NULL,
  guild_id     INTEGER NOT NULL,
  character_id INTEGER NOT NULL,
  PRIMARY KEY (round, continent, guild_id, character_id)
) WITHOUT ROWID;
