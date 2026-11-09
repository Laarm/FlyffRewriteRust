-- 0003_guilds.sql: Guild schema (Plan/13 section 8).
-- Append-only contract (docs/contracts.toml db-migrations): never edit after the first release; add a new file instead.

-- ---- Plan/13 8 ----------
CREATE TABLE guilds (                     -- GUILD_TBL (ref/04 §10, ref/14 §5.1)
  id             INTEGER PRIMARY KEY,     -- from meta.next_guild_id
  name           TEXT    NOT NULL DEFAULT '' COLLATE NOCASE,   -- '' until named (ref/14 §5.4)
  level          INTEGER NOT NULL DEFAULT 1,
  gold           INTEGER NOT NULL DEFAULT 0,   -- m_nGoldGuild: guild bank penya pool
  pxp            INTEGER NOT NULL DEFAULT 0,   -- m_dwContributionPxp
  logo           INTEGER NOT NULL DEFAULT 0,
  notice         TEXT    NOT NULL DEFAULT '',  -- ≤ 127 bytes
  rank_powers    TEXT    NOT NULL DEFAULT '[255,0,0,0,0]' CHECK (json_array_length(rank_powers) = 5),   -- m_adwPower[rank]
  rank_salaries  TEXT    NOT NULL DEFAULT '[0,0,0,0,0]'   CHECK (json_array_length(rank_salaries) = 5), -- m_adwPenya[rank]
  war_wins       INTEGER NOT NULL DEFAULT 0,
  war_losses     INTEGER NOT NULL DEFAULT 0,
  war_surrenders INTEGER NOT NULL DEFAULT 0,
  war_points     INTEGER NOT NULL DEFAULT 0,   -- m_nWinPoint
  created_at     INTEGER NOT NULL
);
CREATE UNIQUE INDEX guilds_name ON guilds(name) WHERE name <> '';

CREATE TABLE guild_members (              -- GUILD_MEMBER_TBL
  character_id     INTEGER PRIMARY KEY REFERENCES characters(id) ON DELETE CASCADE,
  guild_id         INTEGER NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
  rank             INTEGER NOT NULL CHECK (rank BETWEEN 0 AND 4),     -- GUD_MASTER 0 … GUD_ROOKIE 4
  class            INTEGER NOT NULL DEFAULT 0 CHECK (class BETWEEN 0 AND 2),
  alias            TEXT    NOT NULL DEFAULT '',  -- nickname, 2..12 bytes when set
  contributed_gold INTEGER NOT NULL DEFAULT 0,   -- m_nGiveGold (decreases on penya withdrawal; may go negative)
  contributed_pxp  INTEGER NOT NULL DEFAULT 0,   -- m_dwGivePxpCount
  surrenders       INTEGER NOT NULL DEFAULT 0,
  war_id           INTEGER NOT NULL DEFAULT 0,   -- current war participation (0 = none / surrendered)
  joined_at        INTEGER NOT NULL
);
CREATE INDEX        guild_members_guild  ON guild_members(guild_id);
CREATE UNIQUE INDEX guild_members_master ON guild_members(guild_id) WHERE rank = 0;

CREATE TABLE guild_banks (                -- GUILD_BANK_TBL + GUILD_BANK_EXT_TBL
  guild_id  INTEGER PRIMARY KEY REFERENCES guilds(id) ON DELETE CASCADE,
  container TEXT NOT NULL CHECK (json_valid(container))     -- GuildBankDoc (42 slots); gold is guilds.gold
);

CREATE TABLE guild_bank_log (             -- the only kept log table (D5): serves GUILDLOG_VIEW (ref/04 §10.3)
  id            INTEGER PRIMARY KEY,
  guild_id      INTEGER NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
  character_id  INTEGER NOT NULL,         -- 0 = system; no FK
  kind          TEXT    NOT NULL CHECK (kind IN ('A','D','I','O','C','P','R','L')),
  item_id       INTEGER NOT NULL DEFAULT 0,
  upgrade_level INTEGER NOT NULL DEFAULT 0,
  count         INTEGER NOT NULL DEFAULT 0,   -- item count, or penya amount for I/O/P
  at            INTEGER NOT NULL
);
CREATE INDEX guild_bank_log_view ON guild_bank_log(guild_id, kind, id DESC);

CREATE TABLE guild_wars (                 -- GUILD_WAR_TBL (+ LOG_GUILD_WAR_TBL), history kept
  id                 INTEGER PRIMARY KEY, -- from meta.next_guild_war_id
  declarer_guild_id  INTEGER NOT NULL,    -- no FK: history survives disband
  acceptor_guild_id  INTEGER NOT NULL,
  started_at         INTEGER NOT NULL,
  ended_at           INTEGER,
  result             INTEGER,             -- NULL = running; WR_* 0..9 (ref/14 §6.1)
  decl_members INTEGER NOT NULL, decl_deaths INTEGER NOT NULL DEFAULT 0,
  decl_surrenders INTEGER NOT NULL DEFAULT 0, decl_absent_s INTEGER NOT NULL DEFAULT 0,
  acpt_members INTEGER NOT NULL, acpt_deaths INTEGER NOT NULL DEFAULT 0,
  acpt_surrenders INTEGER NOT NULL DEFAULT 0, acpt_absent_s INTEGER NOT NULL DEFAULT 0,
  decl_points_after  INTEGER,
  acpt_points_after  INTEGER
);
CREATE INDEX guild_wars_running ON guild_wars(id) WHERE result IS NULL;

CREATE TABLE guild_rejoin_cooldowns (     -- CHARACTER_TBL.m_tGuildMember (cooldown disabled by default, __REMOVE_GUILD_COOLDOWN)
  character_id INTEGER PRIMARY KEY REFERENCES characters(id) ON DELETE CASCADE,
  until        INTEGER NOT NULL
);

CREATE TABLE guild_quests (               -- GUILD_QUEST_TBL; used only if quest content exists (D1)
  guild_id   INTEGER NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
  quest_id   INTEGER NOT NULL,
  state      INTEGER NOT NULL,
  started_at INTEGER NOT NULL,
  ended_at   INTEGER,
  PRIMARY KEY (guild_id, quest_id)        -- FIX: legacy PK (serverindex, m_idGuild) allowed one row per guild
) WITHOUT ROWID;
