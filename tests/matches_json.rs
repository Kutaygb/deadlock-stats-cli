use deadlock_cli::models::{MatchMeta, PlayerInMatch};

#[test]
fn decode_match_meta_with_players() {
    let json = serde_json::json!({
        "match_id": 1234567890i64,
        "start_time": 1_700_000_000i64,
        "duration_s": 1800,
        "winner_team": "team1",
        "average_badge": 50,
        "region": "eu",
        "patch_version": "1.2.3",
        "info": { "foo": "bar" },
        "players": [
            {
                "account_id": 388674065,
                "hero_id": 1,
                "team": "team1",
                "party_id": 42,
                "lane": "mid",
                "is_victory": true,
                "kills": 10,
                "deaths": 2,
                "assists": 8,
                "networth": 123456,
                "damage": 50000,
                "damage_taken": 25000,
                "obj_damage": 3000,
                "last_hits": 200,
                "accuracy": 0.55,
                "crit_shot_rate": 0.12,
                "extra": {"x": 1}
            }
        ]
    });

    let m: MatchMeta = serde_json::from_value(json).expect("decode");
    assert_eq!(m.match_id, 1234567890i64);
    assert_eq!(m.duration_s.unwrap(), 1800);
    assert_eq!(m.players.as_ref().unwrap().len(), 1);
    let p: &PlayerInMatch = &m.players.as_ref().unwrap()[0];
    assert_eq!(p.account_id, 388674065);
    assert_eq!(p.kills.unwrap(), 10);
}

