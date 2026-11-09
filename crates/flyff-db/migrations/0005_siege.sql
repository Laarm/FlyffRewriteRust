-- 0005_siege.sql: Guild siege and GC1to1 schema (Plan/13 section 10).
-- Append-only contract (docs/contracts.toml db-migrations): never edit after the first release; add a new file instead.

-- ---- Plan/13 10 ----------
CREATE TABLE guild_sieges (               -- tblCombatInfo ("guild combat", ref/04 §11, ref/14 §7)
  id              INTEGER PRIMARY KEY,    -- CombatID, from meta.next_siege_id
  status          INTEGER NOT NULL CHECK (status IN (10,20,30)),   -- 10 open/applying, 20 started, 30 ended
  opened_at       INTEGER NOT NULL,
  started_at      INTEGER,
  ended_at        INTEGER,
  winner_guild_id INTEGER,
  winner_streak   INTEGER NOT NULL DEFAULT 0,   -- StraightWin of the winner
  best_player_id  INTEGER
);
CREATE TABLE guild_siege_bids (           -- tblCombatJoinGuild
  siege_id      INTEGER NOT NULL REFERENCES guild_sieges(id),
  guild_id      INTEGER NOT NULL,         -- no FK (history)
  status        INTEGER NOT NULL CHECK (status IN (10,11,12,20,30,31,32)),
                -- 10 applied, 11 cancelled, 12 re-applied, 20 fighting, 30 graded/reward pending, 31 paid, 32 expired
  bid           INTEGER NOT NULL,         -- CombatFee
  refund        INTEGER NOT NULL DEFAULT 0,   -- ReturnCombatFee
  reward        INTEGER NOT NULL DEFAULT 0,
  points        INTEGER NOT NULL DEFAULT 0,
  straight_wins INTEGER NOT NULL DEFAULT 0,
  applied_at    INTEGER NOT NULL,
  cancelled_at  INTEGER,
  rewarded_at   INTEGER,
  PRIMARY KEY (siege_id, guild_id)
) WITHOUT ROWID;
CREATE TABLE guild_siege_players (        -- tblCombatJoinPlayer
  siege_id     INTEGER NOT NULL REFERENCES guild_sieges(id),
  guild_id     INTEGER NOT NULL,
  character_id INTEGER NOT NULL,
  status       INTEGER NOT NULL DEFAULT 30 CHECK (status IN (30,31,32)),
  points       INTEGER NOT NULL DEFAULT 0,
  reward       INTEGER NOT NULL DEFAULT 0,
  rewarded_at  INTEGER,
  PRIMARY KEY (siege_id, guild_id, character_id)
) WITHOUT ROWID;
CREATE INDEX guild_siege_players_rank ON guild_siege_players(character_id);

CREATE TABLE gc1to1_tenders (             -- GUILD_COMBAT_1TO1_TENDER_TBL; feature off by default (ref/14 §8)
  round       INTEGER NOT NULL,           -- combatID, meta.gc1to1_round
  guild_id    INTEGER NOT NULL,
  penya       INTEGER NOT NULL,
  state       TEXT    NOT NULL CHECK (state IN ('T','F','C','E','N','G')),   -- T tendered, F failed, C cancelled, E/N/G refund states (open question 10)
  tendered_at INTEGER NOT NULL,
  PRIMARY KEY (round, guild_id)
) WITHOUT ROWID;
CREATE TABLE gc1to1_lineups (             -- GUILD_COMBAT_1TO1_BATTLE_PERSON_TBL (line-up order only)
  round        INTEGER NOT NULL,
  guild_id     INTEGER NOT NULL,
  seq          INTEGER NOT NULL,          -- fight order
  character_id INTEGER NOT NULL,
  PRIMARY KEY (round, guild_id, seq)
) WITHOUT ROWID;
