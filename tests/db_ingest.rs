#![cfg(feature = "db")]

use deadlock_cli::{db, models, ui};
use chrono::Utc;

// This test requires a running PostgreSQL at DATABASE_URL.
// It is marked ignored by default. Run with: cargo test -- --ignored
#[tokio::test]
#[ignore]
async fn ingest_roundtrip() {
    let db::DbPool(pool) = db::connect().await.unwrap();
    db::migrate(&pool).await.unwrap();

    let account_id: u32 = 388674065;
    let steamid64 = "76561198348939793".to_string();

    let profile = models::SteamProfile {
        account_id: account_id as i32,
        personaname: "tester".into(),
        profileurl: format!("https://steamcommunity.com/profiles/{steamid64}/"),
        avatar: "".into(),
        avatarmedium: "".into(),
        avatarfull: "".into(),
        countrycode: None,
        realname: None,
        last_updated: Some((Utc::now().timestamp() as i64).to_string()),
    };

    let latest_mmr = models::MMRHistory {
        account_id: account_id as i32,
        match_id: 123456,
        start_time: Utc::now().timestamp() as i32,
        player_score: 42.0,
        rank: 40,
        division: 4,
        division_tier: 2,
    };

    let hero = models::HeroStats {
        account_id: account_id as i32,
        hero_id: 1,
        matches_played: Some(10),
        wins: Some(6),
        last_played: Some(Utc::now().timestamp() as i64),
        time_played: Some(1000),
        ending_level: Some(29.5),
        kills: Some(20),
        deaths: Some(15),
        assists: Some(25),
        kills_per_min: Some(0.2),
        deaths_per_min: Some(0.15),
        assists_per_min: Some(0.25),
        networth_per_min: Some(1000.0),
        last_hits_per_min: Some(4.0),
        damage_per_min: Some(700.0),
        damage_taken_per_min: Some(800.0),
        obj_damage_per_min: Some(100.0),
        accuracy: Some(0.5),
        crit_shot_rate: Some(0.1),
    };

    let combined = ui::CombinedPayload {
        steamid64: steamid64.clone(),
        account_id,
        profile: profile.clone(),
        latest_mmr: Some(latest_mmr.clone()),
        hero_stats: vec![hero.clone()],
    };

    // First ingest
    let res1 = db::ingest_player(&pool, &combined).await.unwrap();
    assert!(res1.mmr_updated);
    assert_eq!(res1.heroes_upserted, 1);
    assert_eq!(res1.hero_history_added, 1);

    // Idempotent re-ingest
    let res2 = db::ingest_player(&pool, &combined).await.unwrap();
    assert!(res2.mmr_updated);
    assert_eq!(res2.heroes_upserted, 1);
    // history should not duplicate due to PK
    assert_eq!(res2.hero_history_added, 1);

    // Verify win_rate generated column
    let rec = sqlx::query!(
        r#"SELECT wins, matches_played, win_rate FROM hero_stats_current WHERE account_id=$1 AND hero_id=$2"#,
        account_id as i64,
        hero.hero_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rec.wins, Some(6));
    assert_eq!(rec.matches_played, Some(10));
    let expected = 6.0f64 / 10.0;
    assert!((rec.win_rate.unwrap() - expected).abs() < 1e-9);
}
