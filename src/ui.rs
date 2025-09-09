use crate::models::{HeroStats, MMRHistory, SteamProfile};
use comfy_table::{presets::UTF8_FULL, Table};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CombinedPayload {
    pub steamid64: String,
    pub account_id: u32,
    pub profile: SteamProfile,
    pub latest_mmr: Option<MMRHistory>,
    pub hero_stats: Vec<HeroStats>,
}

pub fn latest_mmr_for(all: &[MMRHistory], account_id: u32) -> Option<MMRHistory> {
    all.iter()
        .filter(|m| m.account_id as u32 == account_id)
        .cloned()
        .max_by_key(|m| (m.start_time, m.match_id))
}

pub fn print_profile_table(profile: &SteamProfile, steamid64: &str, account_id: u32, mmr: Option<&MMRHistory>) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec!["Profile", "Value"]);

    table.add_row(vec!["Name", &profile.personaname]);
    table.add_row(vec!["SteamID64", steamid64]);
    table.add_row(vec!["Account ID (SteamID3)", &account_id.to_string()]);
    if let Some(c) = &profile.countrycode { table.add_row(vec!["Country", c]); }
    table.add_row(vec!["Profile URL", &profile.profileurl]);
    if let Some(m) = mmr {
        table.add_row(vec!["Rank", &format!("{} (div {}-{})", m.rank, m.division, m.division_tier)]);
    }

    println!("\n== Profile ==\n{}
", table);
}

pub fn print_stats_table(hero_stats: &[HeroStats]) {
    let total_matches: i64 = hero_stats.iter().filter_map(|h| h.matches_played).sum();
    let total_wins: i64 = hero_stats.iter().filter_map(|h| h.wins).sum();
    let winrate = if total_matches > 0 { (total_wins as f64) / (total_matches as f64) * 100.0 } else { 0.0 };

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec!["Stat", "Value"]);
    table.add_row(vec!["Total Matches", &total_matches.to_string()]);
    table.add_row(vec!["Wins", &total_wins.to_string()]);
    table.add_row(vec!["Win Rate", &format!("{:.2}%", winrate)]);

    println!("\n== Stats ==\n{}
", table);
}

pub fn print_detailed_hero_stats(hero_stats: &[HeroStats]) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec!["Hero ID", "Matches", "Wins", "KPM", "DPM", "APM"]);

    for h in hero_stats {
        table.add_row(vec![
            h.hero_id.to_string(),
            h.matches_played.unwrap_or_default().to_string(),
            h.wins.unwrap_or_default().to_string(),
            fmt_opt_f(h.kills_per_min),
            fmt_opt_f(h.deaths_per_min),
            fmt_opt_f(h.assists_per_min),
        ]);
    }
    println!("\n== Detailed Hero Stats ==\n{}
", table);
}

fn fmt_opt_f(v: Option<f64>) -> String { v.map(|x| format!("{:.2}", x)).unwrap_or_else(|| "-".into()) }

