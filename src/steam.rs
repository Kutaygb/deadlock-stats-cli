use anyhow::Result;
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum SteamError {
    #[error("invalid SteamID64")] 
    InvalidSteamId64,
    #[error("invalid Steam community URL")] 
    InvalidCommunityUrl,
    #[error("STEAM_WEB_API_KEY is required to resolve vanity URLs")] 
    MissingSteamWebApiKey,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

const STEAMID64_MIN: u64 = 76561197960265728; // steamID64 offset

pub async fn to_steamid64_with_client(input: &str, http: &Client) -> Result<String, SteamError> {
    let input = input.trim();

    if is_steamid64(input) {
        validate_steamid64(input)?;
        return Ok(input.to_string());
    }

    // URL form
    if let Ok(url) = Url::parse(input) {
        if !host_is_steamcommunity(&url) {
            return Err(SteamError::InvalidCommunityUrl);
        }
        let mut segs = url
            .path_segments()
            .ok_or(SteamError::InvalidCommunityUrl)?
            .filter(|s| !s.is_empty());
        match (segs.next(), segs.next()) {
            (Some("profiles"), Some(id)) => {
                if !is_steamid64(id) {
                    return Err(SteamError::InvalidSteamId64);
                }
                validate_steamid64(id)?;
                return Ok(id.to_string());
            }
            (Some("id"), Some(name)) => {
                let key = std::env::var("STEAM_WEB_API_KEY").map_err(|_| SteamError::MissingSteamWebApiKey)?;
                let sid = resolve_vanity(name, &key, http).await?;
                validate_steamid64(&sid)?;
                return Ok(sid);
            }
            _ => return Err(SteamError::InvalidCommunityUrl),
        }
    }

    // vanity name (no slashes/URL)
    if input.contains('/') || input.contains(':') || input.starts_with("http") {
        return Err(SteamError::InvalidCommunityUrl);
    }
    let key = std::env::var("STEAM_WEB_API_KEY").map_err(|_| SteamError::MissingSteamWebApiKey)?;
    let sid = resolve_vanity(input, &key, http).await?;
    validate_steamid64(&sid)?;
    Ok(sid)
}

#[allow(dead_code)]
pub async fn to_steamid64(input: &str) -> Result<String, SteamError> {
    let http = reqwest::Client::builder()
        .user_agent("deadlock-cli/0.1")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| SteamError::Other(e.into()))?;
    to_steamid64_with_client(input, &http).await
}

fn host_is_steamcommunity(url: &Url) -> bool {
    match url.host_str() {
        Some(h) => h.eq_ignore_ascii_case("steamcommunity.com") || h.eq_ignore_ascii_case("www.steamcommunity.com"),
        None => false,
    }
}

pub fn validate_steamid64(id: &str) -> Result<(), SteamError> {
    if !is_steamid64(id) { return Err(SteamError::InvalidSteamId64); }
    let n: u64 = id.parse().map_err(|_| SteamError::InvalidSteamId64)?;
    if n < STEAMID64_MIN { return Err(SteamError::InvalidSteamId64); }
    Ok(())
}

pub fn steamid64_to_account_id(id: &str) -> Result<u32, SteamError> {
    validate_steamid64(id)?;
    let n: u64 = id.parse().map_err(|_| SteamError::InvalidSteamId64)?;
    let acc = n - STEAMID64_MIN;
    Ok(acc as u32)
}

/// convert a 32-bit account ID to SteamID64 string
pub fn account_id_to_steamid64(account_id: u32) -> String {
    (STEAMID64_MIN + account_id as u64).to_string()
}

/// parse SteamID3 like "[U:1:123456]", classic Steam2 like "STEAM_0:1:12345",
/// or a raw 32-bit account ID into an account_id.
pub fn parse_steamid3_or_account_id(input: &str) -> Result<u32, SteamError> {
    let s = input.trim();

    // [U:1:Z] (SteamID3)
    static ID3_RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(r"(?i)^\[[a-z]:\d+:(\d+)\]$").unwrap()
    });
    if let Some(c) = ID3_RE.captures(s) {
        let z = &c[1];
        let id32: u64 = z.parse().map_err(|_| SteamError::InvalidSteamId64)?;
        if id32 > u32::MAX as u64 { return Err(SteamError::InvalidSteamId64); }
        return Ok(id32 as u32);
    }

    // STEAM_X:Y:Z (Steam2)
    static STEAM2_RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(r"(?i)^STEAM_\d+:(\d+):(\d+)$").unwrap()
    });
    if let Some(c) = STEAM2_RE.captures(s) {
        let y: u64 = c[1].parse().map_err(|_| SteamError::InvalidSteamId64)?;
        let z: u64 = c[2].parse().map_err(|_| SteamError::InvalidSteamId64)?;
        if y > 1 { return Err(SteamError::InvalidSteamId64); }
        let acc = z.checked_mul(2).and_then(|v| v.checked_add(y)).ok_or(SteamError::InvalidSteamId64)?;
        if acc > u32::MAX as u64 { return Err(SteamError::InvalidSteamId64); }
        return Ok(acc as u32);
    }

    // raw digits -> account_id
    if s.chars().all(|ch| ch.is_ascii_digit()) {
        let n: u64 = s.parse().map_err(|_| SteamError::InvalidSteamId64)?;
        if n <= u32::MAX as u64 {
            return Ok(n as u32);
        }
    }

    Err(SteamError::InvalidSteamId64)
}

fn is_steamid64(s: &str) -> bool {
    static RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| Regex::new(r"^\d{17}$").unwrap());
    RE.is_match(s)
}

#[derive(Debug, Deserialize)]
struct VanityResponseWrap { response: VanityResponse }

#[derive(Debug, Deserialize)]
struct VanityResponse {
    success: i32,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    steamid: Option<String>,
}

async fn resolve_vanity(vanity: impl AsRef<str>, key: &str, http: &Client) -> Result<String, SteamError> {
    let vanity = vanity.as_ref();
    let base = std::env::var("STEAM_WEB_API_BASE").unwrap_or_else(|_| "https://api.steampowered.com".to_string());
    let endpoint = format!("{}/ISteamUser/ResolveVanityURL/v1/", base.trim_end_matches('/'));
    let url = reqwest::Url::parse_with_params(
        &endpoint,
        &[("key", key), ("vanityurl", vanity)],
    ).map_err(|e| SteamError::Other(e.into()))?;

    let resp = http.get(url).send().await.map_err(|e| SteamError::Other(e.into()))?;
    if !resp.status().is_success() {
        return Err(SteamError::Other(anyhow::anyhow!("Steam vanity resolve failed: {}", resp.status())));
    }
    let wrap: VanityResponseWrap = resp.json().await.map_err(|e| SteamError::Other(e.into()))?;
    match wrap.response.success {
        1 => Ok(wrap.response.steamid.unwrap()),
        _ => Err(SteamError::Other(anyhow::anyhow!(wrap.response.message.unwrap_or_else(|| "Vanity not found".to_string())))),
    }
}
