use crate::models::{HeroStats, MMRHistory, MatchMeta, PlayerInMatch, SteamProfile};
use crate::ui::CombinedPayload;
use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use sqlx::{postgres::PgPoolOptions, PgPool, Postgres, Transaction};
use std::time::Duration;
use url::Url;

pub struct DbPool(pub PgPool);

pub async fn connect() -> Result<DbPool> {
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        // default local per spec
        "postgres://postgres:@localhost:5432/deadlock".to_string()
    });
    ensure_database_exists(&database_url).await?;
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&database_url)
        .await?;
    Ok(DbPool(pool))
}

async fn ensure_database_exists(database_url: &str) -> Result<()> {
    let url = Url::parse(database_url)?;
    let db_name = url.path().trim_start_matches('/').to_string();
    if db_name.is_empty() {
        // nothing to ensure
        return Ok(());
    }
    // try connecting directly; if it works, DB exists
    if PgPoolOptions::new()
        .acquire_timeout(Duration::from_secs(3))
        .connect(database_url)
        .await
        .is_ok()
    {
        return Ok(());
    }

    // connect to maintenance DB "postgres" and create
    let mut admin_url = url.clone();
    admin_url.set_path("/postgres");
    let admin_pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect(admin_url.as_str())
        .await?;
    // Use EXISTS to get a non-null boolean in a single row
    let exists: bool = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1) AS "exists!: bool""#,
        db_name
    )
    .fetch_one(&admin_pool)
    .await?;
    if !exists {
        // CREATE DATABASE needs dynamic SQL; postgres does not allow param for db name
        let sql = format!("CREATE DATABASE \"{}\"", db_name.replace('"', ""));
        sqlx::query(&sql).execute(&admin_pool).await?;
    }
    Ok(())
}

pub async fn migrate(pool: &PgPool) -> Result<()> {
    sqlx::migrate!().run(pool).await?;
    Ok(())
}

#[derive(Debug, Default)]
pub struct IngestResult {
    pub heroes_upserted: usize,
    pub hero_history_added: usize,
    pub mmr_updated: bool,
}

pub async fn ingest_player(pool: &PgPool, payload: &CombinedPayload) -> Result<IngestResult> {
    let mut tx = pool.begin().await?;

    upsert_player(&mut tx, payload.account_id as i64, &payload.steamid64, &payload.profile).await?;

    let mut res = IngestResult::default();
    if let Some(mmr) = &payload.latest_mmr {
        upsert_latest_mmr(&mut tx, payload.account_id as i64, mmr).await?;
        insert_mmr_history(&mut tx, payload.account_id as i64, mmr).await?;
        res.mmr_updated = true;
    }

    for h in &payload.hero_stats {
        upsert_hero_current(&mut tx, payload.account_id as i64, h).await?;
        res.heroes_upserted += 1;
        if h.last_played.is_some() {
            insert_hero_snapshot(&mut tx, payload.account_id as i64, h).await?;
            res.hero_history_added += 1;
        }
    }

    tx.commit().await?;
    Ok(res)
}

#[derive(Debug, Default)]
pub struct MatchesIngestResult {
    pub matches_upserted: usize,
    pub match_players_upserted: usize,
    pub players_upserted: usize,
}

pub async fn ingest_matches_batch(pool: &PgPool, metas: &[MatchMeta]) -> Result<MatchesIngestResult> {
    let mut tx = pool.begin().await?;
    let mut out = MatchesIngestResult::default();
    for m in metas {
        let start_time = m.start_time.map(|s| ts_from_epoch_secs(s as i64));
        let info_json = m.info.clone().unwrap_or_else(|| serde_json::json!({}));
        sqlx::query(
            r#"
INSERT INTO matches (
  match_id, start_time, duration_s, winner_team, average_badge, region, patch_version, info_json
)
VALUES ($1,$2,$3,$4,$5,$6,$7, COALESCE($8::jsonb,'{}'::jsonb))
ON CONFLICT (match_id) DO UPDATE SET
  start_time = COALESCE(EXCLUDED.start_time, matches.start_time),
  duration_s = COALESCE(EXCLUDED.duration_s, matches.duration_s),
  winner_team = COALESCE(EXCLUDED.winner_team, matches.winner_team),
  average_badge = COALESCE(EXCLUDED.average_badge, matches.average_badge),
  region = COALESCE(EXCLUDED.region, matches.region),
  patch_version = COALESCE(EXCLUDED.patch_version, matches.patch_version),
  info_json = matches.info_json || EXCLUDED.info_json
            "#,
        )
        .bind(m.match_id)
        .bind(start_time)
        .bind(m.duration_s)
        .bind(&m.winner_team)
        .bind(m.average_badge)
        .bind(&m.region)
        .bind(&m.patch_version)
        .bind(info_json)
        .execute(&mut *tx)
        .await?;
        out.matches_upserted += 1;

        if let Some(players) = &m.players {
            for p in players {
                ensure_player_stub(&mut tx, p.account_id as i64).await?;
                upsert_match_player(&mut tx, m.match_id, p).await?;
                out.players_upserted += 1;
                out.match_players_upserted += 1;
            }
        }
    }
    tx.commit().await?;
    Ok(out)
}

async fn ensure_player_stub(tx: &mut Transaction<'_, Postgres>, account_id: i64) -> Result<()> {
    let steamid64 = crate::steam::account_id_to_steamid64(account_id as u32);
    sqlx::query(
        r#"
INSERT INTO players (account_id, steamid64)
VALUES ($1, $2)
ON CONFLICT (account_id) DO NOTHING
        "#,
    )
    .bind(account_id)
    .bind(steamid64)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn upsert_match_player(tx: &mut Transaction<'_, Postgres>, match_id: i64, p: &PlayerInMatch) -> Result<()> {
    let extra_json = p.extra.clone().unwrap_or_else(|| serde_json::json!({}));
    sqlx::query(
        r#"
INSERT INTO match_players (
  match_id, account_id, hero_id, team, party_id, lane, is_victory,
  kills, deaths, assists, networth, damage, damage_taken, obj_damage, last_hits,
  accuracy, crit_shot_rate, extra_json
) VALUES (
  $1,$2,$3,$4,$5,$6,$7,
  $8,$9,$10,$11,$12,$13,$14,$15,
  $16,$17, COALESCE($18::jsonb,'{}'::jsonb)
)
ON CONFLICT (match_id, account_id) DO UPDATE SET
  hero_id = EXCLUDED.hero_id,
  team = EXCLUDED.team,
  party_id = EXCLUDED.party_id,
  lane = EXCLUDED.lane,
  is_victory = EXCLUDED.is_victory,
  kills = EXCLUDED.kills,
  deaths = EXCLUDED.deaths,
  assists = EXCLUDED.assists,
  networth = EXCLUDED.networth,
  damage = EXCLUDED.damage,
  damage_taken = EXCLUDED.damage_taken,
  obj_damage = EXCLUDED.obj_damage,
  last_hits = EXCLUDED.last_hits,
  accuracy = EXCLUDED.accuracy,
  crit_shot_rate = EXCLUDED.crit_shot_rate,
  extra_json = match_players.extra_json || EXCLUDED.extra_json
        "#,
    )
    .bind(match_id)
    .bind(p.account_id as i64)
    .bind(p.hero_id)
    .bind(&p.team)
    .bind(p.party_id)
    .bind(&p.lane)
    .bind(p.is_victory)
    .bind(p.kills)
    .bind(p.deaths)
    .bind(p.assists)
    .bind(p.networth)
    .bind(p.damage)
    .bind(p.damage_taken)
    .bind(p.obj_damage)
    .bind(p.last_hits)
    .bind(p.accuracy)
    .bind(p.crit_shot_rate)
    .bind(extra_json)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn upsert_player(tx: &mut Transaction<'_, Postgres>, account_id: i64, steamid64: &str, p: &SteamProfile) -> Result<()> {
    let updated_at = p
        .last_updated
        .as_ref()
        .and_then(|s| s.parse::<i64>().ok())
        .map(ts_from_epoch_secs);

    let extra: Value = serde_json::json!({});
    sqlx::query!(
        r#"
INSERT INTO players (
  account_id, steamid64, personaname, profileurl, avatar, avatarmedium, avatarfull,
  countrycode, realname, profile_extra, profile_updated_at
) VALUES (
  $1,$2,$3,$4,$5,$6,$7,$8,$9, COALESCE($10::jsonb, '{}'::jsonb), $11
)
ON CONFLICT (account_id) DO UPDATE SET
  steamid64 = EXCLUDED.steamid64,
  personaname = EXCLUDED.personaname,
  profileurl = EXCLUDED.profileurl,
  avatar = EXCLUDED.avatar,
  avatarmedium = EXCLUDED.avatarmedium,
  avatarfull = EXCLUDED.avatarfull,
  countrycode = EXCLUDED.countrycode,
  realname = EXCLUDED.realname,
  profile_extra = players.profile_extra || EXCLUDED.profile_extra,
  profile_updated_at = GREATEST(players.profile_updated_at, EXCLUDED.profile_updated_at);
        "#,
        account_id,
        steamid64,
        p.personaname,
        p.profileurl,
        p.avatar,
        p.avatarmedium,
        p.avatarfull,
        p.countrycode,
        p.realname,
        extra,
        updated_at
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn upsert_latest_mmr(tx: &mut Transaction<'_, Postgres>, account_id: i64, m: &MMRHistory) -> Result<()> {
    let start_time = ts_from_epoch_secs(m.start_time as i64);
    let extra: Value = serde_json::json!({});
    sqlx::query!(
        r#"
INSERT INTO latest_mmr (
  account_id, match_id, start_time, player_score, rank, division, division_tier, extra
) VALUES ($1,$2,$3,$4,$5,$6,$7, COALESCE($8::jsonb,'{}'::jsonb))
ON CONFLICT (account_id) DO UPDATE SET
  match_id = EXCLUDED.match_id,
  start_time = EXCLUDED.start_time,
  player_score = EXCLUDED.player_score,
  rank = EXCLUDED.rank,
  division = EXCLUDED.division,
  division_tier = EXCLUDED.division_tier,
  extra = latest_mmr.extra || EXCLUDED.extra;
        "#,
        account_id,
        m.match_id,
        start_time,
        m.player_score,
        m.rank,
        m.division,
        m.division_tier,
        extra
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_mmr_history(tx: &mut Transaction<'_, Postgres>, account_id: i64, m: &MMRHistory) -> Result<()> {
    let start_time = ts_from_epoch_secs(m.start_time as i64);
    let extra: Value = serde_json::json!({});
    sqlx::query!(
        r#"
INSERT INTO mmr_history (
  account_id, start_time, match_id, player_score, rank, division, division_tier, extra
) VALUES ($1,$2,$3,$4,$5,$6,$7, COALESCE($8::jsonb,'{}'::jsonb))
ON CONFLICT (account_id, start_time) DO NOTHING;
        "#,
        account_id,
        start_time,
        m.match_id,
        m.player_score,
        m.rank,
        m.division,
        m.division_tier,
        extra
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn upsert_hero_current(tx: &mut Transaction<'_, Postgres>, account_id: i64, h: &HeroStats) -> Result<()> {
    let last_played = h.last_played.map(|s| ts_from_epoch_secs(s as i64));
    let extra: Value = serde_json::json!({});
    sqlx::query!(
        // language=PostgreSQL
        r#"
INSERT INTO hero_stats_current (
  account_id, hero_id, matches_played, wins, last_played, time_played, ending_level,
  kills, deaths, assists,
  kills_per_min, deaths_per_min, assists_per_min,
  networth_per_min, last_hits_per_min, damage_per_min, damage_taken_per_min,
  obj_damage_per_min, accuracy, crit_shot_rate, extra
) VALUES (
  $1,$2,$3,$4,$5,$6,$7,
  $8,$9,$10,
  $11,$12,$13,
  $14,$15,$16,$17,
  $18,$19,$20, COALESCE($21::jsonb,'{}'::jsonb)
)
ON CONFLICT (account_id, hero_id) DO UPDATE SET
  matches_played = EXCLUDED.matches_played,
  wins = EXCLUDED.wins,
  last_played = EXCLUDED.last_played,
  time_played = EXCLUDED.time_played,
  ending_level = EXCLUDED.ending_level,
  kills = EXCLUDED.kills,
  deaths = EXCLUDED.deaths,
  assists = EXCLUDED.assists,
  kills_per_min = EXCLUDED.kills_per_min,
  deaths_per_min = EXCLUDED.deaths_per_min,
  assists_per_min = EXCLUDED.assists_per_min,
  networth_per_min = EXCLUDED.networth_per_min,
  last_hits_per_min = EXCLUDED.last_hits_per_min,
  damage_per_min = EXCLUDED.damage_per_min,
  damage_taken_per_min = EXCLUDED.damage_taken_per_min,
  obj_damage_per_min = EXCLUDED.obj_damage_per_min,
  accuracy = EXCLUDED.accuracy,
  crit_shot_rate = EXCLUDED.crit_shot_rate,
  extra = hero_stats_current.extra || EXCLUDED.extra;
        "#,
        account_id,
        h.hero_id,
        h.matches_played.map(|v| v as i32),
        h.wins.map(|v| v as i32),
        last_played,
        h.time_played.map(|v| v as i32),
        h.ending_level,
        h.kills.map(|v| v as i32),
        h.deaths.map(|v| v as i32),
        h.assists.map(|v| v as i32),
        h.kills_per_min,
        h.deaths_per_min,
        h.assists_per_min,
        h.networth_per_min,
        h.last_hits_per_min,
        h.damage_per_min,
        h.damage_taken_per_min,
        h.obj_damage_per_min,
        h.accuracy,
        h.crit_shot_rate,
        extra
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_hero_snapshot(tx: &mut Transaction<'_, Postgres>, account_id: i64, h: &HeroStats) -> Result<()> {
    if let Some(last_played) = h.last_played.map(|s| ts_from_epoch_secs(s as i64)) {
        let snapshot = serde_json::to_value(h)?;
        sqlx::query!(
            // language=PostgreSQL
            r#"
INSERT INTO hero_stats_history (
  account_id, hero_id, last_played, snapshot_json
) VALUES ($1,$2,$3, $4::jsonb)
ON CONFLICT (account_id, hero_id, last_played) DO NOTHING;
            "#,
            account_id,
            h.hero_id,
            last_played,
            snapshot
        )
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

pub fn ts_from_epoch_secs<T: Into<i64>>(secs: T) -> DateTime<Utc> {
    let s = secs.into();
    let s = if s < 0 { 0 } else { s } as i64;
    Utc
        .timestamp_opt(s, 0)
        .single()
        .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().unwrap())
}
