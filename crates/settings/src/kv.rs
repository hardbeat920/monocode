//! `Kv`, the key-value store that replaces webview localStorage.
//!
//! Keys and values are the strings the TypeScript app passed to
//! `localStorage.setItem`, so each ported module keeps its own load and save
//! code. The store keeps every item in memory. A file-backed store writes the
//! whole map to one JSON file `WRITE_DELAY` after a change, through a temp
//! file and a rename, so the file on disk is always complete. `flush` writes
//! at once, and dropping the last handle writes anything still pending.
//!
//! Change subscriptions replace both the `storage` event and the custom
//! `monocode:*-change` events the TypeScript dispatched after a save.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{
    Arc, Condvar, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The file a store opened with `Kv::open` keeps in its directory.
pub const KV_FILE_NAME: &str = "local-storage.json";

/// How long the writer waits after a change, so a burst of changes becomes
/// one write.
pub const WRITE_DELAY: Duration = Duration::from_millis(100);

const FILE_VERSION: u32 = 1;

static NEXT_STORE_ID: AtomicU64 = AtomicU64::new(1);

/// One change, shaped like a `StorageEvent`: the key, the value before, and
/// the value after. `new_value` is `None` when the key was removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KvChange {
    pub key: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
}

type Callback = Arc<dyn Fn(&KvChange) + Send + Sync>;

struct Subscriber {
    id: u64,
    /// `None` hears every key.
    key: Option<String>,
    callback: Callback,
}

#[derive(Default)]
struct State {
    items: BTreeMap<String, String>,
    /// Store bookkeeping that is not a localStorage item, such as the
    /// WebKit import record.
    meta: Map<String, Value>,
    /// Top-level fields of the file this version does not know, kept so a
    /// newer file survives a round trip.
    extra: Map<String, Value>,
    /// Bumped on every change, so the writer knows what is on disk.
    generation: u64,
}

#[derive(Default)]
struct Persist {
    /// Newest generation a change produced.
    dirty: u64,
    /// Newest generation on disk.
    written: u64,
    /// Newest generation whose write failed. The writer waits for a new
    /// change instead of retrying in a loop.
    failed: u64,
    shutdown: bool,
}

struct Shared {
    id: u64,
    path: Option<PathBuf>,
    state: RwLock<State>,
    persist: Mutex<Persist>,
    wake: Condvar,
    /// Keeps the writer thread and `flush` from writing at the same time.
    write_lock: Mutex<()>,
    subscribers: Mutex<Vec<Subscriber>>,
    next_subscriber: AtomicU64,
}

struct Inner {
    shared: Arc<Shared>,
    writer: Option<JoinHandle<()>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let Some(writer) = self.writer.take() else {
            return;
        };
        lock(&self.shared.persist).shutdown = true;
        self.shared.wake.notify_all();
        let _ = writer.join();
        // One more try if the writer's last write failed.
        if let Err(error) = self.shared.write_now() {
            log::warn!("could not save {}: {error}", self.shared.display_path());
        }
    }
}

/// The localStorage replacement. Cloning is cheap and every clone shares the
/// same items. All methods take `&self` and are safe to call from any thread.
#[derive(Clone)]
pub struct Kv {
    inner: Arc<Inner>,
}

impl fmt::Debug for Kv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Kv")
            .field("path", &self.inner.shared.path)
            .field("len", &self.len())
            .finish()
    }
}

/// Keeps a change callback registered. Dropping it unsubscribes.
#[must_use = "dropping a Subscription unsubscribes it"]
pub struct Subscription {
    shared: Weak<Shared>,
    id: u64,
}

impl Subscription {
    /// Keep the callback for the life of the store.
    pub fn detach(self) {
        std::mem::forget(self);
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.upgrade() {
            lock(&shared.subscribers).retain(|subscriber| subscriber.id != self.id);
        }
    }
}

impl fmt::Debug for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subscription")
            .field("id", &self.id)
            .finish()
    }
}

impl Kv {
    /// Open the store kept in `dir`, the app data directory
    /// (`~/Library/Application Support/com.monocode.desktop` on macOS). A
    /// missing file opens empty, and the directory is created on the first
    /// write. A file that is not valid JSON is renamed to
    /// `local-storage.json.corrupt-<ms>` and the store opens empty.
    pub fn open(dir: &Path) -> io::Result<Kv> {
        let path = dir.join(KV_FILE_NAME);
        let state = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<KvFileIn>(&bytes) {
                Ok(file) => State {
                    items: file.items,
                    meta: file.meta,
                    extra: file.extra,
                    generation: 0,
                },
                Err(error) => {
                    let aside = path.with_file_name(format!(
                        "{KV_FILE_NAME}.corrupt-{}",
                        unix_millis(SystemTime::now())
                    ));
                    log::warn!(
                        "{} is not valid JSON ({error}); moving it to {}",
                        path.display(),
                        aside.display()
                    );
                    fs::rename(&path, &aside)?;
                    State::default()
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => State::default(),
            Err(error) => return Err(error),
        };
        let shared = Arc::new(Shared::new(Some(path), state));
        let writer = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("monocode-kv-writer".into())
                .spawn(move || run_writer(&shared))?
        };
        Ok(Kv {
            inner: Arc::new(Inner {
                shared,
                writer: Some(writer),
            }),
        })
    }

    /// A store that never touches the disk, for tests.
    pub fn in_memory() -> Kv {
        Kv {
            inner: Arc::new(Inner {
                shared: Arc::new(Shared::new(None, State::default())),
                writer: None,
            }),
        }
    }

    /// The JSON file behind the store, or `None` for an in-memory store.
    pub fn path(&self) -> Option<&Path> {
        self.inner.shared.path.as_deref()
    }

    /// `localStorage.getItem`.
    pub fn get_item(&self, key: &str) -> Option<String> {
        read(&self.inner.shared.state).items.get(key).cloned()
    }

    /// `localStorage.setItem`. Storing the value a key already holds changes
    /// nothing and notifies no one, as in the Web Storage spec.
    pub fn set_item(&self, key: &str, value: &str) {
        let shared = &self.inner.shared;
        let (generation, old_value) = {
            let mut state = write(&shared.state);
            if state.items.get(key).map(String::as_str) == Some(value) {
                return;
            }
            let old_value = state.items.insert(key.to_string(), value.to_string());
            state.generation += 1;
            (state.generation, old_value)
        };
        shared.mark_dirty(generation);
        shared.notify(&KvChange {
            key: key.to_string(),
            old_value,
            new_value: Some(value.to_string()),
        });
    }

    /// `localStorage.removeItem`. Removing a missing key notifies no one.
    pub fn remove_item(&self, key: &str) {
        let shared = &self.inner.shared;
        let (generation, old_value) = {
            let mut state = write(&shared.state);
            let Some(old_value) = state.items.remove(key) else {
                return;
            };
            state.generation += 1;
            (state.generation, old_value)
        };
        shared.mark_dirty(generation);
        shared.notify(&KvChange {
            key: key.to_string(),
            old_value: Some(old_value),
            new_value: None,
        });
    }

    /// Every key, sorted.
    pub fn keys(&self) -> Vec<String> {
        read(&self.inner.shared.state)
            .items
            .keys()
            .cloned()
            .collect()
    }

    /// `localStorage.key(index)`, over the sorted keys.
    pub fn key(&self, index: usize) -> Option<String> {
        read(&self.inner.shared.state)
            .items
            .keys()
            .nth(index)
            .cloned()
    }

    /// `localStorage.length`.
    pub fn len(&self) -> usize {
        read(&self.inner.shared.state).items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Call `callback` after every change to any key. Callbacks run on the
    /// thread that made the change, after the store is updated and with no
    /// lock held, so they may read or write the store. A callback that owns a
    /// `Kv` clone keeps the store open until the subscription is dropped, so
    /// a long-lived callback should send to a channel instead.
    pub fn subscribe(&self, callback: impl Fn(&KvChange) + Send + Sync + 'static) -> Subscription {
        self.add_subscriber(None, Arc::new(callback))
    }

    /// Call `callback` after every change to `key`.
    pub fn subscribe_key(
        &self,
        key: &str,
        callback: impl Fn(&KvChange) + Send + Sync + 'static,
    ) -> Subscription {
        self.add_subscriber(Some(key.to_string()), Arc::new(callback))
    }

    /// Write pending changes now. In-memory stores return `Ok` at once.
    pub fn flush(&self) -> io::Result<()> {
        self.inner.shared.write_now()
    }

    fn add_subscriber(&self, key: Option<String>, callback: Callback) -> Subscription {
        let shared = &self.inner.shared;
        let id = shared.next_subscriber.fetch_add(1, Ordering::Relaxed);
        lock(&shared.subscribers).push(Subscriber { id, key, callback });
        Subscription {
            shared: Arc::downgrade(shared),
            id,
        }
    }

    /// Identifies this store across clones, for caches keyed by store.
    pub(crate) fn id(&self) -> u64 {
        self.inner.shared.id
    }

    /// Store bookkeeping saved next to the items.
    pub(crate) fn meta(&self, key: &str) -> Option<Value> {
        read(&self.inner.shared.state).meta.get(key).cloned()
    }

    /// Add `items` whose keys the store does not hold yet and record
    /// `meta_key`, in one change, so the file never holds one without the
    /// other. Returns how many items were added.
    pub(crate) fn import_items(
        &self,
        items: Vec<(String, String)>,
        meta_key: &str,
        meta_value: Value,
    ) -> usize {
        let shared = &self.inner.shared;
        let mut changes = Vec::new();
        let generation = {
            let mut state = write(&shared.state);
            for (key, value) in items {
                if state.items.contains_key(&key) {
                    continue;
                }
                state.items.insert(key.clone(), value.clone());
                changes.push(KvChange {
                    key,
                    old_value: None,
                    new_value: Some(value),
                });
            }
            state.meta.insert(meta_key.to_string(), meta_value);
            state.generation += 1;
            state.generation
        };
        shared.mark_dirty(generation);
        for change in &changes {
            shared.notify(change);
        }
        changes.len()
    }
}

impl Shared {
    fn new(path: Option<PathBuf>, state: State) -> Self {
        Shared {
            id: NEXT_STORE_ID.fetch_add(1, Ordering::Relaxed),
            path,
            state: RwLock::new(state),
            persist: Mutex::new(Persist::default()),
            wake: Condvar::new(),
            write_lock: Mutex::new(()),
            subscribers: Mutex::new(Vec::new()),
            next_subscriber: AtomicU64::new(1),
        }
    }

    fn display_path(&self) -> String {
        self.path
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_default()
    }

    fn mark_dirty(&self, generation: u64) {
        if self.path.is_none() {
            return;
        }
        let mut persist = lock(&self.persist);
        persist.dirty = persist.dirty.max(generation);
        drop(persist);
        self.wake.notify_all();
    }

    fn notify(&self, change: &KvChange) {
        let callbacks: Vec<Callback> = lock(&self.subscribers)
            .iter()
            .filter(|subscriber| {
                subscriber
                    .key
                    .as_deref()
                    .is_none_or(|key| key == change.key)
            })
            .map(|subscriber| Arc::clone(&subscriber.callback))
            .collect();
        for callback in callbacks {
            callback(change);
        }
    }

    /// Write the current items if the file is behind them.
    fn write_now(&self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let _writing = lock(&self.write_lock);
        let (bytes, generation) = {
            let state = read(&self.state);
            if state.generation <= lock(&self.persist).written {
                return Ok(());
            }
            let file = KvFileOut {
                version: FILE_VERSION,
                items: &state.items,
                meta: &state.meta,
                extra: &state.extra,
            };
            let bytes = serde_json::to_vec_pretty(&file).map_err(io::Error::other)?;
            (bytes, state.generation)
        };
        let result = write_atomic(path, &bytes, self.id);
        let mut persist = lock(&self.persist);
        match &result {
            Ok(()) => persist.written = persist.written.max(generation),
            Err(_) => persist.failed = persist.failed.max(generation),
        }
        result
    }
}

fn run_writer(shared: &Shared) {
    let mut persist = lock(&shared.persist);
    loop {
        if persist.dirty > persist.written.max(persist.failed) {
            let deadline = Instant::now() + WRITE_DELAY;
            while !persist.shutdown {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                persist = shared
                    .wake
                    .wait_timeout(persist, deadline - now)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
            drop(persist);
            if let Err(error) = shared.write_now() {
                log::warn!("could not save {}: {error}", shared.display_path());
            }
            persist = lock(&shared.persist);
            continue;
        }
        if persist.shutdown {
            return;
        }
        persist = shared
            .wake
            .wait(persist)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

/// Write `bytes` to a temp file next to `path`, sync it, and rename it over
/// `path`.
fn write_atomic(path: &Path, bytes: &[u8], store_id: u64) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| KV_FILE_NAME.to_string());
    let temp = dir.join(format!(".{name}.{}-{store_id}.tmp", std::process::id()));
    let result = write_private(&temp, bytes).and_then(|()| fs::rename(&temp, path));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result?;
    sync_dir(dir);
    Ok(())
}

/// The store can hold tokens, so on Unix only the owner may read it.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Make the rename itself durable. Windows has no directory handle to sync.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(dir) = fs::File::open(dir) {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

pub(crate) fn unix_millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// The file as read. `extra` keeps fields a newer version added.
#[derive(Deserialize)]
struct KvFileIn {
    #[serde(default)]
    #[allow(dead_code)]
    version: u32,
    #[serde(default)]
    items: BTreeMap<String, String>,
    #[serde(default)]
    meta: Map<String, Value>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Serialize)]
struct KvFileOut<'a> {
    version: u32,
    items: &'a BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    meta: &'a Map<String, Value>,
    #[serde(flatten)]
    extra: &'a Map<String, Value>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

fn write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::temp::TempDir;
    use std::sync::atomic::AtomicUsize;

    fn changes(kv: &Kv) -> (Arc<Mutex<Vec<KvChange>>>, Subscription) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let subscription = kv.subscribe(move |change| sink.lock().unwrap().push(change.clone()));
        (seen, subscription)
    }

    #[test]
    fn gets_sets_and_removes_items_like_local_storage() {
        let kv = Kv::in_memory();
        assert_eq!(kv.get_item("monocode.x"), None);
        kv.set_item("monocode.x", "1");
        kv.set_item("monocode.a", "{\"b\":[1,2]}");
        assert_eq!(kv.get_item("monocode.x").as_deref(), Some("1"));
        assert_eq!(kv.len(), 2);
        assert_eq!(kv.keys(), ["monocode.a", "monocode.x"]);
        assert_eq!(kv.key(1).as_deref(), Some("monocode.x"));
        assert_eq!(kv.key(2), None);
        kv.remove_item("monocode.x");
        assert_eq!(kv.get_item("monocode.x"), None);
        assert_eq!(kv.len(), 1);
        assert!(!kv.is_empty());
        assert!(kv.path().is_none());
        assert!(kv.flush().is_ok());
    }

    #[test]
    fn notifies_subscribers_with_old_and_new_values() {
        let kv = Kv::in_memory();
        let (seen, _subscription) = changes(&kv);
        kv.set_item("k", "1");
        kv.set_item("k", "2");
        kv.remove_item("k");
        assert_eq!(
            *seen.lock().unwrap(),
            [
                KvChange {
                    key: "k".into(),
                    old_value: None,
                    new_value: Some("1".into()),
                },
                KvChange {
                    key: "k".into(),
                    old_value: Some("1".into()),
                    new_value: Some("2".into()),
                },
                KvChange {
                    key: "k".into(),
                    old_value: Some("2".into()),
                    new_value: None,
                },
            ]
        );
    }

    #[test]
    fn skips_notifications_when_nothing_changes() {
        let kv = Kv::in_memory();
        kv.set_item("k", "1");
        let (seen, _subscription) = changes(&kv);
        kv.set_item("k", "1");
        kv.remove_item("missing");
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn key_subscriptions_hear_only_their_key_and_stop_when_dropped() {
        let kv = Kv::in_memory();
        let count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&count);
        let subscription = kv.subscribe_key("a", move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        kv.set_item("a", "1");
        kv.set_item("b", "1");
        assert_eq!(count.load(Ordering::SeqCst), 1);
        drop(subscription);
        kv.set_item("a", "2");
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn callbacks_see_the_new_value_and_may_write() {
        let kv = Kv::in_memory();
        let reader = kv.clone();
        kv.subscribe_key("a", move |change| {
            assert_eq!(reader.get_item("a"), change.new_value);
            reader.set_item("b", "written from a callback");
        })
        .detach();
        kv.set_item("a", "1");
        assert_eq!(kv.get_item("b").as_deref(), Some("written from a callback"));
    }

    #[test]
    fn persists_to_one_json_file_and_reopens() {
        let dir = TempDir::new("kv-reopen").unwrap();
        let kv = Kv::open(dir.path()).unwrap();
        assert!(kv.is_empty());
        kv.set_item(
            "monocode.lastModel",
            r#"{"harness":"claude","model":"opus"}"#,
        );
        kv.set_item("monocode.unicode", "caf\u{e9} \u{1f600} \"quoted\"\n");
        kv.flush().unwrap();
        let path = dir.path().join(KV_FILE_NAME);
        assert_eq!(kv.path(), Some(path.as_path()));
        let json: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(
            json["items"]["monocode.lastModel"],
            r#"{"harness":"claude","model":"opus"}"#
        );
        drop(kv);
        let kv = Kv::open(dir.path()).unwrap();
        assert_eq!(kv.len(), 2);
        assert_eq!(
            kv.get_item("monocode.unicode").as_deref(),
            Some("caf\u{e9} \u{1f600} \"quoted\"\n")
        );
    }

    #[test]
    fn drop_writes_pending_changes_without_a_flush() {
        let dir = TempDir::new("kv-drop").unwrap();
        let kv = Kv::open(dir.path()).unwrap();
        kv.set_item("k", "v");
        drop(kv);
        assert_eq!(
            Kv::open(dir.path()).unwrap().get_item("k").as_deref(),
            Some("v")
        );
    }

    #[test]
    fn the_writer_saves_after_the_delay() {
        let dir = TempDir::new("kv-delay").unwrap();
        let kv = Kv::open(dir.path()).unwrap();
        kv.set_item("k", "v");
        let path = dir.path().join(KV_FILE_NAME);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let json: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(json["items"]["k"], "v");
    }

    #[test]
    fn leaves_no_temp_files_and_keeps_the_file_private() {
        let dir = TempDir::new("kv-temp").unwrap();
        let kv = Kv::open(dir.path()).unwrap();
        for i in 0..20 {
            kv.set_item("k", &i.to_string());
            kv.flush().unwrap();
        }
        let names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [KV_FILE_NAME]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.path().join(KV_FILE_NAME))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn creates_the_directory_on_first_write() {
        let dir = TempDir::new("kv-mkdir").unwrap();
        let nested = dir.path().join("a").join("b");
        let kv = Kv::open(&nested).unwrap();
        kv.set_item("k", "v");
        kv.flush().unwrap();
        assert!(nested.join(KV_FILE_NAME).exists());
    }

    #[test]
    fn moves_a_corrupt_file_aside_and_opens_empty() {
        let dir = TempDir::new("kv-corrupt").unwrap();
        fs::write(dir.path().join(KV_FILE_NAME), b"{not json").unwrap();
        let kv = Kv::open(dir.path()).unwrap();
        assert!(kv.is_empty());
        let aside = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .find(|name| name.starts_with(&format!("{KV_FILE_NAME}.corrupt-")));
        assert!(aside.is_some());
    }

    #[test]
    fn keeps_unknown_top_level_fields_and_meta() {
        let dir = TempDir::new("kv-extra").unwrap();
        fs::write(
            dir.path().join(KV_FILE_NAME),
            br#"{"version":1,"items":{"a":"1"},"meta":{"m":true},"future":{"x":1}}"#,
        )
        .unwrap();
        let kv = Kv::open(dir.path()).unwrap();
        assert_eq!(kv.meta("m"), Some(Value::Bool(true)));
        kv.set_item("b", "2");
        kv.flush().unwrap();
        let json: Value =
            serde_json::from_slice(&fs::read(dir.path().join(KV_FILE_NAME)).unwrap()).unwrap();
        assert_eq!(json["future"]["x"], 1);
        assert_eq!(json["meta"]["m"], true);
        assert_eq!(json["items"]["a"], "1");
    }

    #[test]
    fn many_threads_can_write_at_once() {
        let dir = TempDir::new("kv-threads").unwrap();
        let kv = Kv::open(dir.path()).unwrap();
        let heard = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&heard);
        let _subscription = kv.subscribe(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let threads: Vec<_> = (0..8)
            .map(|t| {
                let kv = kv.clone();
                std::thread::spawn(move || {
                    for i in 0..100 {
                        kv.set_item(&format!("t{t}.k{i}"), &format!("{t}-{i}"));
                        assert_eq!(kv.get_item(&format!("t{t}.k{i}")), Some(format!("{t}-{i}")));
                        if i % 25 == 0 {
                            kv.flush().unwrap();
                        }
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(heard.load(Ordering::SeqCst), 800);
        drop(kv);
        let kv = Kv::open(dir.path()).unwrap();
        assert_eq!(kv.len(), 800);
        assert_eq!(kv.get_item("t7.k99").as_deref(), Some("7-99"));
    }

    #[test]
    fn import_items_keeps_existing_keys_and_records_meta() {
        let kv = Kv::in_memory();
        kv.set_item("a", "native");
        let (seen, _subscription) = changes(&kv);
        let added = kv.import_items(
            vec![("a".into(), "old".into()), ("b".into(), "old".into())],
            "record",
            Value::from(1),
        );
        assert_eq!(added, 1);
        assert_eq!(kv.get_item("a").as_deref(), Some("native"));
        assert_eq!(kv.get_item("b").as_deref(), Some("old"));
        assert_eq!(kv.meta("record"), Some(Value::from(1)));
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}
