use deadlock_cli::steam;
use httpmock::prelude::*;

#[tokio::test]
async fn parses_raw_steamid64() {
    let id = "76561197960435530";
    steam::validate_steamid64(id).unwrap();
    let acc = steam::steamid64_to_account_id(id).unwrap();
    assert!(acc > 0);
}

#[tokio::test]
async fn extracts_from_profiles_url() {
    let url = "https://steamcommunity.com/profiles/76561197960435530";
    let http = reqwest::Client::new();
    let sid = steam::to_steamid64_with_client(url, &http).await.unwrap();
    assert_eq!(sid, "76561197960435530");
}

#[tokio::test]
async fn resolves_vanity_from_id_url() {
    let server = MockServer::start();
    // mock ResolveVanityURL response
    let _m = server.mock(|when, then| {
        when.method(GET)
            .path("/ISteamUser/ResolveVanityURL/v1/")
            .query_param("key", "TESTKEY")
            .query_param("vanityurl", "gabelogannewell");
        then.status(200)
            .json_body_obj(&serde_json::json!({
                "response": { "success": 1, "steamid": "76561197960287930" }
            }));
    });

    std::env::set_var("STEAM_WEB_API_BASE", server.base_url());
    std::env::set_var("STEAM_WEB_API_KEY", "TESTKEY");

    let http = reqwest::Client::new();
    let url = "https://steamcommunity.com/id/gabelogannewell/";
    let sid = steam::to_steamid64_with_client(url, &http).await.unwrap();
    assert_eq!(sid, "76561197960287930");
}

#[tokio::test]
async fn vanity_resolution_handles_failure() {
    let server = MockServer::start();
    let _m = server.mock(|when, then| {
        when.method(GET)
            .path("/ISteamUser/ResolveVanityURL/v1/")
            .query_param("key", "TESTKEY")
            .query_param("vanityurl", "nonexistent");
        then.status(200)
            .json_body_obj(&serde_json::json!({
                "response": { "success": 42, "message": "No match" }
            }));
    });

    std::env::set_var("STEAM_WEB_API_BASE", server.base_url());
    std::env::set_var("STEAM_WEB_API_KEY", "TESTKEY");
    let http = reqwest::Client::new();
    let url = "https://steamcommunity.com/id/nonexistent";
    let err = steam::to_steamid64_with_client(url, &http).await.err().unwrap();
    let msg = format!("{}", err);
    assert!(msg.contains("No match"));
}

#[tokio::test]
async fn rejects_invalid_url() {
    let http = reqwest::Client::new();
    let bad = "https://example.com/id/foo";
    let err = steam::to_steamid64_with_client(bad, &http).await.err().unwrap();
    let msg = format!("{}", err);
    assert!(msg.contains("invalid Steam community URL"));
}
