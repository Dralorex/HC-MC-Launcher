//! Hollow Crown catalog, mod-list scan, and join-token handoff.
//!
//! The signing key stays on the server. This module asks for a token, stores
//! the server's bytes, and deletes them when the instance closes. It does not
//! sign anything.

use crate::api::pack::install_from::CreatePackLocation;
use crate::install::InstallPostInstallEdit;
use crate::state::{InstanceLink, ModLoader};
use crate::util::fetch::{
    INSECURE_NO_TIMEOUT_REQWEST_CLIENT, NO_TIMEOUT_REQWEST_CLIENT,
    REQWEST_CLIENT,
};
use crate::{ErrorKind, launcher_user_agent};
use anyhow::{Context, bail};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1_smol::Sha1;
use sha2::{Digest, Sha512};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const DEFAULT_CATALOG_URL: &str =
    "https://launcher.thehollowcrownmc.com/api/v1/catalog";
const DEFAULT_LISTS_URL: &str =
    "https://launcher.thehollowcrownmc.com/api/v1/mod-lists";
/// Minecraft server Play joins. The token gate is a different port on this host.
const PLAY_ADDRESS: &str = "play.thehollowcrownmc.com";
const DEFAULT_TOKEN_URL: &str =
    "http://play.thehollowcrownmc.com:8765/v1/token";
const DEFAULT_TOKEN_CLOSE_URL: &str =
    "http://play.thehollowcrownmc.com:8765/v1/token/close";

const TOKEN_FILE: &str = "hollow-crown/join-token.json";
const PACK_SOURCE_FILE: &str = "hollow-crown/pack-source.json";
const LAUNCHER_HOST: &str = "launcher.thehollowcrownmc.com";
const SESSION_FILE: &str = "hollow-crown/session.json";
const REGISTRY_FILE: &str = "hollow-crown-sessions.json";
const EMAILS_FILE: &str = "hollow-crown-microsoft-emails.json";
const OWNER_PREVIEW_EMAIL: &str = "dralorex@gmail.com";
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

#[derive(Debug, Clone, Deserialize, Default)]
struct CatalogPack {
    #[serde(default)]
    name: String,
    /// `.mrpack` on the launcher host, or a `/files/...` path. When this is
    /// set, install and repair use the file and do not ask Modrinth for a version.
    #[serde(default)]
    mrpack_url: Option<String>,
    #[serde(default)]
    modrinth_project_id: Option<String>,
    #[serde(default)]
    modrinth_version_id: Option<String>,
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
    let _catalog: Catalog = serde_json::from_value(value.clone())
        .context("Hollow Crown catalog is missing packs")?;
    Ok(value)
}

/// Installs one catalog pack from its `mrpack_url` through `install_mrpack`.
pub async fn install_named_pack(
    name: &str,
) -> crate::Result<crate::install::InstallJobSnapshot> {
    let catalog = fetch_catalog().await?;
    let named = catalog
        .packs
        .iter()
        .filter(|pack| pack.name == name)
        .collect::<Vec<_>>();
    if named.is_empty() {
        bail!(ErrorKind::LauncherError(format!(
            "No Hollow Crown catalog pack is named {name}."
        )));
    }
    let Some(raw) = named.iter().find_map(|pack| pack_mrpack(pack)) else {
        bail!(ErrorKind::LauncherError(format!(
            "Catalog pack {name} has no mrpack_url. It still installs from its Modrinth ids."
        )));
    };
    let raw = raw.to_string();
    let job = start_mrpack_install(None, &raw, Some(name.to_string())).await?;
    if let Some(instance_id) = job.instance_id.as_deref() {
        if let Err(error) = remember_pack_source(instance_id, &raw).await {
            tracing::warn!(
                "Could not remember the Hollow Crown pack source for {instance_id}: {error}"
            );
        }
    }
    Ok(job)
}

/// Repairs an instance from the catalog `.mrpack` when one applies.
///
/// `Ok(None)` means this instance still uses its Modrinth ids.
pub async fn repair_instance_from_catalog(
    instance_id: &str,
) -> crate::Result<Option<crate::install::InstallJobSnapshot>> {
    let state = crate::State::get().await?;
    let metadata = crate::state::instances::commands::get_instance_metadata(
        instance_id,
        &state.pool,
    )
    .await?
    .ok_or_else(|| {
        crate::Error::from(ErrorKind::InputError(
            "unknown instance".to_string(),
        ))
    })?;
    let stored = stored_mrpack_url(instance_id).await;
    let (project_id, version_id) = link_pack_ids(&metadata.link);
    let raw = match fetch_catalog().await {
        Ok(catalog) => select_mrpack_url(
            &catalog.packs,
            project_id.as_deref(),
            version_id.as_deref(),
            stored.as_deref(),
            &catalog_url(),
        ),
        Err(error) => {
            tracing::warn!(
                "Hollow Crown catalog was not loaded for pack repair: {error}"
            );
            stored
        }
    };
    let Some(raw) = raw else {
        return Ok(None);
    };
    let job = start_mrpack_install(Some(instance_id), &raw, None).await?;
    if let Err(error) = remember_pack_source(instance_id, &raw).await {
        tracing::warn!(
            "Could not remember the Hollow Crown pack source for {instance_id}: {error}"
        );
    }
    Ok(Some(job))
}

async fn start_mrpack_install(
    instance_id: Option<&str>,
    raw_url: &str,
    title: Option<String>,
) -> crate::Result<crate::install::InstallJobSnapshot> {
    let url =
        resolve_mrpack_url(raw_url, &catalog_url()).map_err(|message| {
            crate::Error::from(ErrorKind::LauncherError(message))
        })?;
    let path = download_mrpack(&url).await?;
    match instance_id {
        Some(instance_id) => {
            crate::install::install_pack_to_existing_instance(
                instance_id.to_string(),
                CreatePackLocation::FromFile { path },
                None,
            )
            .await
        }
        None => {
            crate::install::create_modpack_instance(
                CreatePackLocation::FromFile { path },
                Some(InstallPostInstallEdit {
                    name: title,
                    ..InstallPostInstallEdit::default()
                }),
            )
            .await
        }
    }
}

fn pack_mrpack(pack: &CatalogPack) -> Option<&str> {
    trimmed_text(pack.mrpack_url.as_deref())
}

fn trimmed_text(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn link_pack_ids(link: &InstanceLink) -> (Option<String>, Option<String>) {
    match link {
        InstanceLink::ModrinthModpack {
            project_id,
            version_id,
        } => (Some(project_id.clone()), Some(version_id.clone())),
        InstanceLink::ServerProjectModpack {
            content_project_id,
            content_version_id,
            ..
        } => (
            Some(content_project_id.clone()),
            Some(content_version_id.clone()),
        ),
        InstanceLink::ImportedModpack {
            project_id,
            version_id,
            ..
        } => (project_id.clone(), version_id.clone()),
        InstanceLink::SharedInstance {
            modpack_project_id,
            modpack_version_id,
        } => (modpack_project_id.clone(), modpack_version_id.clone()),
        InstanceLink::Unmanaged
        | InstanceLink::ServerProject { .. }
        | InstanceLink::ModrinthHosting { .. } => (None, None),
    }
}

/// Picks the catalog `mrpack_url` for an installed instance.
///
/// A stored URL or a matching Modrinth project id selects the file. Packs
/// with no `mrpack_url` stay on their Modrinth ids.
fn select_mrpack_url(
    packs: &[CatalogPack],
    project_id: Option<&str>,
    version_id: Option<&str>,
    stored_url: Option<&str>,
    catalog_url: &str,
) -> Option<String> {
    let stored = trimmed_text(stored_url);
    if let Some(stored) = stored
        && let Some(pack) = packs.iter().find(|pack| {
            pack_mrpack(pack)
                .is_some_and(|url| same_mrpack_ref(url, stored, catalog_url))
        })
    {
        return pack_mrpack(pack).map(str::to_string);
    }

    if let Some(project_id) = trimmed_text(project_id) {
        let candidates = packs
            .iter()
            .filter(|pack| {
                pack_mrpack(pack).is_some()
                    && pack
                        .modrinth_project_id
                        .as_deref()
                        .is_some_and(|id| id.eq_ignore_ascii_case(project_id))
            })
            .collect::<Vec<_>>();
        if let Some(version_id) = trimmed_text(version_id)
            && let Some(exact) = candidates.iter().find(|pack| {
                pack.modrinth_version_id
                    .as_deref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(version_id))
            })
        {
            return pack_mrpack(exact).map(str::to_string);
        }
        if candidates.len() == 1 {
            return pack_mrpack(candidates[0]).map(str::to_string);
        }
    }

    stored.map(str::to_string)
}

fn same_mrpack_ref(left: &str, right: &str, catalog_url: &str) -> bool {
    if left.trim() == right.trim() {
        return true;
    }
    match (
        resolve_mrpack_url(left, catalog_url),
        resolve_mrpack_url(right, catalog_url),
    ) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn resolve_mrpack_url(raw: &str, catalog_url: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("mrpack_url is empty.".to_string());
    }
    if raw.starts_with('/') {
        if !is_mrpack_files_path(raw) {
            return Err(
                "A relative mrpack_url must be a /files/ path ending in .mrpack."
                    .to_string(),
            );
        }
        let catalog = url::Url::parse(catalog_url).map_err(|_| {
            "The Hollow Crown catalog URL is invalid.".to_string()
        })?;
        let origin = origin_base(&catalog)?;
        return Ok(format!("{origin}{raw}"));
    }
    let parsed = url::Url::parse(raw).map_err(|_| {
        "mrpack_url must be a /files/... path or an https URL on launcher.thehollowcrownmc.com."
            .to_string()
    })?;
    validate_absolute_mrpack_url(&parsed, catalog_url)?;
    Ok(parsed.to_string())
}

fn validate_absolute_mrpack_url(
    parsed: &url::Url,
    catalog_url: &str,
) -> Result<(), String> {
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(
            "mrpack_url cannot include a username or password.".to_string()
        );
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(
            "mrpack_url cannot include a query or fragment.".to_string()
        );
    }
    if !is_mrpack_files_path(parsed.path()) {
        return Err(
            "mrpack_url must be a /files/ path ending in .mrpack.".to_string()
        );
    }
    if !is_allowed_mrpack_host(parsed, catalog_url) {
        return Err(
            "mrpack_url must be on https://launcher.thehollowcrownmc.com."
                .to_string(),
        );
    }
    Ok(())
}

fn is_mrpack_files_path(path: &str) -> bool {
    if path.contains('\\') || path.split('/').any(|part| part == "..") {
        return false;
    }
    let path = path.split(['?', '#']).next().unwrap_or(path);
    path.starts_with("/files/")
        && path.ends_with(".mrpack")
        && !path.split('/').skip(1).any(|part| part.is_empty())
}

fn is_allowed_mrpack_host(candidate: &url::Url, catalog_url: &str) -> bool {
    if host_is(candidate, LAUNCHER_HOST)
        && candidate.scheme() == "https"
        && candidate.port().is_none()
    {
        return true;
    }
    let Ok(catalog) = url::Url::parse(catalog_url) else {
        return false;
    };
    candidate.scheme() == catalog.scheme()
        && hosts_equal(candidate, &catalog)
        && candidate.port_or_known_default() == catalog.port_or_known_default()
}

fn host_is(url: &url::Url, host: &str) -> bool {
    url.host_str()
        .is_some_and(|value| value.eq_ignore_ascii_case(host))
}

fn hosts_equal(left: &url::Url, right: &url::Url) -> bool {
    match (left.host_str(), right.host_str()) {
        (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
        _ => false,
    }
}

fn origin_base(url: &url::Url) -> Result<String, String> {
    let host = url.host_str().ok_or_else(|| {
        "The Hollow Crown catalog URL has no host.".to_string()
    })?;
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    Ok(format!("{}://{host}{port}", url.scheme()))
}

async fn download_mrpack(url: &str) -> crate::Result<PathBuf> {
    let client = if url.starts_with("https://") {
        &*NO_TIMEOUT_REQWEST_CLIENT
    } else {
        &*INSECURE_NO_TIMEOUT_REQWEST_CLIENT
    };
    let response = client
        .get(url)
        .header("User-Agent", launcher_user_agent())
        .timeout(Duration::from_secs(60 * 60))
        .send()
        .await
        .with_context(|| format!("downloading {url}"))?;
    let final_url = response.url().clone();
    let status = response.status();
    if !status.is_success() {
        let bytes = response.bytes().await.unwrap_or_default();
        bail!(ErrorKind::LauncherError(format!(
            "Hollow Crown pack download {url} failed ({status}): {}",
            truncate(&bytes)
        )));
    }
    if let Err(message) =
        validate_absolute_mrpack_url(&final_url, &catalog_url())
    {
        bail!(ErrorKind::LauncherError(message));
    }
    let path = std::env::temp_dir()
        .join(format!("hollow-crown-{}.mrpack", uuid::Uuid::new_v4()));
    let mut file = tokio::fs::File::create(&path)
        .await
        .with_context(|| format!("creating {}", path.display()))?;
    let mut stream = response.bytes_stream();
    let mut header = Vec::new();
    let mut checked = false;
    let mut size = 0_u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("reading {url}"))?;
        if !checked {
            header.extend_from_slice(&chunk);
            if header.len() < 2 {
                continue;
            }
            if &header[..2] != b"PK" {
                drop(file);
                let _ = tokio::fs::remove_file(&path).await;
                bail!(ErrorKind::LauncherError(format!(
                    "The file at {url} is not an .mrpack archive."
                )));
            }
            file.write_all(&header)
                .await
                .with_context(|| format!("writing {}", path.display()))?;
            size += header.len() as u64;
            checked = true;
            continue;
        }
        size += chunk.len() as u64;
        file.write_all(&chunk)
            .await
            .with_context(|| format!("writing {}", path.display()))?;
    }
    if !checked || size < 2 {
        drop(file);
        let _ = tokio::fs::remove_file(&path).await;
        bail!(ErrorKind::LauncherError(format!(
            "The file at {url} is empty."
        )));
    }
    file.flush()
        .await
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

async fn stored_mrpack_url(instance_id: &str) -> Option<String> {
    let dir = crate::api::instance::get_full_path(instance_id)
        .await
        .ok()?;
    let bytes = std::fs::read(dir.join(PACK_SOURCE_FILE)).ok()?;
    let source = serde_json::from_slice::<PackSource>(&bytes).ok()?;
    trimmed_text(Some(&source.mrpack_url)).map(str::to_string)
}

async fn remember_pack_source(
    instance_id: &str,
    mrpack_url: &str,
) -> crate::Result<()> {
    let dir = crate::api::instance::get_full_path(instance_id).await?;
    let path = dir.join(PACK_SOURCE_FILE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let encoded = serde_json::to_vec_pretty(&PackSource {
        mrpack_url: mrpack_url.to_string(),
    })
    .context("encoding Hollow Crown pack source")?;
    std::fs::write(&path, encoded)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct PackSource {
    mrpack_url: String,
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
    // Hash and scan for the messenger payload. Allow/deny stays in HC-Platform.
    log_scan(&files, &lists);

    let token_bytes =
        mint_token(account_uuid, username, minecraft_access_token, instance_id)
            .await?;
    let summary = validate_token_bytes(&token_bytes).map_err(|message| {
        crate::Error::from(ErrorKind::LauncherError(message))
    })?;

    let token_path = instance_dir.join(TOKEN_FILE);
    let session_path = instance_dir.join(SESSION_FILE);
    if let Some(parent) = token_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    let previous = with_registry(settings_dir, |registry| {
        registry.accounts.remove(account_uuid)
    })?;
    if let Some(previous) = previous {
        let _ = revoke_token(&previous.token_id, account_uuid).await;
        let _ = std::fs::remove_file(&previous.token_path);
        let _ = std::fs::remove_file(&previous.session_path);
    }

    std::fs::write(&token_path, &token_bytes)
        .with_context(|| format!("writing {}", token_path.display()))?;
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

fn install_messenger(
    instance_dir: &Path,
    loader: ModLoader,
) -> crate::Result<()> {
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
    std::fs::write(&destination, bytes)
        .with_context(|| format!("installing {}", destination.display()))?;
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
            let bytes = std::fs::read(&path)
                .with_context(|| format!("reading {}", path.display()))?;
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum ScanNote {
    Blacklisted(String),
    Unknown(String),
    MissingRequired,
}

fn scan_notes(files: &[HashedFile], lists: &ModLists) -> Vec<ScanNote> {
    let mut notes = Vec::new();
    let mut allowed = Vec::new();
    for file in files {
        if lists.blacklist.iter().any(|rule| hash_match(file, rule)) {
            notes.push(ScanNote::Blacklisted(file.path.clone()));
            continue;
        }
        if lists.whitelist.iter().any(|rule| hash_match(file, rule)) {
            allowed.push(file);
            continue;
        }
        notes.push(ScanNote::Unknown(file.path.clone()));
    }
    for rule in &lists.whitelist {
        if !rule.required {
            continue;
        }
        let present = allowed.iter().any(|file| hash_match(file, rule));
        if !present {
            notes.push(ScanNote::MissingRequired);
        }
    }
    notes
}

fn log_scan(files: &[HashedFile], lists: &ModLists) {
    for note in scan_notes(files, lists) {
        match note {
            ScanNote::Blacklisted(path) => tracing::warn!(
                "Hollow Crown scan: {path} is on the blacklist. The plugin decides allow/deny."
            ),
            ScanNote::Unknown(path) => tracing::warn!(
                "Hollow Crown scan: {path} is not on the whitelist. The plugin decides allow/deny."
            ),
            ScanNote::MissingRequired => tracing::warn!(
                "Hollow Crown scan: a required whitelist file is missing. The plugin decides allow/deny."
            ),
        }
    }
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
    !left.trim().is_empty() && left.trim().eq_ignore_ascii_case(right.trim())
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

async fn revoke_token(token_id: &str, account_uuid: &str) -> crate::Result<()> {
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
    std::fs::create_dir_all(settings_dir)
        .with_context(|| format!("creating {}", settings_dir.display()))?;
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
    serde_json::from_slice(&bytes).context("reading the Hollow Crown mod lists")
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct MicrosoftEmails {
    #[serde(default)]
    accounts: HashMap<String, String>,
}

/// Remembers the Microsoft account email for owner-preview checks.
pub fn remember_microsoft_email(account_uuid: &str, email: &str) {
    let Ok(email) = normalize_email(email) else {
        return;
    };
    let Some(settings_dir) = settings_dir() else {
        return;
    };
    let _ = with_emails(&settings_dir, |emails| {
        emails
            .accounts
            .insert(account_uuid.to_string(), email.clone());
    });
}

/// True when the signed-in Microsoft account is the Hollow Crown owner,
/// or when `HOLLOW_CROWN_OWNER_PREVIEW=1` is set for local testing.
pub async fn owner_preview_enabled() -> bool {
    if std::env::var("HOLLOW_CROWN_OWNER_PREVIEW")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return true;
    }
    let Some(settings_dir) = settings_dir() else {
        return false;
    };
    let emails = read_emails(&settings_dir);
    emails.accounts.values().any(|email| {
        normalize_email(email)
            .map(|value| value == OWNER_PREVIEW_EMAIL)
            .unwrap_or(false)
    })
}

pub fn email_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let decoded = decode_jwt_segment(payload)?;
    let value: Value = serde_json::from_slice(&decoded).ok()?;
    value
        .get("email")
        .and_then(Value::as_str)
        .and_then(|email| normalize_email(email).ok())
}

fn normalize_email(email: &str) -> Result<String, ()> {
    let email = email.trim().to_ascii_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(());
    }
    Ok(email)
}

fn decode_jwt_segment(segment: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    use base64::prelude::{BASE64_URL_SAFE, BASE64_URL_SAFE_NO_PAD};
    BASE64_URL_SAFE_NO_PAD
        .decode(segment)
        .ok()
        .or_else(|| BASE64_URL_SAFE.decode(segment).ok())
}

fn settings_dir() -> Option<PathBuf> {
    if let Some(state) = crate::State::get_if_initialized() {
        return Some(state.directories.settings_dir.clone());
    }
    crate::state::DirectoryInfo::initial_settings_dir_path(
        "com.hollowcrown.launcher",
    )
}

fn read_emails(settings_dir: &Path) -> MicrosoftEmails {
    let path = settings_dir.join(EMAILS_FILE);
    match std::fs::read(&path) {
        Ok(bytes) if !bytes.is_empty() => {
            serde_json::from_slice(&bytes).unwrap_or_default()
        }
        _ => MicrosoftEmails::default(),
    }
}

fn with_emails<T>(
    settings_dir: &Path,
    update: impl FnOnce(&mut MicrosoftEmails) -> T,
) -> crate::Result<T> {
    std::fs::create_dir_all(settings_dir)
        .with_context(|| format!("creating {}", settings_dir.display()))?;
    let path = settings_dir.join(EMAILS_FILE);
    let mut emails = read_emails(settings_dir);
    let result = update(&mut emails);
    let encoded = serde_json::to_vec_pretty(&emails)
        .context("encoding Hollow Crown Microsoft emails")?;
    std::fs::write(&path, encoded)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(result)
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

    fn rule(
        sha512: Option<&str>,
        sha1: Option<&str>,
        required: bool,
    ) -> HashRule {
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
        let notes = scan_notes(&files, &lists);
        assert!(
            notes
                .iter()
                .any(|note| matches!(note, ScanNote::Blacklisted(_))),
            "{notes:?}"
        );
    }

    #[test]
    fn unknown_jar_is_noted_without_blocking_play() {
        let files = vec![file("abc", "def")];
        let lists = ModLists {
            whitelist: vec![rule(Some("other"), None, false)],
            blacklist: vec![],
        };
        let notes = scan_notes(&files, &lists);
        assert!(
            notes
                .iter()
                .any(|note| matches!(note, ScanNote::Unknown(_))),
            "{notes:?}"
        );
    }

    #[test]
    fn required_file_absence_is_noted_without_blocking_play() {
        let lists = ModLists {
            whitelist: vec![rule(Some("abc"), None, true)],
            blacklist: vec![],
        };
        let notes = scan_notes(&[], &lists);
        assert!(
            notes
                .iter()
                .any(|note| matches!(note, ScanNote::MissingRequired)),
            "{notes:?}"
        );
    }

    #[test]
    fn matching_whitelist_hash_has_no_scan_notes() {
        let files = vec![file("abc", "def")];
        let lists = ModLists {
            whitelist: vec![rule(None, Some("def"), true)],
            blacklist: vec![],
        };
        assert!(scan_notes(&files, &lists).is_empty());
    }

    #[test]
    fn email_is_read_from_an_id_token_payload() {
        // {"email":"dralorex@gmail.com"}
        let email = email_from_id_token(
            "aaa.eyJlbWFpbCI6ImRyYWxvcmV4QGdtYWlsLmNvbSJ9.bbb",
        );
        assert_eq!(email.as_deref(), Some("dralorex@gmail.com"));
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
                ..CatalogPack::default()
            }],
        };
        assert_eq!(
            hosted_url(&catalog, "ABC").as_deref(),
            Some("https://launcher.thehollowcrownmc.com/files/mod.jar")
        );
        assert_eq!(hosted_url(&catalog, "nope"), None);
    }

    const CATALOG: &str =
        "https://launcher.thehollowcrownmc.com/api/v1/catalog";

    fn sample_pack(mrpack_url: Option<&str>) -> CatalogPack {
        CatalogPack {
            name: "The Hollow Crown Iron Legacy".into(),
            mrpack_url: mrpack_url.map(str::to_string),
            modrinth_project_id: Some("the-hollow-crown-iron-legacy".into()),
            modrinth_version_id: Some("NaXkJw1s".into()),
            ..CatalogPack::default()
        }
    }

    #[test]
    fn relative_mrpack_url_uses_the_launcher_host() {
        let url =
            resolve_mrpack_url("/files/iron-legacy.mrpack", CATALOG).unwrap();
        assert_eq!(
            url,
            "https://launcher.thehollowcrownmc.com/files/iron-legacy.mrpack"
        );
    }

    #[test]
    fn absolute_mrpack_url_on_the_launcher_host_is_kept() {
        let raw =
            "https://launcher.thehollowcrownmc.com/files/iron-legacy.mrpack";
        assert_eq!(resolve_mrpack_url(raw, CATALOG).unwrap(), raw);
    }

    #[test]
    fn relative_mrpack_url_follows_a_catalog_override_origin() {
        let url = resolve_mrpack_url(
            "/files/a.mrpack",
            "http://127.0.0.1:9/api/v1/catalog",
        )
        .unwrap();
        assert_eq!(url, "http://127.0.0.1:9/files/a.mrpack");
    }

    #[test]
    fn mrpack_url_on_another_host_is_refused() {
        let error =
            resolve_mrpack_url("https://example.com/files/a.mrpack", CATALOG)
                .unwrap_err();
        assert!(error.contains("launcher.thehollowcrownmc.com"), "{error}");
    }

    #[test]
    fn path_outside_files_is_refused() {
        let error =
            resolve_mrpack_url("/files/../secret.mrpack", CATALOG).unwrap_err();
        assert!(error.contains("/files/"), "{error}");
    }

    #[test]
    fn modrinth_pack_without_mrpack_url_stays_on_modrinth() {
        let packs = vec![sample_pack(None)];
        assert_eq!(
            select_mrpack_url(
                &packs,
                Some("the-hollow-crown-iron-legacy"),
                Some("NaXkJw1s"),
                None,
                CATALOG,
            ),
            None
        );
    }

    #[test]
    fn mrpack_url_replaces_the_modrinth_version_for_that_project() {
        let packs = vec![sample_pack(Some("/files/iron-legacy.mrpack"))];
        assert_eq!(
            select_mrpack_url(
                &packs,
                Some("the-hollow-crown-iron-legacy"),
                Some("NaXkJw1s"),
                None,
                CATALOG,
            )
            .as_deref(),
            Some("/files/iron-legacy.mrpack")
        );
    }

    #[test]
    fn stored_mrpack_url_is_kept_when_the_catalog_drops_the_pack() {
        assert_eq!(
            select_mrpack_url(
                &[],
                None,
                None,
                Some("/files/iron-legacy.mrpack"),
                CATALOG,
            )
            .as_deref(),
            Some("/files/iron-legacy.mrpack")
        );
    }

    #[test]
    fn catalog_pack_with_only_an_mrpack_url_parses() {
        let catalog: Catalog = serde_json::from_str(
            r#"{"packs":[{"name":"Example","mrpack_url":"/files/example.mrpack","server_address":"play.thehollowcrownmc.com"}]}"#,
        )
        .unwrap();
        assert_eq!(
            pack_mrpack(&catalog.packs[0]),
            Some("/files/example.mrpack")
        );
        assert!(catalog.packs[0].modrinth_project_id.is_none());
    }

    #[tokio::test]
    async fn play_connects_to_the_paper_network() {
        let address =
            server_address_for_pack(Some("any-pack"), Some("any-version"))
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
