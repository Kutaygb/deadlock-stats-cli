#![cfg(feature = "db")]

use deadlock_cli::{db, models};

// This test requires a running PostgreSQL at DATABASE_URL.
// It is marked ignored by default. Run with: cargo test --features db -- --ignored
#[tokio::test]
#[ignore]
async fn persist_one_match() {
    let db::DbPool(pool) = db::connect().await.unwrap();
    db::migrate(&pool).await.unwrap();

    let meta = models::MatchMeta {
        match_id: 9876543210,
        start_time: Some(1_700_000_000),
        duration_s: Some(1200),
        winner_team: Some("team2".into()),
        average_badge: Some(35),
        region: Some("na".into()),
        patch_version: Some("1.2".into()),
        info: None,
        players: Some(vec![models::PlayerInMatch {
            account_id: 388674065,
            hero_id: Some(1),
            team: Some("team2".into()),
            party_id: Some(1),
            lane: Some("mid".into()),
            is_victory: Some(false),
            kills: Some(5),
            deaths: Some(7),
            assists: Some(9),
            networth: Some(22222),
            damage: Some(10000),
            damage_taken: Some(15000),
            obj_damage: Some(3000),
            last_hits: Some(100),
            accuracy: Some(0.3),
            crit_shot_rate: Some(0.05),
            extra: None,
        }]),
    };

    let res = db::ingest_matches_batch(&pool, &[meta]).await.unwrap();
    assert_eq!(res.matches_upserted, 1);
    assert_eq!(res.match_players_upserted, 1);
}

