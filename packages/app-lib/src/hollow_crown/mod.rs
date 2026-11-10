//! Hollow Crown catalog, mod-list scan, and join-token handoff.
//!
//! The signing key stays on the server. This module asks for a token, stores
//! the server's bytes, and deletes them when the instance closes. It does not
//! sign anything.

use crate::state::ModLoader;
use crate::util::fetch::REQWEST_CLIENT;
use crate::{ErrorKind, launcher_user_agent};
use anyhow::{Context, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1_smol::Sha1;
use sha2::{Digest, Sha512};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_CATALOG_URL: &str =
    "https://launcher.thehollowcrownmc.com/api/v1/catalog";
const DEFAULT_LISTS_URL: &str =
    "https://launcher.thehollowcrownmc.com/api/v1/mod-lists";
/// Minecraft server Play joins. The token gate is a different port on this host.
const PLAY_ADDRESS: &str = "play.thehollowcrownmc.com";
const DEFAULT_TOKEN_URL: &str = "http://play.thehollowcrownmc.com:8765/v1/token";
const DEFAULT_TOKEN_CLOSE_URL: &str =
    "http://play.thehollowcrownmc.com:8765/v1/token/close";

const TOKEN_FILE: &str = "hollow-crown/join-token.json";
const SESSION_FILE: &str = "hollow-crown/session.json";
const REGISTRY_FILE: &str = "hollow-crown-sessions.json";
const MESSENGER_FILE_NAME: &str = "hollow-crown-messenger.jar";

/// Fabric and Quilt build of the messenger. It only reads the token file.
const FABRIC_MESSENGER: &[u8] =
    include_bytes!("../../assets/hollow-crown-messenger-fabric.jar");

#[derive(Debug, Clone)]
pub struct PlaySession {
    pub token_id: String,
    pub instance_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
struct Catalog {
    packs: Vec<CatalogPack>,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogPack {
    #[serde(default)]
    hosted_files: Vec<HostedFile>,
}

#[derive(Debug, Clone, Deserialize)]
struct HostedFile {
    #[serde(default)]
    sha512: Option<String>,
    url: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ModLists {
    #[serde(default)]
    whitelist: Vec<HashRule>,
    #[serde(default)]
    blacklist: Vec<HashRule>,
}

#[derive(Debug, Clone, Deserialize)]
struct HashRule {
    #[serde(default)]
    sha512: Option<String>,
    #[serde(default)]
    sha1: Option<String>,
    #[serde(default)]
    required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HashedFile {
    path: String,
    sha512: String,
    sha1: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AccountRegistry {
    #[serde(default)]
    accounts: HashMap<String, ActiveSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ActiveSession {
    token_id: String,
    token_path: PathBuf,
    session_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalSession {
    token_id: String,
    account_uuid: String,
}

pub fn ensure_windows() -> crate::Result<()> {
    if cfg!(windows) {
        return Ok(());
    }
    if std::env::var("HOLLOW_CROWN_ALLOW_NON_WINDOWS").is_ok() {
        tracing::warn!(
            "Hollow Crown Launcher v1 is Windows-only. Continuing because HOLLOW_CROWN_ALLOW_NON_WINDOWS is set."
        );
        return Ok(());
    }
    bail!(ErrorKind::LauncherError(
        "Hollow Crown Launcher v1 runs on Windows only.".into(),
    ));
}

pub async fn server_address_for_pack(
    _project_id: Option<&str>,
    _version_id: Option<&str>,
) -> crate::Result<String> {
    Ok(PLAY_ADDRESS.to_string())
}

pub async fn hosted_mirrors(sha512: &str) -> Vec<String> {
    match fetch_catalog().await {
        Ok(catalog) => hosted_url(&catalog, sha512).into_iter().collect(),
        Err(error) => {
            tracing::warn!(
                "Hollow Crown catalog was not loaded, so downloads stay on the Modrinth CDN: {error}"
            );
            Vec::new()
        }
    }
}

pub async fn catalog_value() -> crate::Result<Value> {
    let bytes = http_get(&catalog_url()).await?;
    let value: Value = serde_json::from_slice(&bytes).with_context(|| {
        "Hollow Crown catalog was not valid JSON".to_string()
    })?;
    let catalog: Catalog = serde_json::from_value(value.clone())
        .context("Hollow Crown catalog is missing packs")?;
    if catalog.packs.is_empty() {
        bail!(ErrorKind::LauncherError(
            "Hollow Crown catalog has no packs.".into(),
        ));
    }
    Ok(value)
}

pub async fn begin_session(
    instance_dir: &Path,
    loader: ModLoader,
    account_uuid: &str,
    username: &str,
    minecraft_access_token: &str,
    instance_id: &str,
    settings_dir: &Path,
) -> crate::Result<PlaySession> {
    install_messenger(instance_dir, loader)?;
    let lists = fetch_lists().await?;
    let files = hash_instance(instance_dir).await?;
    decide_scan(&files, &lists).map_err(|message| {
        crate::Error::from(ErrorKind::LauncherError(message))
    })?;

    let token_bytes = mint_token(
        account_uuid,
        username,
        minecraft_access_token,
        instance_id,
    )
    .await?;
    let summary = validate_token_bytes(&token_bytes).map_err(|message| {
        crate::Error::from(ErrorKind::LauncherError(message))
    })?;

    let token_path = instance_dir.join(TOKEN_FILE);
    let session_path = instance_dir.join(SESSION_FILE);
    if let Some(parent) = token_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("creating {}", parent.display())
        })?;
    }

    let previous = with_registry(settings_dir, |registry| {
        registry.accounts.remove(account_uuid)
    })?;
    if let Some(previous) = previous {
        let _ = revoke_token(&previous.token_id, account_uuid).await;
        let _ = std::fs::remove_file(&previous.token_path);
        let _ = std::fs::remove_file(&previous.session_path);
    }

    std::fs::write(&token_path, &token_bytes).with_context(|| {
        format!("writing {}", token_path.display())
    })?;
    let local = LocalSession {
        token_id: summary.token_id.clone(),
        account_uuid: account_uuid.to_string(),
    };
    std::fs::write(
        &session_path,
        serde_json::to_vec_pretty(&local).context("encoding local session")?,
    )
    .with_context(|| format!("writing {}", session_path.display()))?;

    with_registry(settings_dir, |registry| {
        registry.accounts.insert(
            account_uuid.to_string(),
            ActiveSession {
                token_id: summary.token_id.clone(),
                token_path: token_path.clone(),
                session_path: session_path.clone(),
            },
        );
    })?;

    Ok(PlaySession {
        token_id: summary.token_id,
        instance_dir: instance_dir.to_path_buf(),
    })
}

pub async fn end_session(session: &PlaySession) {
    end_instance(&session.instance_dir).await;
}

pub async fn end_instance(instance_dir: &Path) {
    let session_path = instance_dir.join(SESSION_FILE);
    let token_path = instance_dir.join(TOKEN_FILE);
    let local = match std::fs::read(&session_path) {
        Ok(bytes) => serde_json::from_slice::<LocalSession>(&bytes).ok(),
        Err(_) => None,
    };
    if let Some(local) = local.as_ref() {
        if let Err(error) =
            revoke_token(&local.token_id, &local.account_uuid).await
        {
            tracing::warn!(
                "Could not revoke Hollow Crown join token {}: {error}",
                local.token_id
            );
        }
    }
    let _ = std::fs::remove_file(&token_path);
    let _ = std::fs::remove_file(&session_path);
}

fn install_messenger(instance_dir: &Path, loader: ModLoader) -> crate::Result<()> {
    let bytes = FABRIC_MESSENGER;
    if bytes.is_empty() {
        bail!(ErrorKind::LauncherError(
            "The Hollow Crown messenger jar is missing from this build.".into(),
        ));
    }
    let mods = instance_dir.join("mods");
    std::fs::create_dir_all(&mods)
        .with_context(|| format!("creating {}", mods.display()))?;
    let destination = mods.join(MESSENGER_FILE_NAME);
    std::fs::write(&destination, bytes).with_context(|| {
        format!("installing {}", destination.display())
    })?;
    if !matches!(loader, ModLoader::Fabric | ModLoader::Quilt) {
        bail!(ErrorKind::LauncherError(
            "The Hollow Crown messenger is installed, but this pack's loader cannot run the Fabric jar.".into(),
        ));
    }
    Ok(())
}

async fn hash_instance(instance_dir: &Path) -> crate::Result<Vec<HashedFile>> {
    let mut files = Vec::new();
    for directory in ["mods", "resourcepacks", "shaderpacks"] {
        let dir = instance_dir.join(directory);
        if !dir.is_dir() {
            continue;
        }
        let mut entries = std::fs::read_dir(&dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if !path.is_file() || !is_archive(&path) {
                continue;
            }
            let bytes = std::fs::read(&path).with_context(|| {
                format!("reading {}", path.display())
            })?;
            let relative = format!(
                "{directory}/{}",
                path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
            );
            files.push(HashedFile {
                path: relative,
                sha512: hex_encode(&Sha512::digest(&bytes)),
                sha1: hex_encode(&Sha1::from(&bytes).digest().bytes()),
            });
        }
    }
    Ok(files)
}

fn is_archive(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("jar" | "zip" | "litemod")
    )
}

fn decide_scan(files: &[HashedFile], lists: &ModLists) -> Result<(), String> {
    let mut allowed = Vec::new();
    for file in files {
        if lists.blacklist.iter().any(|rule| hash_match(file, rule)) {
            return Err(format!(
                "Play stopped. {} is on the Hollow Crown blacklist.",
                file.path
            ));
        }
        if lists.whitelist.iter().any(|rule| hash_match(file, rule)) {
            allowed.push(file);
            continue;
        }
        return Err(format!(
            "Play stopped. {} is not on the Hollow Crown whitelist.",
            file.path
        ));
    }
    for rule in &lists.whitelist {
        if !rule.required {
            continue;
        }
        let present = allowed.iter().any(|file| hash_match(file, rule));
        if !present {
            return Err(
                "Play stopped. A required Hollow Crown file is missing."
                    .to_string(),
            );
        }
    }
    Ok(())
}

fn hash_match(file: &HashedFile, rule: &HashRule) -> bool {
    let sha512_ok = rule
        .sha512
        .as_deref()
        .map(|hash| hashes_equal(hash, &file.sha512));
    let sha1_ok = rule
        .sha1
        .as_deref()
        .map(|hash| hashes_equal(hash, &file.sha1));
    match (sha512_ok, sha1_ok) {
        (Some(sha512), Some(sha1)) => sha512 && sha1,
        (Some(sha512), None) => sha512,
        (None, Some(sha1)) => sha1,
        (None, None) => false,
    }
}

fn hashes_equal(left: &str, right: &str) -> bool {
    !left.trim().is_empty()
        && left.trim().eq_ignore_ascii_case(right.trim())
}

fn hosted_url(catalog: &Catalog, sha512: &str) -> Option<String> {
    catalog.packs.iter().find_map(|pack| {
        pack.hosted_files.iter().find_map(|file| {
            let matches = file
                .sha512
                .as_deref()
                .is_some_and(|hash| hashes_equal(hash, sha512));
            matches.then(|| file.url.clone())
        })
    })
}

#[derive(Debug)]
struct TokenSummary {
    token_id: String,
}

fn validate_token_bytes(bytes: &[u8]) -> Result<TokenSummary, String> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| "the join token was not valid JSON".to_string())?;
    let object = value
        .as_object()
        .ok_or_else(|| "the join token was not a JSON object".to_string())?;
    let token_id = object
        .get("token_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "the join token has no token id".to_string())?
        .to_string();
    let signature = object
        .get("signature")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "the join token has no signature. This launcher does not sign tokens."
                .to_string()
        })?;
    let _ = signature;
    let expiry = object
        .get("expiry")
        .ok_or_else(|| "the join token has no expiry".to_string())?;
    let expiry = parse_expiry(expiry)?;
    if expiry <= Utc::now() {
        return Err("the join token is already expired".to_string());
    }
    Ok(TokenSummary { token_id })
}

fn parse_expiry(value: &Value) -> Result<DateTime<Utc>, String> {
    if let Some(text) = value.as_str() {
        return DateTime::parse_from_rfc3339(text)
            .map(|date| date.with_timezone(&Utc))
            .map_err(|_| "the join token expiry is not a time".to_string());
    }
    if let Some(number) = value.as_i64() {
        let seconds = if number > 1_000_000_000_000 {
            number / 1000
        } else {
            number
        };
        return DateTime::from_timestamp(seconds, 0)
            .ok_or_else(|| "the join token expiry is not a time".to_string());
    }
    Err("the join token expiry is not a time".to_string())
}

async fn mint_token(
    account_uuid: &str,
    username: &str,
    minecraft_access_token: &str,
    instance_id: &str,
) -> crate::Result<Vec<u8>> {
    let body = serde_json::json!({
        "account_uuid": account_uuid,
        "username": username,
        "minecraft_access_token": minecraft_access_token,
        "instance_id": instance_id,
        "supersede_previous": true,
    });
    let bytes = http_post(&token_url(), &body).await?;
    Ok(bytes)
}

async fn revoke_token(
    token_id: &str,
    account_uuid: &str,
) -> crate::Result<()> {
    let url = token_close_url();
    let body = serde_json::json!({
        "token_id": token_id,
        "account_uuid": account_uuid,
    });
    let _ = http_post(&url, &body).await?;
    Ok(())
}

fn with_registry<T>(
    settings_dir: &Path,
    update: impl FnOnce(&mut AccountRegistry) -> T,
) -> crate::Result<T> {
    std::fs::create_dir_all(settings_dir).with_context(|| {
        format!("creating {}", settings_dir.display())
    })?;
    let path = settings_dir.join(REGISTRY_FILE);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    file.lock()
        .context("locking Hollow Crown session registry")?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let mut registry = if bytes.is_empty() {
        AccountRegistry::default()
    } else {
        serde_json::from_slice(&bytes).unwrap_or_default()
    };
    let result = update(&mut registry);
    let encoded = serde_json::to_vec_pretty(&registry)
        .context("encoding Hollow Crown session registry")?;
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&encoded)?;
    file.unlock()
        .context("unlocking Hollow Crown session registry")?;
    Ok(result)
}

async fn fetch_catalog() -> crate::Result<Catalog> {
    let bytes = http_get(&catalog_url()).await?;
    serde_json::from_slice(&bytes).context("reading the Hollow Crown catalog")
}

async fn fetch_lists() -> crate::Result<ModLists> {
    let bytes = http_get(&lists_url()).await?;
    serde_json::from_slice(&bytes)
        .context("reading the Hollow Crown mod lists")
}

async fn http_get(url: &str) -> crate::Result<Vec<u8>> {
    let response = REQWEST_CLIENT
        .get(url)
        .header("User-Agent", launcher_user_agent())
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .with_context(|| format!("requesting {url}"))?;
    let status = response.status();
    let bytes = response.bytes().await.context("reading response")?;
    if !status.is_success() {
        bail!(ErrorKind::LauncherError(format!(
            "Hollow Crown request to {url} failed ({status}): {}",
            truncate(&bytes)
        )));
    }
    Ok(bytes.to_vec())
}

async fn http_post(url: &str, body: &Value) -> crate::Result<Vec<u8>> {
    let response = REQWEST_CLIENT
        .post(url)
        .header("User-Agent", launcher_user_agent())
        .timeout(Duration::from_secs(20))
        .json(body)
        .send()
        .await
        .with_context(|| format!("requesting {url}"))?;
    let status = response.status();
    let bytes = response.bytes().await.context("reading response")?;
    if !status.is_success() {
        bail!(ErrorKind::LauncherError(format!(
            "Hollow Crown request to {url} failed ({status}): {}",
            truncate(&bytes)
        )));
    }
    Ok(bytes.to_vec())
}

fn truncate(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.chars().take(240).collect()
}

fn catalog_url() -> String {
    std::env::var("HOLLOW_CROWN_CATALOG_URL")
        .unwrap_or_else(|_| DEFAULT_CATALOG_URL.to_string())
}

fn lists_url() -> String {
    std::env::var("HOLLOW_CROWN_LISTS_URL")
        .unwrap_or_else(|_| DEFAULT_LISTS_URL.to_string())
}

fn token_url() -> String {
    std::env::var("HOLLOW_CROWN_TOKEN_URL")
        .unwrap_or_else(|_| DEFAULT_TOKEN_URL.to_string())
}

fn token_close_url() -> String {
    std::env::var("HOLLOW_CROWN_TOKEN_CLOSE_URL")
        .unwrap_or_else(|_| DEFAULT_TOKEN_CLOSE_URL.to_string())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(sha512: &str, sha1: &str) -> HashedFile {
        HashedFile {
            path: "mods/example.jar".into(),
            sha512: sha512.into(),
            sha1: sha1.into(),
        }
    }

    fn rule(sha512: Option<&str>, sha1: Option<&str>, required: bool) -> HashRule {
        HashRule {
            sha512: sha512.map(str::to_string),
            sha1: sha1.map(str::to_string),
            required,
        }
    }

    #[test]
    fn blacklist_wins_when_a_hash_is_on_both_lists() {
        let files = vec![file("abc", "def")];
        let lists = ModLists {
            whitelist: vec![rule(Some("abc"), None, true)],
            blacklist: vec![rule(Some("ABC"), None, false)],
        };
        let error = decide_scan(&files, &lists).unwrap_err();
        assert!(error.contains("blacklist"), "{error}");
    }

    #[test]
    fn unknown_jar_is_refused() {
        let files = vec![file("abc", "def")];
        let lists = ModLists {
            whitelist: vec![rule(Some("other"), None, false)],
            blacklist: vec![],
        };
        let error = decide_scan(&files, &lists).unwrap_err();
        assert!(error.contains("whitelist"), "{error}");
    }

    #[test]
    fn required_file_must_be_present() {
        let lists = ModLists {
            whitelist: vec![rule(Some("abc"), None, true)],
            blacklist: vec![],
        };
        let error = decide_scan(&[], &lists).unwrap_err();
        assert!(error.contains("required"), "{error}");
    }

    #[test]
    fn matching_whitelist_hash_is_allowed() {
        let files = vec![file("abc", "def")];
        let lists = ModLists {
            whitelist: vec![rule(None, Some("def"), true)],
            blacklist: vec![],
        };
        decide_scan(&files, &lists).unwrap();
    }

    #[test]
    fn token_without_a_signature_is_refused() {
        let error = validate_token_bytes(
            br#"{"token_id":"one","expiry":"2099-01-01T00:00:00Z"}"#,
        )
        .unwrap_err();
        assert!(error.contains("signature"), "{error}");
    }

    #[test]
    fn expired_token_is_refused() {
        let error = validate_token_bytes(
            br#"{"token_id":"one","expiry":"2000-01-01T00:00:00Z","signature":"sig"}"#,
        )
        .unwrap_err();
        assert!(error.contains("expired"), "{error}");
    }

    #[test]
    fn hosted_flag_replaces_the_download_url() {
        let catalog = Catalog {
            packs: vec![CatalogPack {
                hosted_files: vec![HostedFile {
                    sha512: Some("abc".into()),
                    url: "https://launcher.thehollowcrownmc.com/files/mod.jar"
                        .into(),
                }],
            }],
        };
        assert_eq!(
            hosted_url(&catalog, "ABC").as_deref(),
            Some("https://launcher.thehollowcrownmc.com/files/mod.jar")
        );
        assert_eq!(hosted_url(&catalog, "nope"), None);
    }

    #[tokio::test]
    async fn play_connects_to_the_paper_network() {
        let address = server_address_for_pack(Some("any-pack"), Some("any-version"))
            .await
            .unwrap();
        assert_eq!(address, "play.thehollowcrownmc.com");
    }

    #[test]
    fn newer_session_drops_the_previous_token_file() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings");
        let old_token = dir.path().join("old-token.json");
        let old_session = dir.path().join("old-session.json");
        std::fs::write(&old_token, b"old-token").unwrap();
        std::fs::write(&old_session, b"old-session").unwrap();
        with_registry(&settings, |registry| {
            registry.accounts.insert(
                "account".into(),
                ActiveSession {
                    token_id: "old".into(),
                    token_path: old_token.clone(),
                    session_path: old_session.clone(),
                },
            );
        })
        .unwrap();
        let previous = with_registry(&settings, |registry| {
            registry.accounts.remove("account")
        })
        .unwrap()
        .unwrap();
        std::fs::remove_file(&previous.token_path).unwrap();
        std::fs::remove_file(&previous.session_path).unwrap();
        assert!(!old_token.exists());
        assert!(!old_session.exists());
        assert_eq!(previous.token_id, "old");
    }
}
