//! Separate a copy's inherited checkout edits from publishable Git history. The real
//! working files keep inherited edits; the index and public history do not.
use super::*;
use std::sync::{LazyLock, Mutex, OnceLock};

static HELPER: OnceLock<PathBuf> = OnceLock::new();
static REGISTRIES: LazyLock<Mutex<BTreeSet<PathBuf>>> = LazyLock::new(Default::default);
const MARKER: &str = "monocode-publication.json";

#[derive(Serialize, Deserialize)]
struct Marker {
    registry: PathBuf,
    id: String,
}

pub fn set_helper(path: PathBuf) {
    let _ = HELPER.set(path);
}
pub fn register_registry(path: PathBuf) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Ok(());
    }
    let fresh = REGISTRIES
        .lock()
        .map_err(|_| "Publication registry is unavailable")?
        .insert(path.clone());
    if fresh && HELPER.get().is_some() && path.exists() {
        for entry in fs::read_dir(&path).map_err(err)? {
            let entry = entry.map_err(err)?;
            let Ok(w) = read_json::<Workspace>(&entry.path().join("workspace.json")) else {
                continue;
            };
            if entry.file_name().to_str() != Some(&w.id) {
                continue;
            }
            if let Err(error) = load(&path, Path::new(&w.path), &w.id) {
                eprintln!(
                    "Copy publication protection is unavailable for {}: {error}",
                    w.id
                );
            }
        }
    }
    Ok(())
}

pub(super) fn install(store: &Path, w: &Workspace) -> Result<()> {
    let root = Path::new(&w.path);
    identity(&root.join(".git"))?;
    let marker = root.join(".git").join(MARKER);
    if !marker.try_exists().map_err(err)? {
        write_json(
            &marker,
            &Marker {
                registry: canonical(store)?,
                id: w.id.clone(),
            },
        )?;
    } else {
        let existing: Marker = read_json(&marker)?;
        if existing.registry != canonical(store)? || existing.id != w.id {
            return Err("Copy publication metadata was replaced".into());
        }
    }
    if let Some(helper) = HELPER.get() {
        let hooks = root.join(".git/monocode-hooks");
        private_dir(&hooks)?;
        #[cfg(unix)]
        for hook in ["pre-commit", "pre-push"] {
            use std::os::unix::fs::PermissionsExt;
            let executable = helper
                .to_str()
                .ok_or("Invalid isolation helper path")?
                .replace('\'', "'\\''");
            let script = format!("#!/bin/sh\nexec '{executable}' --cow-git-hook {hook}\n");
            let path = hooks.join(hook);
            if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err("Publication hook was replaced".into());
            }
            if fs::read_to_string(&path).ok().as_deref() != Some(&script) {
                let temp = hooks.join(uuid::Uuid::new_v4().to_string());
                fs::write(&temp, script).map_err(err)?;
                fs::set_permissions(&temp, fs::Permissions::from_mode(0o700)).map_err(err)?;
                fs::rename(temp, path).map_err(err)?;
            }
        }
        if config_values(root, "core.hooksPath", true)?
            .last()
            .map(String::as_str)
            != hooks.to_str()
        {
            text(
                root,
                &[
                    "config",
                    "core.hooksPath",
                    hooks.to_str().ok_or("Invalid hooks path")?,
                ],
            )?;
        }
    }
    Ok(())
}

/// None means an ordinary repository. Invalid CoW metadata is an error, never
/// permission to fall back to ordinary staging or publication.
pub fn run(cwd: &Path, action: &str, mut args: Value) -> Result<Option<Value>> {
    let Ok(top) = text(cwd, &["rev-parse", "--show-toplevel"]) else {
        return Ok(None);
    };
    let root = canonical(Path::new(&top))?;
    let marker = root.join(".git").join(MARKER);
    match fs::symlink_metadata(&marker) {
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            let registries = REGISTRIES
                .lock()
                .map_err(|_| "Publication registry is unavailable")?
                .clone();
            for registry in registries {
                if !registry.exists() {
                    continue;
                }
                for record in fs::read_dir(&registry).map_err(err)? {
                    let record = record.map_err(err)?;
                    let Ok(w) = read_json::<Workspace>(&record.path().join("workspace.json"))
                    else {
                        continue;
                    };
                    if Path::new(&w.path) == root && record.file_name().to_str() == Some(&w.id) {
                        load(&registry, &root, &w.id)?;
                        return run(cwd, action, args);
                    }
                }
            }
            return Ok(None);
        }
        Err(e) => return Err(err(e)),
        Ok(_) => (),
    }
    let marker: Marker = read_json(&marker)?;
    let w = load(&marker.registry, &root, &marker.id)?;
    if Path::new(&w.path) != root {
        return Err("Copy publication metadata was replaced".into());
    }
    args["cwd"] = json!(root);
    args["cowId"] = json!(marker.id);
    args["action"] = json!(action);
    args["prefix"] = json!(canonical(cwd)?.strip_prefix(&root).map_err(err)?);
    dispatch(
        &marker.registry,
        json!({"command":"cow_publication", "args":args}),
    )
    .map(Some)
}

pub fn hook(kind: &str, input: &str) -> Result<()> {
    let cwd = std::env::current_dir().map_err(err)?;
    let args = match kind {
        "pre-commit" => json!({"indexFile":std::env::var_os("GIT_INDEX_FILE").map(PathBuf::from)}),
        "pre-push" => json!({"input":input}),
        _ => return Err("Unknown copy-on-write Git hook".into()),
    };
    run(&cwd, kind, args)?.ok_or("Copy publication metadata is unavailable")?;
    Ok(())
}

struct Temporary(PathBuf);
impl Temporary {
    fn new(root: &Path) -> Result<Self> {
        let path = root
            .join(".git")
            .join(format!("monocode-publication-{}", uuid::Uuid::new_v4()));
        private_dir(&path)?;
        Ok(Self(path))
    }
    fn index(&self) -> PathBuf {
        self.0.join("index")
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    mode: String,
    oid: String,
}
type Entries = BTreeMap<String, Entry>;

fn entries(root: &Path, revision: &str) -> Result<Entries> {
    let data = git(root, &["ls-tree", "-r", "-z", revision], None)?;
    data.split(|b| *b == 0)
        .filter(|b| !b.is_empty())
        .map(|record| {
            let record = std::str::from_utf8(record).map_err(err)?;
            let (meta, path) = record.split_once('\t').ok_or("Invalid Git tree")?;
            relative(path)?;
            let fields: Vec<_> = meta.split_whitespace().collect();
            if fields.len() != 3 || !matches!(fields[0], "100644" | "100755" | "120000") {
                return Err("Unsupported publication file type".into());
            }
            Ok((
                path.into(),
                Entry {
                    mode: fields[0].into(),
                    oid: fields[2].into(),
                },
            ))
        })
        .collect()
}

fn normal_git(
    root: &Path,
    index: Option<&Path>,
    args: &[&str],
    input: Option<&[u8]>,
    search_path: Option<&str>,
) -> Result<Vec<u8>> {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command.current_dir(root).env("GIT_TERMINAL_PROMPT", "0");
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    if let Some(path) = search_path {
        command.env("PATH", path);
    }
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().map_err(err)?;
    if let Some(input) = input {
        child
            .stdin
            .take()
            .ok_or("Git stdin missing")?
            .write_all(input)
            .map_err(err)?;
    }
    let output = child.wait_with_output().map_err(err)?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().into());
    }
    Ok(output.stdout)
}
fn index_tree(root: &Path, index: Option<&Path>) -> Result<String> {
    let mut command = git_command(root);
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    let output = command.arg("write-tree").output().map_err(err)?;
    if !output.status.success() {
        return Err("Resolve the Git index conflicts before publishing".into());
    }
    String::from_utf8(output.stdout)
        .map(|s| s.trim().into())
        .map_err(err)
}
fn write_tree(root: &Path, values: &Entries) -> Result<String> {
    let temp = Temporary::new(root)?;
    normal_git(
        root,
        Some(&temp.index()),
        &["read-tree", "--empty"],
        None,
        None,
    )?;
    let mut input = Vec::new();
    for (path, entry) in values {
        input.extend_from_slice(format!("{} {}\t{}\0", entry.mode, entry.oid, path).as_bytes());
    }
    normal_git(
        root,
        Some(&temp.index()),
        &["update-index", "-z", "--index-info"],
        Some(&input),
        None,
    )?;
    index_tree(root, Some(&temp.index()))
}

struct Projection<'a> {
    store: PathBuf,
    root: &'a Path,
    objects: PathBuf,
    baseline: Entries,
    creation: Entries,
    original: Entries,
    trusted: String,
    head: String,
    normalized: BTreeMap<String, Option<Entry>>,
    captured: BTreeMap<String, Option<Entry>>,
    excluded: BTreeSet<String>,
    filters: bool,
    search_path: Option<&'a str>,
}
impl<'a> Projection<'a> {
    fn new(
        store: &Path,
        w: &Workspace,
        root: &'a Path,
        search_path: Option<&'a str>,
    ) -> Result<Self> {
        Self::new_at(
            store,
            w,
            root,
            search_path,
            text(root, &["rev-parse", "HEAD"])?,
        )
    }
    fn new_at(
        store: &Path,
        w: &Workspace,
        root: &'a Path,
        search_path: Option<&'a str>,
        head: String,
    ) -> Result<Self> {
        let objects = snapshot_directory(store, root, &w.id)?;
        let trusted = trusted_base(root, w, &head)?;
        Ok(Self {
            store: store.into(),
            baseline: entries(&objects, &w.baseline)?,
            creation: entries(root, &w.head)?,
            original: entries(root, &trusted)?,
            trusted,
            head,
            objects,
            root,
            normalized: BTreeMap::new(),
            captured: BTreeMap::new(),
            excluded: w.excluded.clone(),
            filters: true,
            search_path,
        })
    }
    fn baseline(&mut self, path: &str) -> Result<Option<Entry>> {
        if let Some(found) = self.normalized.get(path) {
            return Ok(found.clone());
        }
        let found = if let Some(entry) = self.baseline.get(path) {
            let bytes = git(&self.objects, &["cat-file", "blob", &entry.oid], None)?;
            let args = if entry.mode == "120000" {
                vec!["hash-object", "-w", "--stdin"]
            } else {
                vec!["hash-object", "-w", "--stdin", "--path", path]
            };
            let oid = if self.filters {
                normal_git(self.root, None, &args, Some(&bytes), self.search_path)?
            } else {
                git(self.root, &args, Some(&bytes))?
            };
            Some(Entry {
                mode: entry.mode.clone(),
                oid: String::from_utf8(oid).map_err(err)?.trim().into(),
            })
        } else {
            None
        };
        self.captured.insert(path.into(), found.clone());
        let creation = self.creation.get(path);
        let upstream = self.original.get(path);
        // Keep the inherited delta paired with the current upstream publication base.
        let rebased = if found.as_ref() == creation {
            upstream.cloned()
        } else if upstream == creation || upstream == found.as_ref() {
            found
        } else if creation.is_some() && upstream.is_some() && found.is_some() {
            self.merge(path, creation, upstream, found.as_ref())?
        } else {
            return Err(format!(
                "Cannot combine upstream and inherited changes in {path}; the copy was retained"
            ));
        };
        self.normalized.insert(path.into(), rebased.clone());
        Ok(rebased)
    }
    fn project(&mut self, revision: &str) -> Result<String> {
        let candidate = entries(self.root, revision)?;
        let paths: BTreeSet<_> = self
            .original
            .keys()
            .chain(candidate.keys())
            .cloned()
            .collect();
        let mut result = self.original.clone();
        for path in paths {
            let original = self.original.get(&path).cloned();
            let next = candidate.get(&path).cloned();
            if next == original {
                continue;
            }
            if self.excluded.contains(&path) {
                return Err(format!(
                    "Cannot separate edits to initially ignored file {path}; the copy was retained"
                ));
            }
            let baseline = self.baseline(&path)?;
            let projected =
                self.merge(&path, baseline.as_ref(), original.as_ref(), next.as_ref())?;
            if let Some(entry) = projected {
                result.insert(path, entry);
            } else {
                result.remove(&path);
            }
        }
        write_tree(self.root, &result)
    }
    fn merge(
        &self,
        path: &str,
        baseline: Option<&Entry>,
        original: Option<&Entry>,
        next: Option<&Entry>,
    ) -> Result<Option<Entry>> {
        if next == baseline {
            return Ok(original.cloned());
        }
        if baseline == original || next.is_none() || baseline.is_none() {
            return Ok(next.cloned());
        }
        let fail = || {
            format!(
                "Cannot separate new edits from inherited changes in {path}; the copy was retained"
            )
        };
        let (Some(base), Some(ours), Some(theirs)) = (baseline, original, next) else {
            return Err(fail());
        };
        let mode = if theirs.mode == base.mode {
            &ours.mode
        } else if ours.mode == base.mode || ours.mode == theirs.mode {
            &theirs.mode
        } else {
            return Err(fail());
        };
        let oid = if theirs.oid == base.oid {
            ours.oid.clone()
        } else if ours.oid == base.oid || ours.oid == theirs.oid {
            theirs.oid.clone()
        } else {
            if mode == "120000" {
                return Err(fail());
            }
            let temp = Temporary::new(self.root)?;
            for (name, entry) in [("base", base), ("ours", ours), ("theirs", theirs)] {
                fs::write(
                    temp.0.join(name),
                    git(self.root, &["cat-file", "blob", &entry.oid], None)?,
                )
                .map_err(err)?;
            }
            let output = Command::new("git")
                .args(["merge-file", "-p", "--diff-algorithm=histogram"])
                .arg(temp.0.join("ours"))
                .arg(temp.0.join("base"))
                .arg(temp.0.join("theirs"))
                .output()
                .map_err(err)?;
            if !output.status.success() {
                return Err(fail());
            }
            String::from_utf8(git(
                self.root,
                &["hash-object", "-w", "--stdin"],
                Some(&output.stdout),
            )?)
            .map_err(err)?
            .trim()
            .into()
        };
        Ok(Some(Entry {
            mode: mode.clone(),
            oid,
        }))
    }
}

fn trusted_base(root: &Path, w: &Workspace, head: &str) -> Result<String> {
    let mut base = w.head.clone();
    for remote in text(
        root,
        &["for-each-ref", "--format=%(objectname)", "refs/remotes"],
    )?
    .lines()
    {
        let Ok(shared) = text(root, &["merge-base", head, remote]) else {
            continue;
        };
        if git(
            root,
            &["merge-base", "--is-ancestor", &w.head, &shared],
            None,
        )
        .is_err()
        {
            continue;
        }
        if git(root, &["merge-base", "--is-ancestor", &base, &shared], None).is_ok() {
            base = shared;
        } else if git(root, &["merge-base", "--is-ancestor", &shared, &base], None).is_err() {
            return Err("This copy includes multiple independent upstream histories; retain it and review publication in a clean worktree".into());
        }
    }
    Ok(base)
}

fn working_tree(root: &Path, search_path: Option<&str>) -> Result<String> {
    let temp = Temporary::new(root)?;
    normal_git(
        root,
        Some(&temp.index()),
        &["read-tree", "HEAD"],
        None,
        search_path,
    )?;
    normal_git(
        root,
        Some(&temp.index()),
        &["add", "-A", "--", "."],
        None,
        search_path,
    )?;
    index_tree(root, Some(&temp.index()))
}

fn canonical_commit(w: &Workspace, projection: &mut Projection<'_>, head: &str) -> Result<String> {
    let root = projection.root;
    if git(root, &["merge-base", "--is-ancestor", &w.head, head], None).is_err() {
        return Err("The copy's branch no longer descends from its creation HEAD".into());
    }
    let mut mapped = BTreeMap::<String, String>::new();
    for commit in text(
        root,
        &[
            "rev-list",
            "--reverse",
            "--topo-order",
            &format!("{}..{head}", w.head),
        ],
    )?
    .lines()
    {
        if git(
            root,
            &["merge-base", "--is-ancestor", commit, &projection.trusted],
            None,
        )
        .is_ok()
        {
            mapped.insert(commit.into(), commit.into());
            continue;
        }
        let raw = git(root, &["cat-file", "commit", commit], None)?;
        let split = raw
            .windows(2)
            .position(|bytes| bytes == b"\n\n")
            .ok_or("Invalid commit")?;
        let headers = std::str::from_utf8(&raw[..split]).map_err(err)?;
        let mut changed = false;
        let mut parents = Vec::new();
        let mut next_headers = Vec::new();
        let mut tree = String::new();
        for line in headers.lines() {
            if let Some(original) = line.strip_prefix("tree ") {
                tree = project_commit(w, projection, commit, original)?;
                changed |= tree != original;
                next_headers.push(format!("tree {tree}"));
            } else if let Some(parent) = line.strip_prefix("parent ") {
                let next = mapped.get(parent).map_or(parent, String::as_str);
                changed |= next != parent;
                parents.push(next.to_string());
                next_headers.push(format!("parent {next}"));
            } else {
                next_headers.push(line.into());
            }
        }
        let next = if !changed {
            commit.to_string()
        } else {
            if headers.lines().any(|line| {
                line.starts_with("gpgsig ")
                    || line.starts_with("gpgsig-sha256 ")
                    || line.starts_with("mergetag ")
            }) {
                return Err("A signed private commit contains inherited edits. Retain the copy and recreate that commit through native Changes before publishing.".into());
            }
            if parents.len() == 1
                && text(root, &["rev-parse", &format!("{}^{{tree}}", parents[0])])? == tree
            {
                parents[0].clone()
            } else {
                let mut bytes = next_headers.join("\n").into_bytes();
                bytes.extend_from_slice(&raw[split..]);
                String::from_utf8(git(
                    root,
                    &["hash-object", "-w", "-t", "commit", "--stdin"],
                    Some(&bytes),
                )?)
                .map_err(err)?
                .trim()
                .into()
            }
        };
        mapped.insert(commit.into(), next);
    }
    Ok(mapped.get(head).cloned().unwrap_or_else(|| head.to_owned()))
}
fn project_commit(
    w: &Workspace,
    projection: &mut Projection<'_>,
    commit: &str,
    tree: &str,
) -> Result<String> {
    if git(
        projection.root,
        &["merge-base", "--is-ancestor", &projection.trusted, commit],
        None,
    )
    .is_ok()
    {
        return projection.project(tree);
    }
    let mut earlier = Projection::new_at(
        &projection.store,
        w,
        projection.root,
        projection.search_path,
        commit.into(),
    )?;
    earlier.filters = projection.filters;
    earlier.project(tree)
}
fn canonical_head(w: &Workspace, projection: &mut Projection<'_>, write: bool) -> Result<String> {
    let root = projection.root;
    let head = projection.head.clone();
    if text(root, &["rev-parse", "HEAD"])? != head {
        return Err("Copy HEAD changed during publication; retry".into());
    }
    let next = canonical_commit(w, projection, &head)?;
    if write && next != head {
        let branch = text(root, &["symbolic-ref", "HEAD"])?;
        text(
            root,
            &[
                "update-ref",
                &format!("refs/monocode/private/{}/{head}", w.id),
                &head,
            ],
        )?;
        text(root, &["update-ref", &branch, &next, &head])?;
    }
    Ok(next)
}

pub(super) fn retained_commit(store: &Path, w: &Workspace, reference: &str) -> Result<String> {
    let root = Path::new(&w.path);
    let oid = text(
        root,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )?;
    if git(root, &["merge-base", "--is-ancestor", &oid, &w.head], None).is_ok() {
        return text(root, &["rev-parse", "--verify", reference]);
    }
    let mut projection = Projection::new_at(store, w, root, None, oid.clone())?;
    projection.filters = false;
    let clean = canonical_commit(w, &mut projection, &oid)?;
    if clean != oid {
        if text(root, &["cat-file", "-t", reference])? != "commit" {
            return Err("An annotated tag includes inherited edits; retain the copy and recreate that tag before cleanup".into());
        }
        text(
            root,
            &[
                "update-ref",
                &format!("refs/monocode/private/{}/{oid}", w.id),
                &oid,
            ],
        )?;
    }
    if clean == oid {
        text(root, &["rev-parse", "--verify", reference])
    } else {
        Ok(clean)
    }
}

#[cfg(unix)]
fn restore_file(root: &Path, path: &str, mode: Option<&str>, bytes: &[u8]) -> Result<()> {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::PermissionsExt,
    };
    let (parent, name) = destination_parent(root, path)?;
    let Some(mode) = mode else {
        if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(err(std::io::Error::last_os_error()));
        }
        let mut ancestor = Path::new(path).parent();
        while let Some(path) = ancestor.filter(|p| !p.as_os_str().is_empty()) {
            let (parent, name) = destination_parent(root, path.to_str().ok_or("Invalid path")?)?;
            if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0
            {
                break;
            }
            ancestor = path.parent();
        }
        return Ok(());
    };
    let temporary =
        std::ffi::CString::new(format!(".monocode-{}", uuid::Uuid::new_v4())).map_err(err)?;
    let result = (|| {
        if mode == "120000" {
            let link = std::ffi::CString::new(bytes).map_err(err)?;
            if unsafe { libc::symlinkat(link.as_ptr(), parent.as_raw_fd(), temporary.as_ptr()) }
                != 0
            {
                return Err(err(std::io::Error::last_os_error()));
            }
        } else {
            let fd = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    temporary.as_ptr(),
                    libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(err(std::io::Error::last_os_error()));
            }
            let mut file = unsafe { File::from_raw_fd(fd) };
            file.write_all(bytes).map_err(err)?;
            file.set_permissions(fs::Permissions::from_mode(if mode == "100755" {
                0o755
            } else {
                0o644
            }))
            .map_err(err)?;
            file.sync_all().map_err(err)?;
        }
        if unsafe {
            libc::renameat(
                parent.as_raw_fd(),
                temporary.as_ptr(),
                parent.as_raw_fd(),
                name.as_ptr(),
            )
        } != 0
        {
            return Err(err(std::io::Error::last_os_error()));
        }
        Ok(())
    })();
    if result.is_err() {
        unsafe {
            libc::unlinkat(parent.as_raw_fd(), temporary.as_ptr(), 0);
        }
    }
    result
}
#[cfg(not(unix))]
fn restore_file(_: &Path, _: &str, _: Option<&str>, _: &[u8]) -> Result<()> {
    Err(UNSUPPORTED_FILESYSTEM.into())
}

fn discard(projection: &mut Projection<'_>, args: &Value) -> Result<()> {
    let root = projection.root;
    let index = projection.project(&index_tree(root, None)?)?;
    let working = projection.project(&working_tree(root, projection.search_path)?)?;
    let index = entries(root, &index)?;
    let working = entries(root, &working)?;
    let prefix = args["prefix"].as_str().unwrap_or("");
    let selected = args["relative"]
        .as_str()
        .map(|path| relative_path(prefix, path))
        .transpose()?;
    let paths: BTreeSet<_> = index
        .keys()
        .chain(working.keys())
        .filter(|path| {
            let in_scope = Path::new(path).starts_with(prefix);
            in_scope
                && selected.as_ref().is_none_or(|selected| {
                    *path == selected || Path::new(path).starts_with(selected)
                })
        })
        .cloned()
        .collect();
    let mut pending = Vec::new();
    for path in paths {
        if index.get(&path) == working.get(&path) {
            continue;
        }
        let inherited = projection.baseline(&path)?;
        let restored = projection.merge(
            &path,
            projection.original.get(&path),
            inherited.as_ref(),
            index.get(&path),
        )?;
        let raw = if restored == inherited && projection.captured.get(&path) == Some(&inherited) {
            projection
                .baseline
                .get(&path)
                .map(|entry| git(&projection.objects, &["cat-file", "blob", &entry.oid], None))
                .transpose()?
                .unwrap_or_default()
        } else if let Some(entry) = &restored {
            if entry.mode == "120000" {
                git(root, &["cat-file", "blob", &entry.oid], None)?
            } else {
                normal_git(
                    root,
                    None,
                    &["cat-file", "--filters", "--path", &path, &entry.oid],
                    None,
                    projection.search_path,
                )?
            }
        } else {
            Vec::new()
        };
        pending.push((path, restored.map(|e| e.mode), raw));
    }
    pending.sort_by_key(|(path, mode, _)| {
        (mode.is_some(), std::cmp::Reverse(path.matches('/').count()))
    });
    #[cfg(unix)]
    let removed: BTreeSet<String> = pending
        .iter()
        .filter(|(_, mode, _)| mode.is_none())
        .map(|(path, _, _)| path.clone())
        .collect();
    #[cfg(unix)]
    let mut directories = BTreeSet::<String>::new();
    #[cfg(not(unix))]
    let directories = BTreeSet::<String>::new();
    #[cfg(unix)]
    for (path, mode, _) in &pending {
        if mode.is_some() && fs::symlink_metadata(root.join(path)).is_ok_and(|m| m.is_dir()) {
            replacing_directories(root, path, &removed, &mut directories)?;
        }
    }
    let mut directories: Vec<_> = directories.into_iter().collect();
    directories.sort_by_key(|path: &String| std::cmp::Reverse(path.matches('/').count()));
    for (path, mode, bytes) in pending {
        if mode.is_some() {
            for directory in directories.drain(..) {
                if !root.join(&directory).try_exists().map_err(err)? {
                    continue;
                }
                #[cfg(unix)]
                {
                    use std::os::fd::AsRawFd;
                    let (parent, name) = destination_parent(root, &directory)?;
                    if unsafe {
                        libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR)
                    } != 0
                    {
                        return Err(err(std::io::Error::last_os_error()));
                    }
                }
            }
        }
        restore_file(root, &path, mode.as_deref(), &bytes)?;
    }
    Ok(())
}

pub(super) fn dirty(store: &Path, w: &Workspace) -> Result<bool> {
    let root = Path::new(&w.path);
    if !git_dirty(root).unwrap_or(true) {
        return Ok(false);
    }
    let (objects, working) = current(store, w)?;
    if working == w.baseline
        && text(root, &["rev-parse", "HEAD"])? == w.head
        && index_tree(root, None)? == text(root, &["rev-parse", "HEAD^{tree}"])?
    {
        return Ok(false);
    }
    if text(root, &["config", "--get-regexp", "^filter\\."]).is_ok() {
        return Err("Cannot verify copy cleanup without running configured Git filters; the copy was retained".into());
    }
    let _ = objects;
    let temp = Temporary::new(root)?;
    for args in [&["read-tree", "HEAD"][..], &["add", "-A", "--", "."][..]] {
        let output = git_command(root)
            .env("GIT_INDEX_FILE", temp.index())
            .args(args)
            .output()
            .map_err(err)?;
        if !output.status.success() {
            return Err("Cannot verify the copy's uncommitted edits".into());
        }
    }
    let mut projection = Projection::new(store, w, root, None)?;
    projection.filters = false;
    let head = projection.project("HEAD")?;
    let index = projection.project(&index_tree(root, None)?)?;
    let working = projection.project(&index_tree(root, Some(&temp.index()))?)?;
    Ok(index != head || working != index)
}

fn stage(root: &Path, projection: &mut Projection<'_>, args: &Value) -> Result<()> {
    let temp = Temporary::new(root)?;
    let original = fs::read(root.join(".git/index")).map_err(err)?;
    let head = projection.head.clone();
    if text(root, &["rev-parse", "HEAD"])? != head {
        return Err("Copy HEAD changed during staging; retry".into());
    }
    fs::write(temp.index(), &original).map_err(err)?;
    let relative = args["relative"].as_str();
    let path = relative
        .map(|path| relative_path(args["prefix"].as_str().unwrap_or(""), path))
        .transpose()?;
    let index = temp.index();
    let scope = args["prefix"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(".");
    match args["mode"].as_str().ok_or("Missing staging mode")? {
        "all" => {
            normal_git(
                root,
                Some(&index),
                &["--literal-pathspecs", "add", "-A", "--", scope],
                None,
                projection.search_path,
            )?;
        }
        "file" => {
            normal_git(
                root,
                Some(&index),
                &[
                    "--literal-pathspecs",
                    "add",
                    "--",
                    path.as_deref().ok_or("Missing path")?,
                ],
                None,
                projection.search_path,
            )?;
        }
        "unstage-all" => {
            let command = if scope == "." {
                vec!["read-tree", "HEAD"]
            } else {
                vec![
                    "--literal-pathspecs",
                    "restore",
                    "--source=HEAD",
                    "--staged",
                    "--",
                    scope,
                ]
            };
            normal_git(root, Some(&index), &command, None, projection.search_path)?;
        }
        "unstage-file" => {
            normal_git(
                root,
                Some(&index),
                &[
                    "--literal-pathspecs",
                    "restore",
                    "--source=HEAD",
                    "--staged",
                    "--",
                    path.as_deref().ok_or("Missing path")?,
                ],
                None,
                projection.search_path,
            )?;
        }
        "contents" => {
            let path = path.as_deref().ok_or("Missing path")?;
            let contents = args["contents"]
                .as_str()
                .ok_or("Missing contents")?
                .as_bytes();
            let hash = String::from_utf8(normal_git(
                root,
                Some(&index),
                &["hash-object", "-w", "--stdin", "--path", path],
                Some(contents),
                projection.search_path,
            )?)
            .map_err(err)?;
            let values = entries(root, &index_tree(root, Some(&index))?)?;
            let mode = values.get(path).map_or("100644", |e| e.mode.as_str());
            normal_git(
                root,
                Some(&index),
                &[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    mode,
                    hash.trim(),
                    path,
                ],
                None,
                projection.search_path,
            )?;
        }
        _ => return Err("Unknown staging mode".into()),
    }
    let tree = projection.project(&index_tree(root, Some(&index))?)?;
    normal_git(
        root,
        Some(&index),
        &["read-tree", &tree],
        None,
        projection.search_path,
    )?;
    install_index(root, &root.join(".git/index"), &index, &original, &head)
}

fn install_index(
    root: &Path,
    target: &Path,
    index: &Path,
    original: &[u8],
    head: &str,
) -> Result<()> {
    let name = target
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("Invalid Git index path")?;
    let lock_path = target.with_file_name(format!("{name}.lock"));
    let lock = File::options()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|_| "Git index is busy; retry staging")?;
    let result = (|| {
        if fs::read(target).map_err(err)? != original || text(root, &["rev-parse", "HEAD"])? != head
        {
            return Err("The index or HEAD changed while staging; retry".into());
        }
        File::open(index).map_err(err)?.sync_all().map_err(err)?;
        fs::rename(index, target).map_err(err)
    })();
    drop(lock);
    fs::remove_file(lock_path).map_err(err)?;
    result
}
fn relative_path(prefix: &str, path: &str) -> Result<String> {
    relative(path)?;
    let path = Path::new(prefix).join(path);
    let path = path.to_str().ok_or("Invalid path")?;
    relative(path)?;
    Ok(path.into())
}

pub(super) fn action(store: &Path, w: &Workspace, args: &Value) -> Result<Value> {
    if args["action"] == "initialize" {
        return Ok(Value::Null);
    }
    let root = Path::new(&w.path);
    let search_path = args["searchPath"].as_str();
    let mut projection = Projection::new(store, w, root, search_path)?;
    match args["action"]
        .as_str()
        .ok_or("Missing publication action")?
    {
        "trees" => {
            let index = projection.project(&index_tree(root, None)?)?;
            let worktree = projection.project(&working_tree(root, search_path)?)?;
            let head = projection.project("HEAD")?;
            Ok(json!({"head":head,"index":index,"worktree":worktree}))
        }
        "stage" => {
            stage(root, &mut projection, args)?;
            Ok(Value::Null)
        }
        "discard" => {
            discard(&mut projection, args)?;
            Ok(Value::Null)
        }
        "canonicalize" => Ok(json!(canonical_head(w, &mut projection, true)?)),
        "prepare" | "pre-commit" => {
            if args["action"] == "pre-commit"
                && canonical_head(w, &mut projection, false)? != text(root, &["rev-parse", "HEAD"])?
            {
                return Err("Private history needs publication repair. Commit through native Changes to retain the private history and remove inherited edits safely.".into());
            }
            let index = args["indexFile"]
                .as_str()
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join(".git/index"));
            let index = if index.is_absolute() {
                index
            } else {
                root.join(index)
            };
            if index.parent().and_then(|p| canonical(p).ok()) != Some(root.join(".git")) {
                return Err("Commit index is outside the copy's Git directory".into());
            }
            let original = fs::read(&index).map_err(err)?;
            let head = projection.head.clone();
            if text(root, &["rev-parse", "HEAD"])? != head {
                return Err("Copy HEAD changed during commit preparation; retry".into());
            }
            let temp = Temporary::new(root)?;
            fs::write(temp.index(), &original).map_err(err)?;
            let tree = projection.project(&index_tree(root, Some(&temp.index()))?)?;
            normal_git(
                root,
                Some(&temp.index()),
                &["read-tree", &tree],
                None,
                search_path,
            )?;
            install_index(root, &index, &temp.index(), &original, &head)?;
            Ok(Value::Null)
        }
        "pre-push" => {
            for line in args["input"].as_str().unwrap_or("").lines() {
                let fields: Vec<_> = line.split_whitespace().collect();
                if fields.len() != 4 {
                    return Err("Invalid Git push receipt".into());
                }
                let oid = fields[1];
                if oid.bytes().all(|b| b == b'0') {
                    continue;
                }
                let commit = text(
                    root,
                    &["rev-parse", "--verify", &format!("{oid}^{{commit}}")],
                )?;
                let mut projection = Projection::new_at(store, w, root, search_path, commit)?;
                for commit in text(root, &["rev-list", oid, "--not", &w.head])?.lines() {
                    if git(
                        root,
                        &["merge-base", "--is-ancestor", commit, &projection.trusted],
                        None,
                    )
                    .is_ok()
                    {
                        continue;
                    }
                    let raw = text(root, &["rev-parse", &format!("{commit}^{{tree}}")])?;
                    if project_commit(w, &mut projection, commit, &raw)? != raw {
                        return Err("Push refused: private history includes inherited edits. Use the native Changes panel to publish only new edits.".into());
                    }
                }
            }
            Ok(Value::Null)
        }
        "project" => Ok(json!(
            projection.project(args["tree"].as_str().ok_or("Missing tree")?)?
        )),
        _ => Err("Unknown publication action".into()),
    }
}
