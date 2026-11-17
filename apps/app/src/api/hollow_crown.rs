use crate::api::Result;
use theseus::install::InstallJobSnapshot;

pub fn init<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("hollow-crown")
        .invoke_handler(tauri::generate_handler![
            hollow_crown_catalog,
            hollow_crown_install_pack,
            hollow_crown_repair_instance,
            hollow_crown_owner_preview
        ])
        .build()
}

/// The Hollow Crown pack catalog. Install and repair still use the Modrinth pack flow.
#[tauri::command]
pub async fn hollow_crown_catalog() -> Result<serde_json::Value> {
    Ok(theseus::hollow_crown::catalog_value().await?)
}

/// Installs a catalog pack from its `mrpack_url` without a Modrinth version.
#[tauri::command]
pub async fn hollow_crown_install_pack(
    name: String,
) -> Result<InstallJobSnapshot> {
    Ok(theseus::hollow_crown::install_named_pack(&name).await?)
}

/// Repairs an instance from its catalog `.mrpack`.
///
/// `None` means the instance still uses Modrinth project and version ids.
#[tauri::command]
pub async fn hollow_crown_repair_instance(
    instance_id: String,
) -> Result<Option<InstallJobSnapshot>> {
    Ok(
        theseus::hollow_crown::repair_instance_from_catalog(&instance_id)
            .await?,
    )
}

/// Owner preview unlocks advanced Hollow Crown settings and sync UI.
#[tauri::command]
pub async fn hollow_crown_owner_preview() -> Result<bool> {
    Ok(theseus::hollow_crown::owner_preview_enabled().await)
}
