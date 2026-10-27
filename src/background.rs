//! Latest-only background work. No polling, unbounded queue, or joining a
//! thread which may be blocked in filesystem/third-party code.
use std::sync::{Arc, Condvar, Mutex};
struct State<K, V> {
    key: Option<K>,
    pending: Option<(K, u64)>,
    generation: u64,
    value: Option<V>,
    stopped: bool,
}
pub struct Latest<K, V> {
    shared: Arc<(Mutex<State<K, V>>, Condvar)>,
}
impl<K: Clone + PartialEq + Send + 'static, V: Clone + Send + 'static> Latest<K, V> {
    pub fn new(
        mut work: impl FnMut(&K) -> V + Send + 'static,
        notify: impl Fn() + Send + 'static,
    ) -> Result<Self, String> {
        let shared = Arc::new((
            Mutex::new(State {
                key: None,
                pending: None,
                generation: 0,
                value: None,
                stopped: false,
            }),
            Condvar::new(),
        ));
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("constellation-advice".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                loop {
                    let (lock, wake) = &*worker;
                    let (key, generation) = {
                        let Ok(mut state) = lock.lock() else {
                            return;
                        };
                        while !state.stopped && state.pending.is_none() {
                            state = match wake.wait(state) {
                                Ok(s) => s,
                                Err(_) => return,
                            };
                        }
                        if state.stopped {
                            return;
                        }
                        state.pending.take().unwrap()
                    };
                    let value = work(&key);
                    let Ok(mut state) = lock.lock() else {
                        return;
                    };
                    if state.stopped {
                        return;
                    }
                    if state.generation == generation && state.key.as_ref() == Some(&key) {
                        state.value = Some(value);
                        drop(state);
                        notify();
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { shared })
    }
    pub fn get(&self, key: &K) -> Option<V> {
        let (lock, wake) = &*self.shared;
        let mut state = lock.try_lock().ok()?;
        if state.key.as_ref() != Some(key) {
            state.key = Some(key.clone());
            state.generation = state.generation.wrapping_add(1);
            state.pending = Some((key.clone(), state.generation));
            state.value = None;
            wake.notify_one();
        }
        state.value.clone()
    }
    pub fn invalidate(&self) {
        if let Ok(mut state) = self.shared.0.lock() {
            state.generation = state.generation.wrapping_add(1);
            state.key = None;
            state.value = None;
            state.pending = None;
        }
    }
}
impl<K, V> Drop for Latest<K, V> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.shared.0.lock() {
            state.stopped = true;
            state.pending = None;
        }
        self.shared.1.notify_one();
    }
}
