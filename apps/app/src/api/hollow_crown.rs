use crate::api::Result;

pub fn init<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("hollow-crown")
        .invoke_handler(tauri::generate_handler![hollow_crown_catalog])
        .build()
}

/// The Hollow Crown pack catalog. Install and repair still use the Modrinth pack flow.
#[tauri::command]
pub async fn hollow_crown_catalog() -> Result<serde_json::Value> {
    Ok(theseus::hollow_crown::catalog_value().await?)
}
