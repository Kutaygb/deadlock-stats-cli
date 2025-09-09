mod cli;
#[cfg(feature = "db")]
mod db;
mod deadlock;
mod models;
mod steam;
mod ui;

use anyhow::{bail, Context, Result};
use clap::Parser;
use deadlock::DeadlockClient;
use sqlx::Row;
use std::io::{self, Write};
use tokio::runtime::Runtime;

fn main() {

    let _ = dotenvy::dotenv();

    let rt = Runtime::new().expect("failed to create tokio runtime");
    let res = rt.block_on(async_main());
    if let Err(err) = res {
        // map rate limiting 
        if let Some(code) = err.downcast_ref::<deadlock::DeadlockError>() {
            if matches!(code, deadlock::DeadlockError::RateLimited { .. }) {
                eprintln!("Rate limit hit. Please try again later.");
                std::process::exit(29);
            }
        }
        eprintln!("Error: {:#}", err);
        std::process::exit(1);
    }
}

async fn async_main() -> Result<()> {
    use cli::{Args, Command};

    let args = Args::parse();

    //build http clients
    let http = reqwest::Client::builder()
        .user_agent("deadlock-cli/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let base = std::env::var("DEADLOCK_API_BASE").unwrap_or_else(|_| "https://api.deadlock-api.com".to_string());
    let api_key = std::env::var("DEADLOCK_API_KEY").ok();
    let dl = DeadlockClient::new(base, api_key, http.clone());

    let mut want_json = args.json;

    if let Some(Command::Migrate) = args.command {
        #[cfg(feature = "db")]
        {
            let db::DbPool(pool) = db::connect().await?;
            db::migrate(&pool).await?;
            println!("Migrations completed.");
            return Ok(());
        }
        #[cfg(not(feature = "db"))]
        {
            anyhow::bail!("DB feature not enabled. Rebuild with `--features db`.");
        }
    }

    // Matches sync
    if let Some(Command::Matches { cmd }) = args.command.clone() {
        match cmd {
            cli::MatchesSubcommand::Sync { ids, from_account_id, from_steamid, from_id3, since_id, until_id, limit, batch_size, include_info, include_players, dry_run } => {
                #[cfg(feature = "db")]
                {
                    let db::DbPool(pool) = db::connect().await?;
                    db::migrate(&pool).await?;

                    //candidate match ids
                    let mut candidate_ids: Vec<i64> = Vec::new();

                    if !ids.is_empty() {
                        candidate_ids.extend(ids);
                    }

                    //derive from player sources -> use mmr history match_ids
                    let account_id_opt: Option<u32> = if let Some(acc) = from_account_id {
                        Some(acc)
                    } else if let Some(sid) = from_steamid {
                        let http2 = http.clone();
                        let sid64 = steam::to_steamid64_with_client(&sid, &http2).await?;
                        Some(steam::steamid64_to_account_id(&sid64)?)
                    } else if let Some(id3) = from_id3 {
                        Some(steam::parse_steamid3_or_account_id(&id3)?)
                    } else {
                        None
                    };

                    if let Some(account_id) = account_id_opt {
                        let ids_slice = &[account_id];
                        match dl.get_mmr(ids_slice).await {
                            Ok(mmr) => {
                                for m in mmr {
                                    candidate_ids.push(m.match_id);
                                }
                            }
                            Err(e) => {
                                eprintln!("Warning: failed to fetch MMR for account {}: {}", account_id, e);
                            }
                        }
                    }

                    //fallback sequential window if still empty
                    if candidate_ids.is_empty() {
                        let start_from: i64 = if let Some(s) = since_id {
                            s
                        } else {
                            let row = sqlx::query(r#"SELECT COALESCE(MAX(match_id), 0) AS max FROM matches"#)
                                .fetch_one(&pool)
                                .await
                                .ok();
                            row.as_ref().map(|r| r.get::<i64, _>("max")).unwrap_or(0)
                        };

                        let until_cap = until_id.unwrap_or(i64::MAX);
                        let mut next = start_from + 1;
                        while candidate_ids.len() < limit && next <= until_cap {
                            candidate_ids.push(next);
                            next += 1;
                        }
                        if candidate_ids.is_empty() {
                            println!("No IDs to fetch (start_from={}, until_id={:?})", start_from, until_id);
                            return Ok(());
                        }
                    }

                    // normalize and limit
                    candidate_ids.sort_unstable();
                    candidate_ids.dedup();
                    if candidate_ids.len() > limit { candidate_ids.truncate(limit); }

                    // dedupe matches
                    let rows = sqlx::query(r#"SELECT match_id FROM matches WHERE match_id = ANY($1)"#)
                        .bind(&candidate_ids)
                        .fetch_all(&pool)
                        .await
                        .unwrap_or_default();
                    let existing: std::collections::HashSet<i64> = rows.into_iter().map(|r| r.get::<i64, _>("match_id")).collect();
                    candidate_ids.retain(|id| !existing.contains(id));
                    if candidate_ids.is_empty() {
                        println!("All candidate IDs already present or none discovered. Nothing to do.");
                        return Ok(());
                    }

                    // chunk and ingest
                    let mut total_matches = 0usize;
                    let mut total_players = 0usize;
                    for chunk in candidate_ids.chunks(batch_size.max(1)) {
                        let metas = match dl.get_matches_metadata(chunk, include_info, include_players).await {
                            Ok(m) => m,
                            Err(deadlock::DeadlockError::Http { status, .. }) if status == reqwest::StatusCode::NOT_FOUND => {
                                eprintln!("chunk {:?} -> no matches found (404)", &chunk[..chunk.len().min(3)]);
                                Vec::new()
                            }
                            Err(e) => return Err(anyhow::Error::from(e)),
                        };
                        if dry_run {
                            println!("dry-run: fetched {} matches in chunk ({} IDs)", metas.len(), chunk.len());
                        } else {
                            let res = db::ingest_matches_batch(&pool, &metas).await?;
                            total_matches += res.matches_upserted;
                            total_players += res.match_players_upserted;
                            eprintln!(
                                "batch: matches_upserted={}, match_players_upserted={}, players_referenced={}",
                                res.matches_upserted, res.match_players_upserted, res.players_upserted
                            );
                        }
                    }
                    if !dry_run {
                        println!("Done. Total matches upserted={}, match_players upserted={}.", total_matches, total_players);
                    }
                    return Ok(());
                }
                #[cfg(not(feature = "db"))]
                {
                    anyhow::bail!("DB feature not enabled. Rebuild with `--features db`.");
                }
            }
            cli::MatchesSubcommand::History { account_id, steamid, id3, force_refetch, only_stored_history, dry_run } => {
                if force_refetch && only_stored_history {
                    anyhow::bail!("--force-refetch and --only-stored-history cannot be used together");
                }

                #[cfg(feature = "db")]
                {
                    let db::DbPool(pool) = db::connect().await?;
                    db::migrate(&pool).await?;

                    // resolve account_id
                    let acc: u32 = if let Some(a) = account_id {
                        a
                    } else if let Some(s) = steamid {
                        let sid64 = steam::to_steamid64_with_client(&s, &http).await?;
                        steam::steamid64_to_account_id(&sid64)?
                    } else if let Some(s) = id3 {
                        steam::parse_steamid3_or_account_id(&s)?
                    } else {
                        anyhow::bail!("Provide one of --account-id, --steamid, or --id3");
                    };

                    // fetch history
                    let entries = dl.get_player_match_history(acc, force_refetch, only_stored_history).await?;
                    if entries.is_empty() {
                        println!("No history entries returned for account {}", acc);
                        return Ok(());
                    }

                    // group by match_id into MatchMeta with PlayerInMatch list
                    use std::collections::BTreeMap;
                    let mut grouped: BTreeMap<i64, (Option<i64>, Option<i32>, Vec<crate::models::PlayerInMatch>)> = BTreeMap::new();
                    for e in entries {
                        let start = Some(e.start_time as i64);
                        let dur = Some(e.match_duration_s);
                        let ent = grouped.entry(e.match_id).or_insert((start, dur, Vec::new()));
                        if ent.0.is_none() { ent.0 = start; }
                        if ent.1.is_none() { ent.1 = dur; }
                        let extra = serde_json::json!({
                            "denies": e.denies,
                            "game_mode": e.game_mode,
                            "match_mode": e.match_mode,
                            "match_result": e.match_result,
                            "objectives_mask_team0": e.objectives_mask_team0,
                            "objectives_mask_team1": e.objectives_mask_team1,
                            "hero_level": e.hero_level
                        });
                        let pim = crate::models::PlayerInMatch {
                            account_id: e.account_id,
                            hero_id: Some(e.hero_id),
                            team: Some(format!("team{}", e.player_team)),
                            party_id: None,
                            lane: None,
                            is_victory: None,
                            kills: Some(e.player_kills),
                            deaths: Some(e.player_deaths),
                            assists: Some(e.player_assists),
                            networth: Some(e.net_worth as i64),
                            damage: None,
                            damage_taken: None,
                            obj_damage: None,
                            last_hits: Some(e.last_hits),
                            accuracy: None,
                            crit_shot_rate: None,
                            extra: Some(extra),
                        };
                        ent.2.push(pim);
                    }

                    let metas: Vec<crate::models::MatchMeta> = grouped.into_iter().map(|(mid, (st, du, players))| {
                        crate::models::MatchMeta {
                            match_id: mid,
                            start_time: st,
                            duration_s: du,
                            winner_team: None,
                            average_badge: None,
                            region: None,
                            patch_version: None,
                            info: None,
                            players: Some(players),
                        }
                    }).collect();

                    if dry_run {
                        println!("dry-run: would persist {} matches ({} participants)", metas.len(), metas.iter().map(|m| m.players.as_ref().map(|v| v.len()).unwrap_or(0)).sum::<usize>());
                        return Ok(());
                    }

                    let res = db::ingest_matches_batch(&pool, &metas).await?;
                    println!("History persisted. matches_upserted={}, match_players_upserted={}", res.matches_upserted, res.match_players_upserted);
                    return Ok(());
                }
                #[cfg(not(feature = "db"))]
                {
                    anyhow::bail!("DB feature not enabled. Rebuild with `--features db`.");
                }
            }
        }
    }

    let steamid64 = match args.command {
        Some(Command::BySteamId { id }) => id,
        Some(Command::BySteamId3 { id3 }) => {
            let acc = steam::parse_steamid3_or_account_id(&id3)?;
            steam::account_id_to_steamid64(acc)
        }
        Some(Command::ByVanity { name }) => {
            steam::to_steamid64_with_client(&name, &http).await?
        }
        Some(Command::ByUrl { url }) => {
            steam::to_steamid64_with_client(&url, &http).await?
        }
        Some(Command::Migrate) => unreachable!("handled above"),
        Some(Command::Matches { .. }) => unreachable!("handled above"),
        None => {

            loop {
                println!("Deadlock CLI");
                println!("1) Lookup by SteamID64");
                println!("2) Lookup by Steam Community ID (vanity name)");
                println!("3) Lookup by Steam Community URL");
                println!("4) Lookup by SteamID3 / Account ID");
                println!("j) Toggle JSON output [{}]", if want_json { "on" } else { "off" });
                println!("q) Quit");
                print!("> ");
                io::stdout().flush().ok();
                let mut line = String::new();
                io::stdin().read_line(&mut line).ok();
                let choice = line.trim();
                match choice {
                    "1" => {
                        let id = prompt("Enter SteamID64: ")?;
                        break id;
                    }
                    "2" => {
                        let name = prompt("Enter Steam Community ID (vanity name): ")?;
                        let sid = steam::to_steamid64_with_client(&name, &http).await?;
                        break sid;
                    }
                    "3" => {
                        let url = prompt("Enter full Steam Community URL: ")?;
                        let sid = steam::to_steamid64_with_client(&url, &http).await?;
                        break sid;
                    }
                    "4" => {
                        let id3 = prompt("Enter SteamID3 (e.g. [U:1:123456]) or Account ID: ")?;
                        let acc = steam::parse_steamid3_or_account_id(&id3)?;
                        let sid = steam::account_id_to_steamid64(acc);
                        break sid;
                    }
                    "j" | "J" => {
                        want_json = !want_json;
                        continue;
                    }
                    "q" | "Q" => return Ok(()),
                    _ => {
                        println!("Invalid choice. Please try again.\n");
                        continue;
                    }
                }
            }
        }
    };

    let steamid64 = steamid64.trim();
    steam::validate_steamid64(steamid64)?;
    let account_id = steam::steamid64_to_account_id(steamid64)?;

    let ids_vec = vec![account_id];
    let ids = ids_vec.as_slice();
    let (steam_profiles_res, mmr_res, hero_stats_res) = tokio::join!(
        dl.get_steam_profiles(ids),
        dl.get_mmr(ids),
        dl.get_player_hero_stats(ids)
    );

    // handle 404/empty profiles explicitly
    let steam_profile = match steam_profiles_res {
        Ok(mut v) if !v.is_empty() => v.remove(0),
        Ok(_) => bail!("Player not found (no Steam profile)."),
        Err(e) => match e {
            deadlock::DeadlockError::Http { status, .. } if status == reqwest::StatusCode::NOT_FOUND => {
                bail!("Player not found.")
            }
            other => return Err(anyhow::Error::from(other)),
        },
    };

    let latest_mmr = match mmr_res {
        Ok(v) => ui::latest_mmr_for(&v, account_id),
        Err(e) => {
            eprintln!("Warning: failed to fetch MMR: {}", e);
            None
        }
    };

    let hero_stats = match hero_stats_res {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Warning: failed to fetch hero stats: {}", e);
            Vec::new()
        }
    };

    #[cfg(feature = "db")]
    {
        let combined = ui::CombinedPayload {
            steamid64: steamid64.to_string(),
            account_id,
            profile: steam_profile.clone(),
            latest_mmr: latest_mmr.clone(),
            hero_stats: hero_stats.clone(),
        };
        let db::DbPool(pool) = db::connect().await?;
        db::migrate(&pool).await?; // ensure schema
        let res = db::ingest_player(&pool, &combined).await?;
        eprintln!(
            "Saved to DB: heroes_upserted={}, hero_history_added={}, mmr_updated={}",
            res.heroes_upserted, res.hero_history_added, res.mmr_updated
        );

        match dl.get_player_match_history(account_id, false, true).await {
            Ok(entries) if !entries.is_empty() => {
                use std::collections::BTreeMap;
                let mut grouped: BTreeMap<i64, (Option<i64>, Option<i32>, Vec<crate::models::PlayerInMatch>)> = BTreeMap::new();
                for e in entries {
                    let start = Some(e.start_time as i64);
                    let dur = Some(e.match_duration_s);
                    let ent = grouped.entry(e.match_id).or_insert((start, dur, Vec::new()));
                    if ent.0.is_none() { ent.0 = start; }
                    if ent.1.is_none() { ent.1 = dur; }
                    let extra = serde_json::json!({
                        "denies": e.denies,
                        "game_mode": e.game_mode,
                        "match_mode": e.match_mode,
                        "match_result": e.match_result,
                        "objectives_mask_team0": e.objectives_mask_team0,
                        "objectives_mask_team1": e.objectives_mask_team1,
                        "hero_level": e.hero_level
                    });
                    let pim = crate::models::PlayerInMatch {
                        account_id: e.account_id,
                        hero_id: Some(e.hero_id),
                        team: Some(format!("team{}", e.player_team)),
                        party_id: None,
                        lane: None,
                        is_victory: None,
                        kills: Some(e.player_kills),
                        deaths: Some(e.player_deaths),
                        assists: Some(e.player_assists),
                        networth: Some(e.net_worth as i64),
                        damage: None,
                        damage_taken: None,
                        obj_damage: None,
                        last_hits: Some(e.last_hits),
                        accuracy: None,
                        crit_shot_rate: None,
                        extra: Some(extra),
                    };
                    ent.2.push(pim);
                }
                let metas: Vec<crate::models::MatchMeta> = grouped.into_iter().map(|(mid, (st, du, players))| {
                    crate::models::MatchMeta {
                        match_id: mid,
                        start_time: st,
                        duration_s: du,
                        winner_team: None,
                        average_badge: None,
                        region: None,
                        patch_version: None,
                        info: None,
                        players: Some(players),
                    }
                }).collect();
                if !metas.is_empty() {
                    let mres = db::ingest_matches_batch(&pool, &metas).await?;
                    eprintln!(
                        "Saved match history: matches_upserted={}, match_players_upserted={}",
                        mres.matches_upserted, mres.match_players_upserted
                    );
                }
            }
            Ok(_) => {
                eprintln!("No stored match history for this player yet.");
            }
            Err(e) => {
                eprintln!("Warning: failed to fetch match history: {}", e);
            }
        }
    }

    if want_json {
        let payload = ui::CombinedPayload {
            steamid64: steamid64.to_string(),
            account_id,
            profile: steam_profile.clone(),
            latest_mmr: latest_mmr.clone(),
            hero_stats: hero_stats.clone(),
        };
        let s = serde_json::to_string_pretty(&payload)?;
        println!("{}", s);
        return Ok(());
    }

    ui::print_profile_table(&steam_profile, steamid64, account_id, latest_mmr.as_ref());
    ui::print_stats_table(&hero_stats);

    let show_details = confirm("Show detailed hero stats? [y/N] ")?;
    if show_details {
        ui::print_detailed_hero_stats(&hero_stats);
    }

    Ok(())
}

fn prompt(msg: &str) -> Result<String> {
    print!("{}", msg);
    io::stdout().flush().ok();
    let mut s = String::new();
    io::stdin().read_line(&mut s).context("failed to read input")?;
    Ok(s.trim().to_string())
}

fn confirm(msg: &str) -> Result<bool> {
    let s = prompt(msg)?;
    Ok(matches!(s.as_str(), "y" | "Y" | "yes" | "Yes"))
}
