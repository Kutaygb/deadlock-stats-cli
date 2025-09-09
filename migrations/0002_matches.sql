-- matches metadata
CREATE TABLE IF NOT EXISTS matches (
  match_id       BIGINT      PRIMARY KEY,
  start_time     TIMESTAMPTZ,
  duration_s     INT,
  winner_team    TEXT,
  average_badge  INT,
  region         TEXT,
  patch_version  TEXT,
  info_json      JSONB NOT NULL DEFAULT '{}'::jsonb,
  fetched_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_matches_start_time ON matches(start_time DESC);

-- participants per match
CREATE TABLE IF NOT EXISTS match_players (
  match_id       BIGINT REFERENCES matches(match_id) ON DELETE CASCADE,
  account_id     BIGINT REFERENCES players(account_id) ON DELETE CASCADE,
  hero_id        INT,
  team           TEXT,
  party_id       BIGINT,
  lane           TEXT,
  is_victory     BOOLEAN,
  kills          INT,
  deaths         INT,
  assists        INT,
  networth       BIGINT,
  damage         BIGINT,
  damage_taken   BIGINT,
  obj_damage     BIGINT,
  last_hits      INT,
  accuracy       DOUBLE PRECISION,
  crit_shot_rate DOUBLE PRECISION,
  extra_json     JSONB NOT NULL DEFAULT '{}'::jsonb,
  PRIMARY KEY (match_id, account_id)
);

CREATE INDEX IF NOT EXISTS idx_match_players_account ON match_players(account_id);
CREATE INDEX IF NOT EXISTS idx_match_players_match ON match_players(match_id);

