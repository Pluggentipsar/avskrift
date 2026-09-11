//! Windows dictation boundary. COM objects never leave the session worker thread.
//! Text is injected as Unicode, so the user's clipboard (including images) is untouched.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use windows::Win32::{
    Foundation::HWND,
    System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED},
    UI::{Accessibility::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

pub struct Com;
impl Com {
    pub fn new() -> Result<Self, String> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().map_err(|e| e.to_string())?;
        }
        Ok(Self)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

pub struct Target {
    automation: IUIAutomation,
    element: IUIAutomationElement,
    window: HWND,
}

impl Target {
    pub fn capture() -> Option<Self> {
        unsafe {
            let window = GetForegroundWindow();
            let mut pid = 0;
            GetWindowThreadProcessId(window, Some(&mut pid));
            if pid == 0 || pid == std::process::id() {
                return None;
            }
            let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let element = automation.GetFocusedElement().ok()?;
            if !editable(&element) {
                return None;
            }
            Some(Self { automation, element, window })
        }
    }

    pub fn insert(&self, text: &str) -> Result<(), String> {
        unsafe {
            // Do not release physical keys on the user's behalf. Wait briefly, then keep the draft.
            let start = Instant::now();
            while [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN, VK_SPACE]
                .iter()
                .any(|k| GetAsyncKeyState(k.0 as i32) < 0)
            {
                if start.elapsed() > Duration::from_secs(1) {
                    return Err("Tangenter hölls nere. Kopiera texten från Diktat.".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let current = self.automation.GetFocusedElement().map_err(|_| "Textfältet kunde inte kontrolleras.")?;
            if GetForegroundWindow() != self.window
                || !editable(&current)
                || !self.automation.CompareElements(&self.element, &current).map(|v| v.as_bool()).unwrap_or(false)
            {
                return Err("Fokus har ändrats. Texten finns kvar i Diktat.".into());
            }
            let inputs = unicode_inputs(text);
            if inputs.is_empty() {
                return Ok(());
            }
            // One SendInput batch avoids interleaving user keystrokes between our characters.
            if SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) != inputs.len() as u32 {
                return Err("Infogningen kan vara ofullständig. Kontrollera textfältet innan du kopierar igen.".into());
            }
            Ok(())
        }
    }
}

unsafe fn editable(element: &IUIAutomationElement) -> bool {
    if element.CurrentIsPassword().map(|v| v.as_bool()).unwrap_or(true)
        || !element.CurrentIsEnabled().map(|v| v.as_bool()).unwrap_or(false)
        || !element.CurrentIsKeyboardFocusable().map(|v| v.as_bool()).unwrap_or(false)
    {
        return false;
    }
    let Ok(kind) = element.CurrentControlType() else {
        return false;
    };
    if let Ok(value) = element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
        return (kind == UIA_EditControlTypeId || kind == UIA_DocumentControlTypeId)
            && !value.CurrentIsReadOnly().map(|v| v.as_bool()).unwrap_or(true);
    }
    kind == UIA_EditControlTypeId
        || (kind == UIA_DocumentControlTypeId
            && element.GetCurrentPatternAs::<IUIAutomationTextEditPattern>(UIA_TextEditPatternId).is_ok())
}

fn unicode_inputs(text: &str) -> Vec<INPUT> {
    // Never synthesize Enter/Tab: dictation must not submit a form or change focus.
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .encode_utf16()
        .flat_map(|unit| {
            [false, true].map(move |up| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wScan: unit,
                        dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                        ..Default::default()
                    },
                },
            })
        })
        .collect()
}

/// Capture only the microphone, in memory, with a five-minute ceiling. No WAV is created.
pub fn record(stop: Arc<AtomicBool>, ready: impl FnOnce()) -> Result<Vec<f32>, String> {
    use std::collections::VecDeque;
    use wasapi::*;
    let err = |e: wasapi::WasapiError| e.to_string();
    let enumerator = DeviceEnumerator::new().map_err(err)?;
    let device = enumerator.get_default_device(&Direction::Capture).map_err(err)?;
    let mut client = device.get_iaudioclient().map_err(err)?;
    let format = WaveFormat::new(32, 32, &SampleType::Float, 48000, 2, None);
    let (_, minimum) = client.get_device_period().map_err(err)?;
    client
        .initialize_client(
            &format,
            &Direction::Capture,
            &StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: minimum },
        )
        .map_err(err)?;
    let event = client.set_get_eventhandle().map_err(err)?;
    let capture = client.get_audiocaptureclient().map_err(err)?;
    client.start_stream().map_err(err)?;
    ready();
    let mut queue = VecDeque::new();
    let mut samples = Vec::new();
    let start = Instant::now();
    let result = (|| {
        loop {
            capture.read_from_device_to_deque(&mut queue).map_err(err)?;
            while queue.len() >= 8 {
                let mut channels = [0f32; 2];
                for value in &mut channels {
                    let bytes = std::array::from_fn(|_| queue.pop_front().unwrap());
                    *value = f32::from_le_bytes(bytes);
                }
                let value = (channels[0] + channels[1]) * 0.5;
                samples.push(if value.is_finite() { value.clamp(-1.0, 1.0) } else { 0.0 });
            }
            // Drain the last available packet before stopping, so releasing the shortcut does
            // not discard the end of the final word that arrived during the event wait.
            if stop.load(Ordering::Relaxed) || start.elapsed() >= Duration::from_secs(300) {
                break;
            }
            let _ = event.wait_for_event(50);
        }
        Ok(crate::audio::resample_to_16k(&samples, 48000))
    })();
    let _ = client.stop_stream();
    result
}

/// This thread owns its hotkey registration. Changes are acknowledged through app state.
pub fn hotkeys(app: tauri::AppHandle) {
    use tauri::Manager;
    std::thread::spawn(move || {
        let mut previous = None;
        let mut hold_ready = false;
        let mut toggle_ready = false;
        let mut held_session = None;
        loop {
            let state = app.state::<crate::dictation::Dictation>();
            let enabled = state.shortcuts_enabled();
            if previous != Some(enabled) {
                unsafe {
                    let _ = UnregisterHotKey(None, 1);
                    let _ = UnregisterHotKey(None, 2);
                }
                hold_ready = false;
                toggle_ready = false;
                let mut errors = Vec::new();
                if enabled {
                    hold_ready =
                        unsafe { RegisterHotKey(None, 1, MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT, VK_SPACE.0 as u32) }
                            .is_ok();
                    toggle_ready =
                        unsafe { RegisterHotKey(None, 2, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, VK_SPACE.0 as u32) }
                            .is_ok();
                    if !hold_ready {
                        errors.push("Ctrl+Shift+Space kunde inte registreras (håll inne).");
                    }
                    if !toggle_ready {
                        errors.push("Ctrl+Alt+Space kunde inte registreras (start/stopp).");
                    }
                }
                let error = if errors.is_empty() {
                    None
                } else {
                    Some(format!("{} En annan app kan använda genvägen. Stäng av och aktivera kortkommandona igen när den är ledig.", errors.join(" ")))
                };
                state.shortcut_result(&app, hold_ready, toggle_ready, error);
                previous = Some(enabled);
            }
            unsafe {
                let mut message = MSG::default();
                while PeekMessageW(&mut message, None, WM_HOTKEY, WM_HOTKEY, PM_REMOVE).as_bool() {
                    match message.wParam.0 {
                        1 if hold_ready && held_session.is_none() => {
                            held_session = crate::dictation::press_hotkey(&app, true)
                        }
                        2 if toggle_ready => {
                            crate::dictation::press_hotkey(&app, false);
                        }
                        _ => {}
                    }
                }
                // RegisterHotKey only reports presses. Poll the three keys while a hold session
                // exists; releasing ANY part of the combination stops that exact session once.
                // This also handles a quick tap whose release precedes WM_HOTKEY processing.
                if held_session.is_some() {
                    let down = [VK_CONTROL, VK_SHIFT, VK_SPACE].iter().all(|key| GetAsyncKeyState(key.0 as i32) < 0);
                    if !enabled || !down {
                        if let Some(token) = held_session.take() {
                            crate::dictation::release_hotkey(&app, token);
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_pairs_preserve_swedish_and_surrogates_without_enter() {
        let input = unicode_inputs("Åäö🙂\n\t");
        let units: Vec<u16> = "Åäö🙂  ".encode_utf16().collect();
        assert_eq!(input.len(), units.len() * 2);
        for (pair, unit) in input.chunks_exact(2).zip(units) {
            unsafe {
                assert_eq!(pair[0].Anonymous.ki.wScan, unit);
                assert_eq!(pair[0].Anonymous.ki.wVk.0, 0);
                assert_eq!(pair[1].Anonymous.ki.dwFlags, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP);
            }
        }
    }
}
