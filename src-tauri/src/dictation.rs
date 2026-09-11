//! Session dictation, independent of meeting/project state. Audio never reaches disk.
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub enabled: bool,
    pub model: String,
    pub auto_insert: bool,
    pub save_history: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self { enabled: false, model: "kb-whisper-base".into(), auto_insert: true, save_history: false }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub created_at: u64,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_text: Option<String>,
    pub saved: bool,
    pub delivery: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    revision: u64,
    supported: bool,
    settings: Settings,
    pub phase: String,
    started_at: Option<u64>,
    message: String,
    hold_shortcut_ready: bool,
    toggle_shortcut_ready: bool,
    shortcut_error: Option<String>,
    input_mode: InputMode,
    backend: &'static str,
    preparing_model: bool,
    storage_error: Option<String>,
    entries: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
struct Disk {
    settings: Settings,
    entries: Vec<Entry>,
}

struct Inner {
    snapshot: Snapshot,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    // Refuse to overwrite unreadable history; session dictation remains available.
    unreadable: bool,
}
pub struct Dictation {
    inner: Mutex<Inner>,
    file: PathBuf,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
fn active(phase: &str) -> bool {
    matches!(phase, "starting" | "recording" | "processing")
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum InputMode {
    Button,
    Hold,
    Toggle,
}

#[derive(Debug, PartialEq, Eq)]
enum ShortcutTransition {
    Start,
    Stop,
    Ignore,
}

fn shortcut_transition(phase: &str, current: InputMode, requested: InputMode) -> ShortcutTransition {
    if phase == "processing" {
        return ShortcutTransition::Ignore;
    }
    if matches!(phase, "starting" | "recording") {
        // A hold press only starts a fresh session. It must not toggle another recording.
        // The button can always stop; a toggle shortcut stops only a toggle session.
        return if requested == InputMode::Button || (requested == InputMode::Toggle && current == InputMode::Toggle) {
            ShortcutTransition::Stop
        } else {
            ShortcutTransition::Ignore
        };
    }
    ShortcutTransition::Start
}

fn stop_session(inner: &mut Inner, token: &Arc<AtomicBool>) -> bool {
    // A delayed release from an old session must never stop a newly started one.
    if !Arc::ptr_eq(&inner.stop, token) || !matches!(inner.snapshot.phase.as_str(), "starting" | "recording") {
        return false;
    }
    token.store(true, Ordering::Relaxed);
    inner.snapshot.phase = "processing".into();
    inner.snapshot.message = "Bearbetar diktatet…".into();
    true
}

impl Dictation {
    pub fn new(file: PathBuf, default_model: String) -> Self {
        let (disk, error) = match std::fs::read(&file) {
            Ok(bytes) => match serde_json::from_slice::<Disk>(&bytes) {
                Ok(disk) => (Some(disk), None),
                Err(e) => (None, Some(format!("Dikthistoriken kunde inte läsas och lämnas orörd: {e}"))),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (None, None),
            Err(e) => (None, Some(format!("Dikthistoriken kunde inte öppnas: {e}"))),
        };
        let Disk { settings, entries } = disk.unwrap_or_else(|| Disk {
            settings: Settings { model: default_model, ..Settings::default() },
            entries: vec![],
        });
        Self {
            file,
            inner: Mutex::new(Inner {
                unreadable: error.is_some(),
                snapshot: Snapshot {
                    revision: 0,
                    supported: cfg!(windows),
                    settings,
                    phase: "idle".into(),
                    started_at: None,
                    message: "Redo att diktera".into(),
                    hold_shortcut_ready: false,
                    toggle_shortcut_ready: false,
                    shortcut_error: None,
                    input_mode: InputMode::Button,
                    backend: crate::transcribe::Transcriber::backend_label(),
                    preparing_model: false,
                    storage_error: error,
                    entries,
                },
                stop: Arc::new(AtomicBool::new(false)),
                cancel: Arc::new(AtomicBool::new(false)),
            }),
        }
    }

    pub fn is_active(&self) -> bool {
        active(&self.inner.lock().unwrap().snapshot.phase)
    }

    pub fn while_idle<T>(&self, action: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let inner = self.inner.lock().unwrap();
        if active(&inner.snapshot.phase) {
            return Err("Stoppa diktatet innan du startar en mötesinspelning.".into());
        }
        action()
    }

    fn persist(&self, inner: &Inner) -> Result<(), String> {
        if inner.unreadable {
            return Err("Den befintliga historiken kunde inte läsas. Den skrivs inte över. Diktaten finns kvar under sessionen.".into());
        }
        let disk = Disk {
            settings: inner.snapshot.settings.clone(),
            entries: inner.snapshot.entries.iter().filter(|e| e.saved).cloned().collect(),
        };
        let bytes = serde_json::to_vec_pretty(&disk).map_err(|e| e.to_string())?;
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        crate::storage::atomic_write(&self.file, &bytes).map_err(|e| e.to_string())
    }

    fn publish(app: &AppHandle, inner: &mut Inner) {
        inner.snapshot.revision += 1;
        let _ = app.emit("avskrift:dictation", &inner.snapshot);
        if let Some(window) = app.get_webview_window("dictation-overlay") {
            if active(&inner.snapshot.phase) {
                let _ = window.show();
            } else {
                let revision = inner.snapshot.revision;
                let app = app.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(6));
                    let state = app.state::<Dictation>();
                    let inner = state.inner.lock().unwrap();
                    if inner.snapshot.revision == revision {
                        if let Some(window) = app.get_webview_window("dictation-overlay") {
                            let _ = window.hide();
                        }
                    }
                });
            }
        }
    }

    #[cfg(windows)]
    pub fn shortcuts_enabled(&self) -> bool {
        self.inner.lock().unwrap().snapshot.settings.enabled
    }
    #[cfg(windows)]
    pub fn shortcut_result(&self, app: &AppHandle, hold_ready: bool, toggle_ready: bool, error: Option<String>) {
        let mut inner = self.inner.lock().unwrap();
        inner.snapshot.hold_shortcut_ready = hold_ready;
        inner.snapshot.toggle_shortcut_ready = toggle_ready;
        inner.snapshot.shortcut_error = error;
        Self::publish(app, &mut inner);
    }
}

#[tauri::command]
pub fn dictation_snapshot(state: State<Dictation>) -> Snapshot {
    state.inner.lock().unwrap().snapshot.clone()
}

#[tauri::command]
pub fn configure_dictation(app: AppHandle, settings: Settings) -> Result<(), String> {
    if crate::models::whisper_url(&settings.model).is_none() {
        return Err("Okänd talmodell".into());
    }
    let state = app.state::<Dictation>();
    let mut inner = state.inner.lock().unwrap();
    if active(&inner.snapshot.phase) {
        return Err("Stoppa diktatet innan du ändrar inställningar.".into());
    }
    let old = inner.snapshot.settings.clone();
    inner.snapshot.settings = settings;
    if let Err(e) = state.persist(&inner) {
        inner.snapshot.settings = old;
        return Err(e);
    }
    inner.snapshot.storage_error = None;
    Dictation::publish(&app, &mut inner);
    let model = inner.snapshot.settings.model.clone();
    let enabled = inner.snapshot.settings.enabled;
    drop(inner);
    if enabled {
        warm_model(&app, model);
    }
    Ok(())
}

fn warm_model(app: &AppHandle, model: String) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let backend = app.state::<crate::Backend>();
        let path = backend.paths.whisper_file(&model);
        if path.is_file() {
            // Do not enqueue behind an existing job or compete with its model selection.
            {
                let mut transcriber = crate::transcribe::Transcriber::new();
                let state = app.state::<Dictation>();
                {
                    let mut inner = state.inner.lock().unwrap();
                    if inner.snapshot.settings.model != model {
                        return;
                    }
                    inner.snapshot.preparing_model = true;
                    Dictation::publish(&app, &mut inner);
                }
                let result = transcriber.try_prepare(&path);
                drop(transcriber);
                let mut inner = state.inner.lock().unwrap();
                inner.snapshot.preparing_model = false;
                if inner.snapshot.phase == "idle" {
                    if let Err(error) = result {
                        inner.snapshot.message = format!("Modellen kunde inte förberedas: {error}");
                    }
                }
                Dictation::publish(&app, &mut inner);
            }
        }
    });
}

#[tauri::command]
pub fn edit_dictation(app: AppHandle, id: String, text: String, saved: bool, expected_text: Option<String>) -> Result<(), String> {
    if text.len() > 100_000 {
        return Err("Diktatet är för långt.".into());
    }
    let state = app.state::<Dictation>();
    let mut inner = state.inner.lock().unwrap();
    let index = inner.snapshot.entries.iter().position(|e| e.id == id).ok_or("Diktatet finns inte kvar.")?;
    let old = inner.snapshot.entries[index].clone();
    if expected_text.is_some_and(|expected| expected != old.text) {
        return Err("Diktatet har ändrats sedan förslaget skapades. Stäng förslaget och bearbeta den aktuella texten.".into());
    }
    if inner.snapshot.entries[index].original_text.is_none() {
        inner.snapshot.entries[index].original_text = Some(old.text.clone());
    }
    inner.snapshot.entries[index].text = text;
    inner.snapshot.entries[index].saved = saved;
    if old.saved || saved {
        if let Err(e) = state.persist(&inner) {
            inner.snapshot.entries[index] = old;
            return Err(e);
        }
    }
    Dictation::publish(&app, &mut inner);
    Ok(())
}

#[tauri::command]
pub fn delete_dictation(app: AppHandle, id: String) -> Result<(), String> {
    let state = app.state::<Dictation>();
    let mut inner = state.inner.lock().unwrap();
    let index = inner.snapshot.entries.iter().position(|e| e.id == id).ok_or("Diktatet finns inte kvar.")?;
    let old = inner.snapshot.entries.remove(index);
    if old.saved {
        if let Err(e) = state.persist(&inner) {
            inner.snapshot.entries.insert(index, old);
            return Err(e);
        }
    }
    Dictation::publish(&app, &mut inner);
    Ok(())
}

#[tauri::command]
pub fn cancel_dictation(app: AppHandle) {
    let state = app.state::<Dictation>();
    let mut inner = state.inner.lock().unwrap();
    inner.cancel.store(true, Ordering::Relaxed);
    inner.stop.store(true, Ordering::Relaxed);
    if active(&inner.snapshot.phase) {
        inner.snapshot.message = "Avbryter…".into();
        Dictation::publish(&app, &mut inner);
    }
}

#[tauri::command]
pub fn toggle_dictation(app: AppHandle) -> Result<(), String> {
    request_recording(&app, InputMode::Button).map(|_| ())
}

#[cfg(windows)]
pub fn press_hotkey(app: &AppHandle, hold: bool) -> Option<Arc<AtomicBool>> {
    match request_recording(app, if hold { InputMode::Hold } else { InputMode::Toggle }) {
        Ok(token) => token,
        Err(error) => {
            let state = app.state::<Dictation>();
            let mut inner = state.inner.lock().unwrap();
            inner.snapshot.message = error;
            Dictation::publish(app, &mut inner);
            if let Some(window) = app.get_webview_window("dictation-overlay") {
                let _ = window.show();
            }
            None
        }
    }
}

#[cfg(windows)]
pub fn release_hotkey(app: &AppHandle, token: Arc<AtomicBool>) {
    let state = app.state::<Dictation>();
    let mut inner = state.inner.lock().unwrap();
    if stop_session(&mut inner, &token) {
        Dictation::publish(app, &mut inner);
    }
}

fn request_recording(app: &AppHandle, mode: InputMode) -> Result<Option<Arc<AtomicBool>>, String> {
    let external = mode != InputMode::Button;
    if !cfg!(windows) {
        return Err("Diktering finns ännu bara på Windows.".into());
    }
    let state = app.state::<Dictation>();
    let mut inner = state.inner.lock().unwrap();
    if external && !inner.snapshot.settings.enabled {
        return Ok(None);
    }
    match shortcut_transition(&inner.snapshot.phase, inner.snapshot.input_mode, mode) {
        ShortcutTransition::Ignore => return Ok(None),
        ShortcutTransition::Stop => {
            let token = inner.stop.clone();
            if stop_session(&mut inner, &token) {
                Dictation::publish(app, &mut inner);
            }
            return Ok(None);
        }
        ShortcutTransition::Start => {}
    }
    let backend = app.state::<crate::Backend>();
    let settings = inner.snapshot.settings.clone();
    if !backend.paths.whisper_file(&settings.model).is_file() {
        return Err("Hämta den valda talmodellen i Transkribera ljud först.".into());
    }
    if backend.meeting.lock().unwrap().is_some() {
        return Err("Stoppa mötesinspelningen innan du dikterar.".into());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let cancel = Arc::new(AtomicBool::new(false));
    inner.stop = stop.clone();
    let session_token = stop.clone();
    inner.cancel = cancel.clone();
    inner.snapshot.input_mode = mode;
    inner.snapshot.phase = "starting".into();
    inner.snapshot.started_at = None;
    inner.snapshot.message = "Startar mikrofonen…".into();
    Dictation::publish(app, &mut inner);
    #[cfg(windows)]
    {
        let app = app.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                session(&app, settings, external, stop, cancel.clone())
            }));
            let error = match result {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(e),
                Err(_) => Some("Dikteringen avbröts oväntat.".into()),
            };
            if let Some(error) = error {
                let state = app.state::<Dictation>();
                let mut inner = state.inner.lock().unwrap();
                inner.snapshot.phase = "idle".into();
                inner.snapshot.message = error;
                Dictation::publish(&app, &mut inner);
            }
        });
    }
    Ok(Some(session_token))
}

#[cfg(windows)]
fn session(
    app: &AppHandle,
    settings: Settings,
    external: bool,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    use crate::dictation_native::{Com, Target};
    let _com = Com::new()?;
    let target = if external && settings.auto_insert { Target::capture() } else { None };
    warm_model(app, settings.model.clone());
    let samples = crate::dictation_native::record(stop.clone(), || {
        let state = app.state::<Dictation>();
        let mut inner = state.inner.lock().unwrap();
        if !stop.load(Ordering::Relaxed) {
            inner.snapshot.phase = "recording".into();
            inner.snapshot.started_at = Some(now());
            inner.snapshot.message = "Lyssnar…".into();
            Dictation::publish(app, &mut inner);
        }
    })?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Diktatet avbröts. Inget sparades.".into());
    }
    if !has_audio(&samples) {
        return Err("Inget tydligt ljud registrerades. Kontrollera mikrofonen och försök igen.".into());
    }
    {
        let state = app.state::<Dictation>();
        let mut inner = state.inner.lock().unwrap();
        inner.snapshot.phase = "processing".into();
        inner.snapshot.message = "Transkriberar lokalt…".into();
        Dictation::publish(app, &mut inner);
    }
    let backend = app.state::<crate::Backend>();
    // Queue above normal jobs once the active native pass releases its allocation.
    let _priority = crate::work::Scope::dictation(cancel.clone());
    let mut transcriber = crate::transcribe::Transcriber::new();
    let path = backend.paths.whisper_file(&settings.model);
    let segments = transcriber
        .transcribe(&settings.model, &path, &samples, "sv", false, false, &|_| {}, |_| {})
        .map_err(|e| e.to_string())?;
    drop(transcriber);
    drop(samples);
    if cancel.load(Ordering::Relaxed) {
        return Err("Diktatet avbröts. Inget sparades.".into());
    }
    let text = segments.into_iter().map(|s| s.text).collect::<Vec<_>>().join(" ").trim().to_string();
    if text.is_empty() {
        return Err("Ingen text kunde tolkas. Försök igen.".into());
    }
    // Recovery comes BEFORE insertion. Failure to save is visible and never drops the session text.
    let id = format!("{}", now());
    let state = app.state::<Dictation>();
    {
        let mut inner = state.inner.lock().unwrap();
        inner.snapshot.entries.insert(
            0,
            Entry {
                id: id.clone(),
                created_at: now(),
                text: text.clone(),
                original_text: None,
                saved: settings.save_history,
                delivery: "Redo att kopiera".into(),
            },
        );
        if settings.save_history {
            if let Err(error) = state.persist(&inner) {
                inner.snapshot.entries[0].saved = false;
                inner.snapshot.storage_error =
                    Some(format!("Kunde inte spara. Texten finns kvar under sessionen: {error}"));
            }
        }
        Dictation::publish(app, &mut inner);
    }
    let delivery = if cancel.load(Ordering::Relaxed) {
        "Infogningen avbröts. Texten finns kvar i Diktat.".into()
    } else if let Some(target) = target {
        match target.insert(&text) {
            Ok(()) => "Infogning skickad – kontrollera textfältet".into(),
            Err(reason) => reason,
        }
    } else if external && settings.auto_insert {
        "Inget säkert textfält hittades. Kopiera från Diktat.".into()
    } else {
        "Klart. Texten finns i Diktat.".into()
    };
    let mut inner = state.inner.lock().unwrap();
    if let Some(entry) = inner.snapshot.entries.iter_mut().find(|e| e.id == id) {
        entry.delivery = delivery.clone();
    }
    inner.snapshot.phase = "idle".into();
    inner.snapshot.message = delivery;
    Dictation::publish(app, &mut inner);
    Ok(())
}

fn has_audio(samples: &[f32]) -> bool {
    samples.len() >= 4000 && samples.iter().filter(|s| s.abs() > 0.008).count() >= 160
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let backend = app.state::<crate::Backend>();
    let catalogue = backend.paths.whisper_catalogue();
    let model = ["kb-whisper-base", "kb-whisper-small", "kb-whisper-tiny", "kb-whisper-medium", "kb-whisper-large"]
        .into_iter()
        .find(|id| catalogue.iter().any(|m| m.id == *id && m.downloaded))
        .unwrap_or("kb-whisper-base");
    let file = app.path().app_data_dir()?.join("dictation.json");
    app.manage(Dictation::new(file, model.into()));
    let initial = app.state::<Dictation>().inner.lock().unwrap().snapshot.settings.clone();
    if initial.enabled {
        warm_model(app, initial.model);
    }
    #[cfg(windows)]
    {
        tauri::WebviewWindowBuilder::new(app, "dictation-overlay", tauri::WebviewUrl::App("dictation-overlay".into()))
            .title("AVskrift – diktering")
            .inner_size(390.0, 112.0)
            .position(24.0, 24.0)
            .decorations(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .focusable(false)
            .visible(false)
            .build()?;
        crate::dictation_native::hotkeys(app.clone());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hold_presses_never_toggle_or_restart_a_busy_session() {
        assert_eq!(shortcut_transition("idle", InputMode::Button, InputMode::Hold), ShortcutTransition::Start);
        for phase in ["starting", "recording", "processing"] {
            for mode in [InputMode::Hold, InputMode::Toggle, InputMode::Button] {
                assert_eq!(shortcut_transition(phase, mode, InputMode::Hold), ShortcutTransition::Ignore);
            }
        }
        assert_eq!(shortcut_transition("recording", InputMode::Toggle, InputMode::Toggle), ShortcutTransition::Stop);
        assert_eq!(shortcut_transition("recording", InputMode::Hold, InputMode::Toggle), ShortcutTransition::Ignore);
        assert_eq!(shortcut_transition("recording", InputMode::Hold, InputMode::Button), ShortcutTransition::Stop);
        assert_eq!(shortcut_transition("processing", InputMode::Toggle, InputMode::Toggle), ShortcutTransition::Ignore);
    }

    #[test]
    fn release_during_startup_stops_once_and_never_stops_a_later_session() {
        let state = Dictation::new(temp_file("release"), "kb-whisper-base".into());
        let mut inner = state.inner.lock().unwrap();
        inner.snapshot.phase = "starting".into();
        inner.snapshot.input_mode = InputMode::Hold;
        let first = inner.stop.clone();
        assert!(stop_session(&mut inner, &first));
        assert!(first.load(Ordering::Relaxed));
        assert_eq!(inner.snapshot.phase, "processing");
        assert!(!stop_session(&mut inner, &first));
        inner.stop = Arc::new(AtomicBool::new(false));
        inner.snapshot.phase = "recording".into();
        inner.snapshot.input_mode = InputMode::Toggle;
        assert!(!stop_session(&mut inner, &first));
        assert!(!inner.stop.load(Ordering::Relaxed));
        assert_eq!(inner.snapshot.phase, "recording");
    }

    #[test]
    fn previous_shortcut_setting_does_not_break_saved_settings() {
        // The previous single-shortcut selector is ignored; both modes now have fixed shortcuts.
        let settings: Settings = serde_json::from_str(
            r#"{"enabled":true,"shortcut":"Ctrl+Alt+Space","model":"kb-whisper-small","saveHistory":true}"#,
        )
        .unwrap();
        assert!(settings.enabled && settings.save_history);
        assert_eq!(settings.model, "kb-whisper-small");
    }

    fn temp_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("avskrift-dictation-test-{}-{}-{name}.json", std::process::id(), now()))
    }
    #[test]
    fn persistence_excludes_session_text_and_keeps_saved_edits() {
        let file = temp_file("roundtrip");
        let state = Dictation::new(file.clone(), "kb-whisper-base".into());
        let mut inner = state.inner.lock().unwrap();
        inner.snapshot.entries = vec![
            Entry { id: "1".into(), created_at: 1, text: "Tillfälligt".into(), original_text: Some("Hemligt sessionsoriginal".into()), saved: false, delivery: "".into() },
            Entry { id: "2".into(), created_at: 2, text: "Sparat åäö".into(), original_text: Some("Original åäö".into()), saved: true, delivery: "".into() },
        ];
        state.persist(&inner).unwrap();
        inner.snapshot.entries[1].text = "Rättat".into();
        state.persist(&inner).unwrap();
        let reopened = Dictation::new(file.clone(), "kb-whisper-base".into());
        let loaded = reopened.inner.lock().unwrap();
        assert_eq!(loaded.snapshot.entries.len(), 1);
        assert_eq!(loaded.snapshot.entries[0].text, "Rättat");
        assert_eq!(loaded.snapshot.entries[0].original_text.as_deref(),Some("Original åäö"));
        assert!(!std::fs::read_to_string(&file).unwrap().contains("sessionsoriginal"));
        drop(loaded);
        std::fs::remove_file(file).unwrap();
    }
    #[test]
    fn corrupted_history_is_not_overwritten() {
        let file = temp_file("corrupt");
        std::fs::write(&file, b"broken").unwrap();
        let state = Dictation::new(file.clone(), "kb-whisper-base".into());
        assert!(state.persist(&state.inner.lock().unwrap()).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"broken");
        std::fs::remove_file(file).unwrap();
    }
    #[test]
    fn silence_and_clicks_do_not_trigger_whisper() {
        assert!(!has_audio(&vec![0.0; 16000]));
        let mut click = vec![0.0; 16000];
        click[100] = 1.0;
        assert!(!has_audio(&click));
        assert!(!has_audio(&vec![0.1; 3000]));
        assert!(has_audio(&vec![0.1; 16000]));
    }
}
