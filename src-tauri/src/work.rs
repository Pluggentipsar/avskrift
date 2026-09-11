//! Request-scoped cancellation. Registration precedes dispatch; publication and cancellation
//! share a lock so an accepted cancellation cannot replace the previous transcript.
use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use std::{cell::RefCell, collections::HashMap, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}}, time::{Duration, Instant}};

struct Entry { cancel: Arc<AtomicBool>, claimed: bool, committed: bool, created: Instant }
static JOBS: Lazy<Mutex<HashMap<String, Entry>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static NEXT: AtomicU64 = AtomicU64::new(1);
#[derive(Clone)]
struct Context { id: Option<String>, cancel: Arc<AtomicBool>, priority: bool }
thread_local! { static CURRENT: RefCell<Option<Context>> = const { RefCell::new(None) }; }
#[derive(Debug)]
pub struct Cancelled;
impl std::fmt::Display for Cancelled { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "Arbetet avbröts. Tidigare resultat finns kvar.") } }
impl std::error::Error for Cancelled {}

pub fn begin() -> Result<String> {
    let mut jobs = JOBS.lock().unwrap();
    jobs.retain(|_, e| e.claimed || e.created.elapsed() < Duration::from_secs(300));
    anyhow::ensure!(jobs.len() < 32, "För många väntande arbeten. Försök igen senare.");
    let id = NEXT.fetch_add(1, Ordering::Relaxed).to_string();
    jobs.insert(id.clone(), Entry { cancel: Arc::new(AtomicBool::new(false)), claimed: false, committed: false, created: Instant::now() });
    Ok(id)
}
pub fn cancel(id: &str) -> bool {
    let jobs = JOBS.lock().unwrap();
    if let Some(e) = jobs.get(id).filter(|e| !e.committed) { e.cancel.store(true, Ordering::Relaxed); true } else { false }
}
pub fn forget(id: &str) {
    let mut jobs = JOBS.lock().unwrap();
    if jobs.get(id).is_some_and(|e| !e.claimed) { jobs.remove(id); }
}
pub fn token() -> Option<Arc<AtomicBool>> { CURRENT.with(|c| c.borrow().as_ref().map(|c| c.cancel.clone())) }
pub fn priority() -> bool { CURRENT.with(|c| c.borrow().as_ref().is_some_and(|c| c.priority)) }
pub fn check() -> Result<()> {
    if token().is_some_and(|t| t.load(Ordering::Relaxed)) { Err(anyhow!(Cancelled)) } else { Ok(()) }
}
pub struct Scope(Option<Context>);
impl Scope {
    pub fn dictation(cancel: Arc<AtomicBool>) -> Self { Self::set(Context { id: None, cancel, priority: true }) }
    fn set(context: Context) -> Self { Self(CURRENT.with(|c| c.replace(Some(context)))) }
}
impl Drop for Scope { fn drop(&mut self) { CURRENT.with(|c| { c.replace(self.0.take()); }); } }
struct Registration(String);
impl Drop for Registration { fn drop(&mut self) { JOBS.lock().unwrap().remove(&self.0); } }
pub fn run<T>(id: Option<String>, run: impl FnOnce() -> Result<T>) -> Result<T> {
    let Some(id) = id else { return run(); };
    let token = {
        let mut jobs = JOBS.lock().unwrap();
        let e = jobs.get_mut(&id).ok_or_else(|| anyhow!("Arbetet har löpt ut. Försök igen."))?;
        anyhow::ensure!(!e.claimed, "Arbetet har redan startat.");
        e.claimed = true;
        e.cancel.clone()
    };
    let _registration = Registration(id.clone());
    let _scope = Scope::set(Context { id: Some(id), cancel: token, priority: false });
    check()?;
    let result = run()?;
    commit(|| ())?;
    Ok(result)
}
pub fn commit<T>(publish: impl FnOnce() -> T) -> Result<T> {
    let id = CURRENT.with(|c| c.borrow().as_ref().and_then(|c| c.id.clone()));
    let mut jobs = JOBS.lock().unwrap();
    check()?;
    let result = publish();
    if let Some(e) = id.and_then(|id| jobs.get_mut(&id)) { e.committed = true; }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn cancel_before_dispatch_and_isolation() {
        let id = begin().unwrap(); assert!(cancel(&id));
        assert!(run(Some(id.clone()), || -> Result<()> { panic!("cancelled work ran") }).is_err());
        assert!(!cancel(&id));
        let fresh = begin().unwrap(); assert_eq!(run(Some(fresh), || Ok(7)).unwrap(), 7);
    }
    #[test] fn accepted_cancel_preserves_previous_result() {
        let id = begin().unwrap(); let mut previous = 9;
        assert!(run(Some(id.clone()), || { assert!(cancel(&id)); commit(|| previous = 3) }).is_err());
        assert_eq!(previous, 9);
    }
    #[test] fn publication_wins_late_cancel() {
        let id = begin().unwrap();
        run(Some(id.clone()), || { commit(|| ())?; assert!(!cancel(&id)); Ok(()) }).unwrap();
    }
    #[test] fn panic_cleans_registration_and_context() {
        let id = begin().unwrap();
        let _ = std::panic::catch_unwind(|| run(Some(id.clone()), || -> Result<()> { panic!("worker panic") }));
        assert!(!cancel(&id)); assert!(token().is_none());
    }
}
