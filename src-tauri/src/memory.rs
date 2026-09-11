//! Shared admission lock and bounded model caches. Lock order: WORK -> model cache.
//! No active native allocation can be evicted; inference holds WORK until it returns.
use anyhow::Result;
#[cfg(test)]
use anyhow::anyhow;
use serde::Serialize;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

pub const MIB: u64 = 1024 * 1024;
pub const GPU_MARGIN: u64 = 512 * MIB;
pub const IDLE: Duration = Duration::from_secs(120);
pub static WORK: Gate = Gate::new();
struct Queue { busy: bool, priority_waiters: usize }
pub struct Gate { state: Mutex<Queue>, wake: Condvar }
pub struct Permit<'a>(&'a Gate);
impl Gate {
    const fn new() -> Self { Self { state: Mutex::new(Queue { busy: false, priority_waiters: 0 }), wake: Condvar::new() } }
    pub fn try_lock(&self) -> Result<Permit<'_>, ()> {
        let mut s = self.state.lock().map_err(|_| ())?;
        if s.busy || s.priority_waiters > 0 { return Err(()); }
        s.busy = true; Ok(Permit(self))
    }
    fn enter(&self) -> Result<Permit<'_>> {
        let priority = crate::work::priority();
        let mut s = self.state.lock().unwrap();
        if priority { s.priority_waiters += 1; }
        loop {
            if let Err(e) = crate::work::check() {
                if priority { s.priority_waiters -= 1; self.wake.notify_all(); }
                return Err(e);
            }
            if !s.busy && (priority || s.priority_waiters == 0) {
                if priority { s.priority_waiters -= 1; }
                s.busy = true; return Ok(Permit(self));
            }
            s = self.wake.wait_timeout(s, Duration::from_millis(50)).unwrap().0;
        }
    }
}
impl Drop for Permit<'_> {
    fn drop(&mut self) { self.0.state.lock().unwrap().busy = false; self.0.wake.notify_all(); }
}
pub fn enter() -> Result<Permit<'static>> { WORK.enter() }

#[cfg(test)]
mod queue_tests {
    use super::*;
    use std::sync::{Arc, mpsc, atomic::{AtomicBool, Ordering}};
    fn await_priority(g: &Gate) {
        let until = Instant::now() + Duration::from_secs(3);
        while g.state.lock().unwrap().priority_waiters == 0 {
            assert!(Instant::now() < until); std::thread::yield_now();
        }
    }
    #[test]
    fn dictation_passes_queued_work_after_active_step() {
        let gate = Arc::new(Gate::new()); let active = gate.enter().unwrap();
        let (tx, rx) = mpsc::channel();
        let g = gate.clone(); let tx1 = tx.clone();
        let low = std::thread::spawn(move || { let _p = g.enter().unwrap(); tx1.send("normal").unwrap(); });
        let g = gate.clone();
        let high = std::thread::spawn(move || {
            let _scope = crate::work::Scope::dictation(Arc::new(AtomicBool::new(false)));
            let _p = g.enter().unwrap(); tx.send("dictation").unwrap();
        });
        await_priority(&gate); assert!(rx.try_recv().is_err()); drop(active);
        assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), "dictation");
        assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), "normal");
        high.join().unwrap(); low.join().unwrap(); assert!(gate.try_lock().is_ok());
    }
    #[test]
    fn cancelled_waiter_leaves_no_priority_reservation() {
        let gate = Arc::new(Gate::new()); let active = gate.enter().unwrap();
        let flag = Arc::new(AtomicBool::new(false)); let f = flag.clone(); let g = gate.clone();
        let waiter = std::thread::spawn(move || {
            let _scope = crate::work::Scope::dictation(f); assert!(g.enter().is_err());
        });
        await_priority(&gate); flag.store(true,Ordering::Relaxed); waiter.join().unwrap();
        assert_eq!(gate.state.lock().unwrap().priority_waiters,0);
        drop(active); assert!(gate.try_lock().is_ok());
    }
}

pub struct Cache<T> {
    pub value: Option<T>,
    used: Instant,
}
impl<T> Cache<T> {
    pub fn new() -> Self {
        Self { value: None, used: Instant::now() }
    }
    pub fn touch(&mut self) {
        self.used = Instant::now();
    }
    pub fn clear(&mut self) {
        self.value = None;
    }
    pub fn sweep(&mut self, now: Instant, pressure: bool) {
        // Ten seconds prevents memory pressure from discarding a model between adjacent chunks.
        let limit = if pressure { Duration::from_secs(10) } else { IDLE };
        if now.saturating_duration_since(self.used) >= limit {
            self.clear();
        }
    }
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    pub ram_free: Option<u64>,
    pub ram_total: Option<u64>,
    pub gpus: Vec<DeviceMemory>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceMemory {
    #[serde(skip)]
    pub index: usize,
    pub name: String,
    pub free: u64,
    pub total: u64,
    pub integrated: bool,
}

impl MemoryInfo {
    pub fn text_devices(&self) -> Vec<usize> {
        self.gpus.iter().filter(|g| !g.integrated && g.free > GPU_MARGIN).map(|g| g.index).collect()
    }
    pub fn devices_under_pressure(&self, devices: &[usize]) -> bool {
        devices.iter().any(|id| self.gpus.iter().find(|g| g.index == *id).is_none_or(|g| g.free < GPU_MARGIN))
    }
    pub fn ram_pressure(&self) -> bool {
        matches!((self.ram_free, self.ram_total), (Some(f), Some(t)) if f < (t / 10).max(1024 * MIB))
    }
    pub fn pressure(&self) -> bool {
        // Shared-memory pressure is covered by RAM. An unused iGPU or unknown report must
        // not discard otherwise warm CPU/discrete-GPU models on a hybrid computer.
        self.ram_pressure() || self.gpus.iter().any(|g| !g.integrated && g.total > 0 && g.free < GPU_MARGIN)
    }
    pub fn cpu_fits(&self, bytes: u64) -> bool {
        self.ram_free.is_none_or(|free| free >= bytes.saturating_add(512 * MIB))
    }
    pub fn speech_gpu_fits(&self, bytes: u64) -> bool {
        // Whisper and llama have separate registries. A conservative minimum avoids treating
        // another adapter's memory as available on Whisper's default GPU.
        !self.gpus.is_empty()
            && self
                .gpus
                .iter()
                .all(|g| g.free >= bytes.saturating_add(GPU_MARGIN) && (!g.integrated || self.cpu_fits(bytes)))
    }
}

// Whisper statically links an older ggml while llama uses DLLs. Linking ggml_* calls directly
// can resolve to Whisper's registry, whose device pointers must NEVER enter llama. Resolve both
// registry and device operations explicitly from the matching text engine libraries instead.
type Device = *mut std::ffi::c_void;
struct DeviceApi {
    count: unsafe extern "C" fn() -> usize,
    get: unsafe extern "C" fn(usize) -> Device,
    kind: unsafe extern "C" fn(Device) -> i32,
    description: unsafe extern "C" fn(Device) -> *const std::ffi::c_char,
    memory: unsafe extern "C" fn(Device, *mut usize, *mut usize),
    _registry: libloading::Library,
    _base: libloading::Library,
}
impl DeviceApi {
    unsafe fn load() -> std::result::Result<Self, libloading::Error> {
        // These libraries are already dependencies of the loaded llama library. Keep their
        // handles alive with the function pointers; no runtime install or download is performed.
        let registry = libloading::Library::new(libloading::library_filename("ggml"))?;
        let base = libloading::Library::new(libloading::library_filename("ggml-base"))?;
        Ok(Self {
            count: *registry.get(b"ggml_backend_dev_count\0")?,
            get: *registry.get(b"ggml_backend_dev_get\0")?,
            kind: *base.get(b"ggml_backend_dev_type\0")?,
            description: *base.get(b"ggml_backend_dev_description\0")?,
            memory: *base.get(b"ggml_backend_dev_memory\0")?,
            _registry: registry,
            _base: base,
        })
    }
}
static DEVICES: once_cell::sync::Lazy<Option<DeviceApi>> =
    once_cell::sync::Lazy::new(|| match unsafe { DeviceApi::load() } {
        Ok(api) => Some(api),
        Err(e) => {
            eprintln!("AVskrift: minnesuppgifter från textmotorn är inte tillgängliga: {e}");
            None
        }
    });

/// Caller holds WORK. Device pointers never leave this query or enter a different ggml version.
pub fn sample() -> MemoryInfo {
    crate::llm::init_backend();
    let mut info = MemoryInfo::default();
    let Some(api) = DEVICES.as_ref() else {
        return info;
    };
    unsafe {
        for i in 0..(api.count)() {
            let dev = (api.get)(i);
            if dev.is_null() {
                continue;
            }
            let (mut free, mut total) = (0, 0);
            (api.memory)(dev, &mut free, &mut total);
            free = free.min(total);
            // Enum values and scalar signatures are from the pinned ggml-backend.h; no native
            // struct layout or device pointer is shared with the static speech engine.
            match (api.kind)(dev) {
                0 if total > 0 => {
                    info.ram_total = Some(total as u64);
                    if cfg!(windows) {
                        info.ram_free = Some(free as u64);
                    }
                }
                kind @ (1 | 2) if cfg!(any(feature = "vulkan", feature = "cuda", feature = "metal")) => {
                    let ptr = (api.description)(dev);
                    let name = if ptr.is_null() {
                        "GPU".into()
                    } else {
                        std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
                    };
                    info.gpus.push(DeviceMemory {
                        index: i,
                        name,
                        free: free as u64,
                        total: total as u64,
                        integrated: kind == 2,
                    });
                }
                _ => {}
            }
        }
    }
    info
}

/// Native APIs don't distinguish all device/allocation failures. Retry only errors explicitly
/// tagged at model/context/decode boundaries, never malformed prompts or incomplete responses.
#[derive(Debug)]
pub struct DeviceFailure(pub String);
impl std::fmt::Display for DeviceFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for DeviceFailure {}
pub fn can_retry(gpu: bool, error: &anyhow::Error) -> bool {
    gpu && error.downcast_ref::<DeviceFailure>().is_some()
}

// Faults only exist in test binaries; no environment switch can affect a shipped app.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Fault {
    TextContext,
    SpeechCompute,
}
#[cfg(test)]
thread_local! { static FAULT: std::cell::Cell<Option<Fault>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
pub(crate) fn inject(fault: Fault) {
    FAULT.set(Some(fault));
}
#[cfg(test)]
pub(crate) fn take_fault(fault: Fault) -> bool {
    FAULT.with(|f| {
        if f.get() == Some(fault) {
            f.set(None);
            true
        } else {
            false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_expires_after_last_use_and_pressure_has_a_grace_period() {
        let mut c = Cache::new();
        c.value = Some(7);
        let now = c.used;
        c.sweep(now + Duration::from_secs(119), false);
        assert_eq!(c.value, Some(7));
        c.sweep(now + IDLE, false);
        assert!(c.value.is_none());
        c.value = Some(8);
        c.touch();
        let now = c.used;
        c.sweep(now + Duration::from_secs(9), true);
        assert_eq!(c.value, Some(8));
        c.sweep(now + Duration::from_secs(10), true);
        assert!(c.value.is_none());
    }
    #[test]
    fn budgets_handle_integrated_small_unknown_and_busy_memory() {
        let mut m = MemoryInfo { ram_free: Some(3 * 1024 * MIB), ram_total: Some(8 * 1024 * MIB), gpus: vec![] };
        assert!(!m.speech_gpu_fits(MIB));
        assert!(m.cpu_fits(2 * 1024 * MIB));
        assert!(!m.cpu_fits(3 * 1024 * MIB));
        m.gpus.push(DeviceMemory {
            index: 0,
            name: "test".into(),
            free: 4 * 1024 * MIB,
            total: 8 * 1024 * MIB,
            integrated: false,
        });
        assert!(m.speech_gpu_fits(3 * 1024 * MIB));
        m.gpus[0].integrated = true;
        assert!(!m.speech_gpu_fits(3 * 1024 * MIB));
        m.ram_free = Some(400 * MIB);
        assert!(m.pressure());
        assert!(!m.cpu_fits(u64::MAX));
    }
    #[test]
    fn only_device_errors_on_gpu_are_retryable() {
        let e = anyhow!(DeviceFailure("allocation".into()));
        assert!(can_retry(true, &e));
        assert!(!can_retry(false, &e));
        assert!(!can_retry(true, &anyhow!("för långt svar")));
        assert!(!can_retry(true, &anyhow!("ogiltig grammatik")));
    }

    #[test]
    fn hybrid_graphics_budget_uses_only_selected_discrete_devices() {
        let mut m = MemoryInfo::default();
        m.gpus = vec![
            DeviceMemory { index: 0, name: "integrated".into(), free: 128 * MIB, total: 1024 * MIB, integrated: true },
            DeviceMemory {
                index: 1,
                name: "discrete".into(),
                free: 8 * 1024 * MIB,
                total: 16 * 1024 * MIB,
                integrated: false,
            },
        ];
        assert_eq!(m.text_devices(), vec![1]);
        assert!(!m.pressure());
        assert!(!m.devices_under_pressure(&[1]));
        assert!(m.devices_under_pressure(&[2]));
        m.gpus[1].free = 256 * MIB;
        assert!(m.text_devices().is_empty());
        assert!(m.devices_under_pressure(&[1]));
    }
}
