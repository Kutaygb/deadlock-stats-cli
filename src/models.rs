use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamProfile {
    pub account_id: i32,
    pub personaname: String,
    pub profileurl: String,
    pub avatar: String,
    pub avatarmedium: String,
    pub avatarfull: String,
    pub countrycode: Option<String>,
    pub realname: Option<String>,
    #[serde(default, deserialize_with = "opt_string_from_string_or_int")]
    pub last_updated: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MMRHistory {
    pub account_id: i32,
    pub match_id: i64,
    pub start_time: i32,
    pub player_score: f64,
    pub rank: i32,
    pub division: i32,
    pub division_tier: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HeroStats {
    pub account_id: i32,
    pub hero_id: i32,
    pub matches_played: Option<i64>,
    pub wins: Option<i64>,
    pub last_played: Option<i64>,
    pub time_played: Option<i64>,
    pub ending_level: Option<f64>,
    pub kills: Option<i64>,
    pub deaths: Option<i64>,
    pub assists: Option<i64>,
    pub kills_per_min: Option<f64>,
    pub deaths_per_min: Option<f64>,
    pub assists_per_min: Option<f64>,
    pub networth_per_min: Option<f64>,
    pub last_hits_per_min: Option<f64>,
    pub damage_per_min: Option<f64>,
    pub damage_taken_per_min: Option<f64>,
    pub obj_damage_per_min: Option<f64>,
    pub accuracy: Option<f64>,
    pub crit_shot_rate: Option<f64>,
    // many more fields exist; we only map the ones we display
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlayerMatchHistoryEntry {
    pub account_id: i32,
    pub match_id: i64,
    pub hero_id: i32,
    pub hero_level: i32,
    pub start_time: i32,
    pub game_mode: i32,
    pub match_mode: i32,
    pub player_team: i32,
    pub player_kills: i32,
    pub player_deaths: i32,
    pub player_assists: i32,
    pub denies: i32,
    pub net_worth: i32,
    pub last_hits: i32,
    pub match_duration_s: i32,
    pub match_result: i32,
    pub objectives_mask_team0: i32,
    pub objectives_mask_team1: i32,
}

// ============ Matches Metadata (bulk) ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchMeta {
    pub match_id: i64,
    /// Unix seconds
    #[serde(default)]
    pub start_time: Option<i64>,
    #[serde(default)]
    pub duration_s: Option<i32>,
    #[serde(default)]
    pub winner_team: Option<String>,
    #[serde(default)]
    pub average_badge: Option<i32>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub patch_version: Option<String>,
    /// Additional info if included
    #[serde(default)]
    pub info: Option<Value>,
    /// Players if included
    #[serde(default)]
    pub players: Option<Vec<PlayerInMatch>>, 
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlayerInMatch {
    pub account_id: i32,
    #[serde(default)]
    pub hero_id: Option<i32>,
    #[serde(default)]
    pub team: Option<String>,
    #[serde(default)]
    pub party_id: Option<i64>,
    #[serde(default)]
    pub lane: Option<String>,
    #[serde(default)]
    pub is_victory: Option<bool>,
    #[serde(default)]
    pub kills: Option<i32>,
    #[serde(default)]
    pub deaths: Option<i32>,
    #[serde(default)]
    pub assists: Option<i32>,
    #[serde(default)]
    pub networth: Option<i64>,
    #[serde(default)]
    pub damage: Option<i64>,
    #[serde(default)]
    pub damage_taken: Option<i64>,
    #[serde(default)]
    pub obj_damage: Option<i64>,
    #[serde(default)]
    pub last_hits: Option<i32>,
    #[serde(default)]
    pub accuracy: Option<f64>,
    #[serde(default)]
    pub crit_shot_rate: Option<f64>,
    #[serde(default)]
    pub extra: Option<Value>,
}

// Accept either a string or an integer and convert to Some(String)
fn opt_string_from_string_or_int<'de, D>(de: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StrOrInt {
        S(String),
        I(i64),
    }
    let v = Option::<StrOrInt>::deserialize(de)?;
    Ok(v.map(|x| match x { StrOrInt::S(s) => s, StrOrInt::I(i) => i.to_string() }))
}
