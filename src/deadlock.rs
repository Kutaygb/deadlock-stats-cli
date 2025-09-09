use crate::models::{HeroStats, MMRHistory, MatchMeta, PlayerMatchHistoryEntry, SteamProfile};
use anyhow::Result;
use reqwest::{header, Client, StatusCode, Url};
use serde::de::DeserializeOwned;
use std::time::{Duration, SystemTime};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DeadlockError {
    #[error("HTTP {status}: {message}")]
    Http { status: StatusCode, message: String },

    #[error("Rate limited: {0}")]
    RateLimited(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Clone)]
pub struct DeadlockClient {
    base: Url,
    api_key: Option<String>,
    http: Client,
}

impl DeadlockClient {
    pub fn new(base: impl AsRef<str>, api_key: Option<String>, http: Client) -> Self {
        let base = Url::parse(base.as_ref()).expect("Invalid DEADLOCK_API_BASE");
        Self { base, api_key, http }
    }

    pub async fn get_steam_profiles(&self, account_ids: &[u32]) -> Result<Vec<SteamProfile>, DeadlockError> {
        let url = self.base.join("/v1/players/steam").unwrap();
        let ids = join_ids(account_ids);
        self.get_json(url, vec![("account_ids", ids)]).await
    }

    pub async fn get_mmr(&self, account_ids: &[u32]) -> Result<Vec<MMRHistory>, DeadlockError> {
        let url = self.base.join("/v1/players/mmr").unwrap();
        let ids = join_ids(account_ids);
        self.get_json(url, vec![("account_ids", ids)]).await
    }

    pub async fn get_player_hero_stats(&self, account_ids: &[u32]) -> Result<Vec<HeroStats>, DeadlockError> {
        let url = self.base.join("/v1/players/hero-stats").unwrap();
        let ids = join_ids(account_ids);
        self.get_json(url, vec![("account_ids", ids)]).await
    }

    pub async fn get_matches_metadata(
        &self,
        match_ids: &[i64],
        include_info: bool,
        include_players: bool,
    ) -> Result<Vec<MatchMeta>, DeadlockError> {
        let url = self.base.join("/v1/matches/metadata").unwrap();
        let ids = match_ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        let mut q = vec![("match_ids", ids)];
        if include_info { q.push(("include_info", "true".into())); }
        if include_players { q.push(("include_players", "true".into())); }
        self.get_json(url, q).await
    }

    pub async fn get_player_match_history(
        &self,
        account_id: u32,
        force_refetch: bool,
        only_stored_history: bool,
    ) -> Result<Vec<PlayerMatchHistoryEntry>, DeadlockError> {
        let url = self.base.join(&format!("/v1/players/{}/match-history", account_id)).unwrap();
        let mut q: Vec<(&str, String)> = Vec::new();
        if force_refetch { q.push(("force_refetch", "true".into())); }
        if only_stored_history { q.push(("only_stored_history", "true".into())); }
        self.get_json(url, q).await
    }

    async fn get_json<T: DeserializeOwned>(&self, url: Url, query: Vec<(&str, String)>) -> Result<T, DeadlockError> {
        let mut last_err: Option<DeadlockError> = None;
        let mut delay = Duration::from_millis(400);
        for attempt in 0..4 {
            let mut req = self.http.get(url.clone()).query(&query);
            if let Some(key) = &self.api_key {
                req = req.header("X-API-KEY", key);
            }
            let resp = req.send().await;
            match resp {
                Ok(rsp) => {
                    let status = rsp.status();
                    let headers = rsp.headers().clone();

                    if status == StatusCode::TOO_MANY_REQUESTS {
                        let msg = rsp.text().await.unwrap_or_default();

                        if let Some(wait_dur) = headers
                            .get(header::RETRY_AFTER)
                            .and_then(|v| v.to_str().ok())
                            .and_then(parse_retry_after)
                        {
                            tokio::time::sleep(wait_dur).await;
                        } else if attempt < 3 {
                            tokio::time::sleep(delay).await;
                            delay = delay.saturating_mul(2);
                        }
                        last_err = Some(DeadlockError::RateLimited(msg));
                        continue;
                    }

                    if !status.is_success() {
                        let msg = rsp.text().await.unwrap_or_default();
                        return Err(DeadlockError::Http { status, message: msg });
                    }

                    let out = rsp
                        .json::<T>()
                        .await
                        .map_err(|e| DeadlockError::Other(e.into()))?;
                    return Ok(out);
                }
                Err(e) => {
                    last_err = Some(DeadlockError::Other(e.into()));
                    if attempt < 3 {
                        tokio::time::sleep(delay).await;
                        delay = delay.saturating_mul(2);
                        continue;
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| DeadlockError::Other(anyhow::anyhow!("HTTP failed"))))
    }
}

fn join_ids(ids: &[u32]) -> String {
    ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")
}

fn parse_retry_after(s: &str) -> Option<Duration> {
    if let Ok(n) = s.parse::<u64>() {
        return Some(Duration::from_secs(n));
    }
    
    if let Ok(when) = httpdate::parse_http_date(s) {
        let now = SystemTime::now();
        if let Ok(wait) = when.duration_since(now) {
            return Some(wait);
        }
    }
    None
}
