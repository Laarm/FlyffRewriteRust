-- 0004_relations.sql: Couples and campus schema (Plan/13 section 9).
-- Append-only contract (docs/contracts.toml db-migrations): never edit after the first release; add a new file instead.

-- ---- Plan/13 9 ----------
CREATE TABLE couples (                    -- tblCouple
  id         INTEGER PRIMARY KEY,         -- from meta.next_couple_id
  experience INTEGER NOT NULL DEFAULT 0,  -- couple minutes (ref/14 §11.1)
  created_at INTEGER NOT NULL
);
CREATE TABLE couple_members (             -- tblCouplePlayer: PK guarantees one couple per character in either role
  character_id INTEGER PRIMARY KEY REFERENCES characters(id) ON DELETE CASCADE,
  couple_id    INTEGER NOT NULL REFERENCES couples(id) ON DELETE CASCADE
);
CREATE INDEX couple_members_couple ON couple_members(couple_id);
CREATE TABLE couple_proposals (           -- tblPropose: proposal cooldown (0 s with __NO_COUPLE_CD → rows rarely exist)
  proposer_id    INTEGER PRIMARY KEY REFERENCES characters(id) ON DELETE CASCADE,
  cooldown_until INTEGER NOT NULL
);

CREATE TABLE campuses (                   -- tblCampus (master/pupil)
  id         INTEGER PRIMARY KEY,         -- from meta.next_campus_id
  created_at INTEGER NOT NULL
);
CREATE TABLE campus_members (             -- tblCampusMember
  character_id INTEGER PRIMARY KEY REFERENCES characters(id) ON DELETE CASCADE,
  campus_id    INTEGER NOT NULL REFERENCES campuses(id) ON DELETE CASCADE,
  role         INTEGER NOT NULL CHECK (role IN (1,2)),   -- 1 master, 2 pupil
  joined_at    INTEGER NOT NULL
);
CREATE INDEX        campus_members_campus ON campus_members(campus_id);
CREATE UNIQUE INDEX campus_members_master ON campus_members(campus_id) WHERE role = 1;
CREATE TABLE campus_points (              -- CHARACTER_TBL.m_nCampusPoint moved here (service-owned, may be negative)
  character_id INTEGER PRIMARY KEY REFERENCES characters(id) ON DELETE CASCADE,
  points       INTEGER NOT NULL
);
