//! In-app updates: signed NSIS installers published on GitHub. The app only contacts GitHub when
//! the user asks it to look for an update. Each build variant (CPU, Vulkan) follows its own manifest,
//! per channel, in the release tagged `updates`; the updater verifies every installer against the
//! public key in tauri.conf.json before running it.
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

const MANIFESTS: &str = "https://github.com/Pluggentipsar/avskrift/releases/download/updates";

fn variant() -> &'static str {
    if cfg!(feature = "vulkan") { "vulkan" } else { "cpu" }
}

/// `stable` follows released versions only; `beta` also gets pre-releases (whichever is newest).
fn manifest_url(channel: &str) -> String {
    let channel = if channel == "stable" { "stable" } else { "beta" };
    format!("{MANIFESTS}/{channel}-{}.json", variant())
}

/// Installed with the NSIS installer (which leaves its uninstaller next to the exe), as opposed to a
/// portable folder from the ZIP. Only an installed copy can update itself in place.
fn installed() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("uninstall.exe").is_file()))
        .unwrap_or(false)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    current: String,
    installed: bool,
    variant: &'static str,
    available: Option<Available>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Available {
    version: String,
    notes: Option<String>,
}

fn updater(app: &AppHandle, channel: &str) -> Result<tauri_plugin_updater::Updater, String> {
    let url = manifest_url(channel).parse().map_err(|e| format!("{e}"))?;
    app.updater_builder()
        .endpoints(vec![url])
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_update(app: AppHandle, channel: String) -> Result<UpdateInfo, String> {
    let update = updater(&app, &channel)?
        .check()
        .await
        .map_err(|e| format!("Kunde inte söka efter uppdateringar: {e}"))?;
    Ok(UpdateInfo {
        current: app.package_info().version.to_string(),
        installed: installed(),
        variant: variant(),
        available: update.map(|u| Available { version: u.version, notes: u.body }),
    })
}

/// Download, verify and run the installer. On Windows the installer closes the app, replaces it and
/// starts it again; saved work lives in the app's data folder and is not touched.
#[tauri::command]
pub async fn install_update(app: AppHandle, channel: String) -> Result<(), String> {
    if !installed() {
        return Err("Den här kopian är portabel. Hämta den nya ZIP-filen från GitHub i stället.".into());
    }
    if app.state::<crate::Backend>().meeting.lock().map(|m| m.is_some()).unwrap_or(true) {
        return Err("Avsluta mötesinspelningen innan du uppdaterar.".into());
    }
    let update = updater(&app, &channel)?
        .check()
        .await
        .map_err(|e| format!("Kunde inte söka efter uppdateringar: {e}"))?
        .ok_or("Ingen ny version hittades.")?;
    let progress = app.clone();
    let mut received = 0u64;
    update
        .download_and_install(
            move |chunk, total| {
                received += chunk as u64;
                let _ = progress.emit("avskrift:update-progress", serde_json::json!({ "received": received, "total": total }));
            },
            || {},
        )
        .await
        .map_err(|e| format!("Uppdateringen kunde inte installeras: {e}"))?;
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn each_variant_and_channel_has_its_own_manifest() {
        let beta = manifest_url("beta");
        assert!(beta.ends_with(&format!("/updates/beta-{}.json", variant())));
        assert!(manifest_url("stable").contains("/stable-"));
        assert_eq!(manifest_url("anything"), beta, "unknown channels fall back to beta");
    }
}
