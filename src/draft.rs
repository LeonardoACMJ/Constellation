//! One portable recovery document. It is never the launcher's live config.
use crate::{config::Config, storage};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
};
pub const LIMIT: u64 = 16 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub version: u32,
    pub baseline: u64,
    #[serde(with = "config_text")]
    pub config: Config,
    pub universe: u64,
    pub selected: u64,
    /// Raw fields include invalid/unapplied edits and owned-dialog drafts.
    pub fields: BTreeMap<String, String>,
}
pub fn fingerprint(c: &Config) -> u64 {
    serde_json::to_vec(c)
        .unwrap_or_default()
        .iter()
        .fold(0xcbf29ce484222325, |h, b| {
            (h ^ *b as u64).wrapping_mul(0x100000001b3)
        })
}
pub fn read(root: &Path) -> Result<Option<Draft>, String> {
    let file = match std::fs::File::open(root.join("data/editor-draft.json")) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = vec![];
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("Draft recovery document exceeds 16 MiB".into());
    }
    let draft: Draft = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if draft.version != 1 {
        return Err("Unsupported recovery version".into());
    }
    Ok(Some(draft))
}
mod config_text {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        config: &Config,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&serde_json::to_string(config).map_err(serde::ser::Error::custom)?)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(decoder: D) -> Result<Config, D::Error> {
        storage::parse_config(String::deserialize(decoder)?.as_bytes())
            .map_err(serde::de::Error::custom)
    }
}
struct State {
    pending: Option<Draft>,
    generation: u64,
    stopped: bool,
    error: Option<String>,
}
pub struct Writer {
    root: PathBuf,
    shared: Arc<(Mutex<State>, Condvar)>,
}
impl Writer {
    pub fn new(root: PathBuf) -> Result<Self, String> {
        let shared = Arc::new((
            Mutex::new(State {
                pending: None,
                generation: 0,
                stopped: false,
                error: None,
            }),
            Condvar::new(),
        ));
        let background = shared.clone();
        let folder = root.clone();
        std::thread::Builder::new()
            .name("constellation-draft".into())
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let (lock, wake) = &*background;
                loop {
                    let (draft, generation) = {
                        let mut state = lock.lock().unwrap();
                        while !state.stopped && state.pending.is_none() {
                            state = wake.wait(state).unwrap();
                        }
                        if state.stopped {
                            return;
                        }
                        (state.pending.take().unwrap(), state.generation)
                    };
                    let path = folder.join("data/editor-draft.json");
                    let temp = folder.join(format!(
                        "data/editor-draft-{}-{generation}.tmp",
                        std::process::id()
                    ));
                    let result = (|| -> Result<(), String> {
                        let bytes = serde_json::to_vec(&draft).map_err(|e| e.to_string())?;
                        if bytes.len() as u64 > LIMIT {
                            return Err("Recovery document exceeds 16 MiB".into());
                        }
                        std::fs::create_dir_all(folder.join("data")).map_err(|e| e.to_string())?;
                        let mut f = std::fs::File::create(&temp).map_err(|e| e.to_string())?;
                        f.write_all(&bytes).map_err(|e| e.to_string())?;
                        f.sync_all().map_err(|e| e.to_string())?;
                        drop(f);
                        let state = lock.lock().unwrap();
                        if state.generation == generation && !state.stopped {
                            storage::replace(&temp, &path).map_err(|e| e.to_string())?;
                        }
                        Ok(())
                    })();
                    let _ = std::fs::remove_file(&temp);
                    let mut state = lock.lock().unwrap();
                    if state.generation == generation {
                        state.error = result.err();
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { root, shared })
    }
    pub fn submit(&self, draft: Draft) {
        let (mut state, wake) = (self.shared.0.lock().unwrap(), &self.shared.1);
        state.generation = state.generation.wrapping_add(1);
        state.pending = Some(draft);
        wake.notify_one();
    }
    pub fn error(&self) -> Option<String> {
        self.shared.0.lock().unwrap().error.clone()
    }
    pub fn clear(&self) -> Result<(), String> {
        let mut state = self.shared.0.lock().unwrap();
        state.generation = state.generation.wrapping_add(1);
        state.pending = None;
        match std::fs::remove_file(self.root.join("data/editor-draft.json")) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        let mut state = self.shared.0.lock().unwrap();
        state.stopped = true;
        state.generation = state.generation.wrapping_add(1);
        self.shared.1.notify_one();
    }
}
