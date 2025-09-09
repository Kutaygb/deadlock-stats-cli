-- players table
CREATE TABLE IF NOT EXISTS players (
  account_id      BIGINT       PRIMARY KEY,
  steamid64       TEXT         UNIQUE NOT NULL CHECK (steamid64 ~ '^\d{17}$'),
  personaname     TEXT,
  profileurl      TEXT,
  avatar          TEXT,
  avatarmedium    TEXT,
  avatarfull      TEXT,
  countrycode     TEXT,
  realname        TEXT,
  profile_extra   JSONB        NOT NULL DEFAULT '{}'::jsonb,
  profile_updated_at TIMESTAMPTZ,
  profile_domain  TEXT GENERATED ALWAYS AS (NULLIF(split_part(profileurl, '/', 3), '')) STORED
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_players_steamid64 ON players(steamid64);
-- JSON index only if needed for filtering later
-- CREATE INDEX IF NOT EXISTS idx_players_profile_extra_gin ON players USING GIN (profile_extra);

-- latest_mmr snapshot
CREATE TABLE IF NOT EXISTS latest_mmr (
  account_id      BIGINT PRIMARY KEY REFERENCES players(account_id) ON DELETE CASCADE,
  match_id        BIGINT,
  start_time      TIMESTAMPTZ,
  player_score    DOUBLE PRECISION,
  rank            INT,
  division        INT,
  division_tier   INT,
  extra           JSONB NOT NULL DEFAULT '{}'::jsonb
);

-- mmr_history append-only
CREATE TABLE IF NOT EXISTS mmr_history (
  account_id      BIGINT REFERENCES players(account_id) ON DELETE CASCADE,
  start_time      TIMESTAMPTZ NOT NULL,
  match_id        BIGINT,
  player_score    DOUBLE PRECISION,
  rank            INT,
  division        INT,
  division_tier   INT,
  extra           JSONB NOT NULL DEFAULT '{}'::jsonb,
  PRIMARY KEY (account_id, start_time)
);
CREATE INDEX IF NOT EXISTS idx_mmr_history_account_time ON mmr_history (account_id, start_time DESC);

-- hero_stats_current
CREATE TABLE IF NOT EXISTS hero_stats_current (
  account_id      BIGINT REFERENCES players(account_id) ON DELETE CASCADE,
  hero_id         INT    NOT NULL,
  matches_played  INT,
  wins            INT,
  last_played     TIMESTAMPTZ,
  time_played     INT,
  ending_level    DOUBLE PRECISION,
  kills           INT,
  deaths          INT,
  assists         INT,
  kills_per_min   DOUBLE PRECISION,
  deaths_per_min  DOUBLE PRECISION,
  assists_per_min DOUBLE PRECISION,
  networth_per_min DOUBLE PRECISION,
  last_hits_per_min DOUBLE PRECISION,
  damage_per_min  DOUBLE PRECISION,
  damage_taken_per_min DOUBLE PRECISION,
  obj_damage_per_min DOUBLE PRECISION,
  accuracy        DOUBLE PRECISION,
  crit_shot_rate  DOUBLE PRECISION,
  extra           JSONB NOT NULL DEFAULT '{}'::jsonb,
  PRIMARY KEY (account_id, hero_id)
);

ALTER TABLE hero_stats_current
  ADD COLUMN IF NOT EXISTS win_rate DOUBLE PRECISION
  GENERATED ALWAYS AS (
    CASE WHEN matches_played > 0 THEN (wins::double precision / matches_played) ELSE NULL END
  ) STORED;

CREATE INDEX IF NOT EXISTS idx_hero_stats_current_last_played ON hero_stats_current (account_id, last_played DESC);
CREATE INDEX IF NOT EXISTS idx_hero_stats_current_hero ON hero_stats_current (hero_id);

-- hero_stats_history append-only
CREATE TABLE IF NOT EXISTS hero_stats_history (
  account_id      BIGINT NOT NULL REFERENCES players(account_id) ON DELETE CASCADE,
  hero_id         INT    NOT NULL,
  last_played     TIMESTAMPTZ NOT NULL,
  snapshot_json   JSONB  NOT NULL,
  PRIMARY KEY (account_id, hero_id, last_played)
);
CREATE INDEX IF NOT EXISTS idx_hero_stats_hist_time ON hero_stats_history (account_id, last_played DESC);

