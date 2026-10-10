//! Native copy-on-write workspaces. Ownership and baselines live outside agent checkouts.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
type Result<T> = std::result::Result<T, String>;
const UNSUPPORTED_FILESYSTEM: &str = "Copy-on-write requires APFS on macOS";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub path: String,
    pub source_cwd: String,
    pub project_cwd: String,
    pub session_id: String,
    pub branch: Option<String>,
    pub head: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unpushed: Option<u64>,
    baseline: String,
    excluded: BTreeSet<String>,
    identity: (u64, u64),
    #[serde(default)]
    git_config_version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    removal_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    initial_refs: Option<BTreeMap<String, String>>,
}
impl Workspace {
    fn view(&self) -> Result<Value> {
        let git_identity = identity(&Path::new(&self.path).join(".git"))?;
        let mut value = json!({
            "id": self.id, "path": self.path, "sourceCwd": self.source_cwd,
            "projectCwd": self.project_cwd, "sessionId": self.session_id,
            "branch": self.branch, "head": self.head,
            "rootIdentity": [self.identity.0.to_string(), self.identity.1.to_string()],
            "gitIdentity": [git_identity.0.to_string(), git_identity.1.to_string()]
        });
        if let Some(dirty) = self.dirty {
            value["dirty"] = json!(dirty);
        }
        if let Some(unpushed) = self.unpushed {
            value["unpushed"] = json!(unpushed);
        }
        Ok(value)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreationIntent {
    id: String,
    path: PathBuf,
    project_cwd: PathBuf,
    identity: Option<(u64, u64)>,
    baseline_identity: Option<(u64, u64)>,
    removal_path: Option<PathBuf>,
}
fn cleanup_creation(store: &Path, intent: &mut CreationIntent) -> Result<()> {
    id_valid(&intent.id)?;
    let parent = intent.project_cwd.with_file_name(format!(
        "{}-cow",
        intent
            .project_cwd
            .file_name()
            .ok_or("Invalid creation project")?
            .to_string_lossy()
    ));
    if intent.path != parent.join(&intent.id) {
        return Err("Invalid pending creation path".into());
    }
    identity(&parent)?;
    let record = store.join(&intent.id);
    match fs::symlink_metadata(record.join("workspace.json")) {
        Ok(_) => {
            let w: Workspace = read_json(&record.join("workspace.json"))?;
            if w.id != intent.id
                || Path::new(&w.path) != intent.path
                || Some(w.identity) != intent.identity
            {
                return Err("Pending creation record was replaced".into());
            }
            // Registration completed before interruption. Leave the usable copy alone.
            return fs::remove_file(record.join("creation.json")).map_err(err);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(err(error)),
    }
    match fs::symlink_metadata(&intent.path) {
        Ok(_) => {
            if Some(identity(&intent.path)?) != intent.identity {
                return Err("Pending creation root was replaced; retained for inspection".into());
            }
            let tombstone = parent.join(format!("remove-{}", uuid::Uuid::new_v4()));
            intent.removal_path = Some(tombstone.clone());
            write_json(&record.join("creation.json"), intent)?;
            fs::rename(&intent.path, &tombstone).map_err(err)?;
            if Some(identity(&tombstone)?) != intent.identity {
                let _ = fs::rename(&tombstone, &intent.path);
                return Err("Pending creation root changed during cleanup".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(err(error)),
    }
    if let Some(tombstone) = &intent.removal_path {
        if tombstone.parent() != Some(parent.as_path())
            || !tombstone
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix("remove-"))
                .is_some_and(|id| id_valid(id).is_ok())
        {
            return Err("Invalid pending creation removal path".into());
        }
        match fs::symlink_metadata(tombstone) {
            Ok(_) => {
                if Some(identity(tombstone)?) != intent.identity {
                    return Err("Pending creation cleanup root was replaced".into());
                }
                fs::remove_dir_all(tombstone).map_err(err)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(err(error)),
        }
    }
    let baseline = parent.join(".baselines").join(&intent.id);
    match fs::symlink_metadata(&baseline) {
        Ok(_) => {
            identity(&parent.join(".baselines"))?;
            if Some(identity(&baseline)?) != intent.baseline_identity {
                return Err("Pending creation baseline was replaced".into());
            }
            fs::remove_dir_all(baseline).map_err(err)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(err(error)),
    }
    fs::remove_dir_all(record).map_err(err)
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn canonical(p: &Path) -> Result<PathBuf> {
    p.canonicalize().map_err(err)
}
fn relative(p: &str) -> Result<()> {
    if p.is_empty()
        || p.contains('\0')
        || Path::new(p)
            .components()
            .any(|x| !matches!(x, Component::Normal(_)))
    {
        return Err("Invalid project-relative path".into());
    }
    Ok(())
}
fn id_valid(id: &str) -> Result<()> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| "Invalid isolation ID".into())
}
#[cfg(unix)]
fn identity(p: &Path) -> Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::symlink_metadata(p).map_err(err)?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err("Isolation root was replaced".into());
    }
    Ok((m.dev(), m.ino()))
}
#[cfg(not(unix))]
fn identity(_: &Path) -> Result<(u64, u64)> {
    Err(UNSUPPORTED_FILESYSTEM.into())
}
fn private_dir(p: &Path) -> Result<()> {
    fs::create_dir_all(p).map_err(err)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let m = fs::symlink_metadata(p).map_err(err)?;
        if !m.is_dir() || m.file_type().is_symlink() || m.uid() != unsafe { libc::geteuid() } {
            return Err("Isolation storage must be an owned directory".into());
        }
        fs::set_permissions(p, fs::Permissions::from_mode(0o700)).map_err(err)?;
    }
    Ok(())
}
fn write_json(p: &Path, v: &impl Serialize) -> Result<()> {
    let temp = p.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut f = File::options()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(err)?;
    serde_json::to_writer(&mut f, v).map_err(err)?;
    f.sync_all().map_err(err)?;
    fs::rename(temp, p).map_err(err)?;
    // Persist the rename too, not just the temporary file's contents.
    File::open(p.parent().ok_or("Missing JSON parent")?)
        .map_err(err)?
        .sync_all()
        .map_err(err)
}
fn read_json<T: serde::de::DeserializeOwned>(p: &Path) -> Result<T> {
    let parent = p.parent().ok_or("Missing registry parent")?;
    let metadata = fs::symlink_metadata(parent).map_err(err)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Registry directory was replaced".into());
    }
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err("Registry directory has a different owner".into());
        }
        File::options()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(p)
            .map_err(err)?
    };
    #[cfg(not(unix))]
    let file = File::open(p).map_err(err)?;
    serde_json::from_reader(file).map_err(err)
}
fn git_base(root: &Path) -> Command {
    let mut c = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            c.env_remove(key);
        }
    }
    c.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0");
    c.arg("-C").arg(root).args([
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "diff.external=",
        "-c",
        "core.attributesFile=/dev/null",
        "-c",
        "core.sshCommand=false",
        "-c",
        "protocol.allow=never",
        "-c",
        "protocol.file.allow=always",
    ]);
    c
}
fn git_command(root: &Path) -> Command {
    let mut c = git_base(root);
    // Git status/reset can invoke clean/smudge/process filters from local attributes.
    // Read names only, then override every executable filter before any file operation.
    if let Ok(output) = git_base(root)
        .args([
            "config",
            "--includes",
            "--local",
            "--null",
            "--name-only",
            "--get-regexp",
            "^filter\\.",
        ])
        .output()
    {
        let mut names = BTreeSet::new();
        for key in output.stdout.split(|b| *b == 0).filter(|b| !b.is_empty()) {
            if let Ok(key) = std::str::from_utf8(key) {
                if let Some((name, _)) = key.rsplit_once('.') {
                    names.insert(name.to_string());
                }
            }
        }
        for name in names {
            for setting in ["clean=", "smudge=", "process=", "required=false"] {
                c.arg("-c").arg(format!("{name}.{setting}"));
            }
        }
    }
    c
}
fn git(root: &Path, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut c = git_command(root);
    c.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if input.is_some() {
        c.stdin(Stdio::piped());
    }
    let mut child = c.spawn().map_err(err)?;
    if let Some(bytes) = input {
        let mut stdin = child.stdin.take().ok_or("Git stdin missing")?;
        stdin.write_all(bytes).map_err(err)?;
    }
    let out = child.wait_with_output().map_err(err)?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(out.stdout)
}
fn text(root: &Path, args: &[&str]) -> Result<String> {
    String::from_utf8(git(root, args, None)?)
        .map(|s| s.trim().to_string())
        .map_err(err)
}
fn repo(p: &Path) -> Result<PathBuf> {
    let root = canonical(p)?;
    let top = text(&root, &["rev-parse", "--show-toplevel"])?;
    if canonical(Path::new(&top))? != root {
        return Err("Select the repository root for copy-on-write".into());
    }
    Ok(root)
}
fn paths(root: &Path) -> Result<BTreeSet<String>> {
    git_paths(root, &["--cached", "--others", "--exclude-standard"])
}
fn ignored_paths(root: &Path) -> Result<BTreeSet<String>> {
    git_paths(root, &["--others", "--ignored", "--exclude-standard"])
}
fn git_paths(root: &Path, options: &[&str]) -> Result<BTreeSet<String>> {
    let mut args = vec!["ls-files", "-z"];
    args.extend_from_slice(options);
    let output = ignore_command(root)?.args(args).output().map_err(err)?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    let bytes = output.stdout;
    let mut out = BTreeSet::new();
    for b in bytes.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        let p = String::from_utf8(b.to_vec()).map_err(|_| "Non-UTF-8 paths are unsupported")?;
        relative(&p)?;
        out.insert(p);
    }
    Ok(out)
}
fn ignore_command(root: &Path) -> Result<Command> {
    // Preserve effective ignore rules without enabling executable global Git settings.
    let output = config_reader(root)
        .args([
            "config",
            "--includes",
            "--path",
            "--null",
            "--get",
            "core.excludesFile",
        ])
        .output()
        .map_err(err)?;
    let mut command = git_base(root);
    match output.status.code() {
        Some(0) => {
            let value = String::from_utf8(output.stdout).map_err(err)?;
            command.arg("-c").arg(format!(
                "core.excludesFile={}",
                value.trim_end_matches('\0')
            ));
        }
        Some(1) => {}
        _ => return Err("Cannot read Git ignore configuration".into()),
    }
    Ok(command)
}
#[cfg(unix)]
fn ignored_socket(root: &Path, path: &Path) -> Result<bool> {
    let relative = path.strip_prefix(root).map_err(err)?;
    // Runtime sockets cannot be cloned or used by the isolated process. Respect
    // Git's index: an ignored pattern must never hide a tracked file replacement.
    let output = ignore_command(root)?
        .args(["check-ignore", "--quiet", "--"])
        .arg(relative)
        .output()
        .map_err(err)?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(String::from_utf8_lossy(&output.stderr).to_string()),
    }
}
#[cfg(unix)]
fn tracked_path(root: &Path, relative: &str) -> Result<bool> {
    let output = git_base(root)
        .arg("--literal-pathspecs")
        .args(["ls-files", "--error-unmatch", "--", relative])
        .output()
        .map_err(err)?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(String::from_utf8_lossy(&output.stderr).to_string()),
    }
}
fn validate_repo(root: &Path) -> Result<()> {
    text(root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    if !git(root, &["ls-files", "-u"], None)?.is_empty() {
        return Err("Resolve index conflicts before cloning".into());
    }
    if git(root, &["ls-files", "-v", "-z"], None)?
        .split(|b| *b == 0)
        .any(|record| {
            record
                .first()
                .is_some_and(|b| b.is_ascii_lowercase() || *b == b'S')
        })
    {
        return Err("Skip-worktree and assume-unchanged index entries are unsupported; clear their flags before using copy-on-write".into());
    }
    for args in [
        &["config", "--get", "core.sparseCheckout"][..],
        &["config", "--get", "extensions.partialClone"][..],
        &["rev-parse", "--shared-index-path"][..],
    ] {
        if text(root, args).is_ok_and(|s| !s.is_empty() && s != "false") {
            return Err("Sparse, split-index and partial repositories are unsupported".into());
        }
    }
    if text(root, &["rev-parse", "--is-shallow-repository"])? == "true" {
        return Err("Shallow repositories are unsupported".into());
    }
    let common = PathBuf::from(text(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);
    if common.join("objects/info/alternates").exists() {
        return Err("Git object alternates are unsupported".into());
    }
    let stage = git(root, &["ls-files", "--stage"], None)?;
    if String::from_utf8_lossy(&stage)
        .lines()
        .any(|s| s.starts_with("160000 "))
    {
        return Err("Submodules are unsupported".into());
    }
    Ok(())
}
// Platform clone primitive; filesystem validation lives in check_filesystem.
#[cfg(target_os = "macos")]
fn clone_file(source: &File, parent: &File, name: &std::ffi::CStr) -> Result<()> {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn fclonefileat(src: i32, dst: i32, name: *const libc::c_char, flags: u32) -> i32;
    }
    if unsafe { fclonefileat(source.as_raw_fd(), parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(format!(
            "Native APFS clone failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
#[cfg(all(unix, not(target_os = "macos")))]
fn clone_file(_: &File, _: &File, _: &std::ffi::CStr) -> Result<()> {
    Err(UNSUPPORTED_FILESYSTEM.into())
}
#[cfg(unix)]
fn open_dir(p: &Path) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(p)
        .map_err(err)
}
fn relative_link(parent: &Path, target: &Path) -> PathBuf {
    let from = parent.components().collect::<Vec<_>>();
    let to = target.components().collect::<Vec<_>>();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut path = PathBuf::new();
    for _ in common..from.len() {
        path.push("..");
    }
    for component in &to[common..] {
        path.push(component.as_os_str());
    }
    path
}
#[cfg(unix)]
fn clone_tree(source: &Path, dest: &Path, exclude_git: bool) -> Result<()> {
    clone_tree_inner(
        source,
        dest,
        exclude_git,
        &BTreeSet::from([canonical(source)?]),
        false,
        identity(source)?,
    )
}
#[cfg(unix)]
fn clone_external(source: &Path, dest: &Path, ancestors: &BTreeSet<PathBuf>) -> Result<()> {
    use std::os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, PermissionsExt},
    };
    if ancestors.contains(source) || ancestors.len() >= 32 {
        return Err(format!(
            "External symlink cycle or excessive nesting: {}",
            source.display()
        ));
    }
    check_filesystem(source)?;
    let metadata = fs::symlink_metadata(source).map_err(err)?;
    if metadata.is_dir() {
        for root in ancestors {
            for ancestor in root.ancestors() {
                let m = fs::metadata(ancestor).map_err(err)?;
                if (m.dev(), m.ino()) == (metadata.dev(), metadata.ino()) {
                    return Err(format!(
                        "External symlink cycle or ancestor target: {}",
                        source.display()
                    ));
                }
            }
        }
        let mut ancestors = ancestors.clone();
        ancestors.insert(source.to_path_buf());
        clone_tree_inner(
            source,
            dest,
            false,
            &ancestors,
            true,
            (metadata.dev(), metadata.ino()),
        )
    } else if metadata.is_file() {
        let path = source
            .strip_prefix("/")
            .map_err(err)?
            .to_str()
            .ok_or("Non-UTF-8 path")?;
        let file = open_regular(Path::new("/"), path)?;
        let opened = file.metadata().map_err(err)?;
        if (metadata.dev(), metadata.ino()) != (opened.dev(), opened.ino()) {
            return Err("External target replaced while cloning; retry".into());
        }
        let parent = open_dir(dest.parent().ok_or("Missing clone parent")?)?;
        let name = std::ffi::CString::new(dest.file_name().ok_or("Missing clone name")?.as_bytes())
            .map_err(err)?;
        clone_file(&file, &parent, &name)?;
        fs::set_permissions(dest, fs::Permissions::from_mode(opened.mode() & 0o777)).map_err(err)
    } else {
        Err(format!(
            "Unsupported external symlink target: {}",
            source.display()
        ))
    }
}
#[cfg(unix)]
fn clone_tree_inner(
    source: &Path,
    dest: &Path,
    exclude_git: bool,
    ancestors: &BTreeSet<PathBuf>,
    materialized: bool,
    expected_identity: (u64, u64),
) -> Result<()> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    struct Context<'a> {
        root: &'a Path,
        out: &'a Path,
        root_dev: u64,
        eligible: Option<&'a BTreeSet<String>>,
        ancestors: &'a BTreeSet<PathBuf>,
        materialized: bool,
    }
    #[allow(clippy::unnecessary_cast)] // stat field widths differ across platforms.
    fn walk(src: &Path, dst: &Path, source_fd: &File, context: &Context<'_>) -> Result<()> {
        let Context {
            root,
            out,
            root_dev,
            eligible,
            ancestors,
            materialized,
        } = *context;
        let dest_fd = open_dir(dst)?;
        for entry in fs::read_dir(src).map_err(err)? {
            let entry = entry.map_err(err)?;
            let name = entry.file_name();
            if name == ".git" {
                if src == root && eligible.is_some() {
                    continue;
                }
                if eligible.is_some() || materialized {
                    return Err(format!("Nested Git repository: {}", entry.path().display()));
                }
            }
            use std::os::unix::ffi::OsStrExt;
            let cname = std::ffi::CString::new(name.as_bytes()).map_err(err)?;
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(err)?
                .to_str()
                .ok_or("Non-UTF-8 path")?
                .to_string();
            let runtime = materialized || eligible.is_some_and(|paths| !paths.contains(&relative));
            let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
            if unsafe {
                libc::fstatat(
                    source_fd.as_raw_fd(),
                    cname.as_ptr(),
                    stat.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } != 0
            {
                let error = std::io::Error::last_os_error();
                if runtime && error.kind() == std::io::ErrorKind::NotFound {
                    continue;
                }
                return Err(err(error));
            }
            let stat = unsafe { stat.assume_init() };
            if stat.st_dev as u64 != root_dev {
                return Err("Cross-filesystem content is unsupported".into());
            }
            let target = dst.join(&name);
            let kind = stat.st_mode & libc::S_IFMT;
            if kind == libc::S_IFSOCK && eligible.is_some() && ignored_socket(root, &entry.path())?
            {
                continue;
            }
            if kind == libc::S_IFLNK {
                let mut buffer = vec![0u8; 65536];
                let length = unsafe {
                    libc::readlinkat(
                        source_fd.as_raw_fd(),
                        cname.as_ptr(),
                        buffer.as_mut_ptr().cast(),
                        buffer.len(),
                    )
                };
                if length < 0 {
                    let error = std::io::Error::last_os_error();
                    if runtime && error.kind() == std::io::ErrorKind::NotFound {
                        continue;
                    }
                    return Err(err(error));
                }
                if length as usize == buffer.len() {
                    return Err("Symlink target is too long".into());
                }
                buffer.truncate(length as usize);
                use std::os::unix::ffi::OsStringExt;
                let link = PathBuf::from(std::ffi::OsString::from_vec(buffer));
                let resolved = match fs::canonicalize(src.join(&link)) {
                    Ok(path) => path,
                    Err(error) if runtime && error.kind() == std::io::ErrorKind::NotFound => {
                        continue
                    }
                    Err(error) => {
                        return Err(format!("Cannot resolve symlink {relative}: {error}"))
                    }
                };
                if !resolved.starts_with(root) {
                    if !materialized && (eligible.is_none() || tracked_path(root, &relative)?) {
                        return Err(format!(
                            "Tracked or unsupported external symlink: {relative}"
                        ));
                    }
                    clone_external(&resolved, &target, ancestors)?;
                    continue;
                }
                let link = if link.is_absolute() {
                    relative_link(
                        dst.strip_prefix(out).map_err(err)?,
                        resolved.strip_prefix(root).map_err(err)?,
                    )
                } else {
                    link
                };
                std::os::unix::fs::symlink(link, target).map_err(err)?;
            } else if kind == libc::S_IFDIR || kind == libc::S_IFREG {
                let flags = libc::O_RDONLY
                    | libc::O_NONBLOCK
                    | libc::O_NOFOLLOW
                    | if kind == libc::S_IFDIR {
                        libc::O_DIRECTORY
                    } else {
                        0
                    };
                let fd = unsafe { libc::openat(source_fd.as_raw_fd(), cname.as_ptr(), flags) };
                if fd < 0 {
                    let error = std::io::Error::last_os_error();
                    if runtime && error.kind() == std::io::ErrorKind::NotFound {
                        continue;
                    }
                    return Err(err(error));
                }
                let file = unsafe { File::from_raw_fd(fd) };
                let before = file.metadata().map_err(err)?;
                if before.mode() & libc::S_IFMT as u32 != kind as u32 {
                    return Err(format!(
                        "Source file type changed while cloning: {relative}; retry"
                    ));
                }
                if before.ino() != stat.st_ino as u64 || before.dev() != stat.st_dev as u64 {
                    if runtime && kind == libc::S_IFREG {
                        continue;
                    }
                    return Err(format!("Source changed while cloning: {relative}; retry"));
                }
                if kind == libc::S_IFDIR {
                    private_dir(&target)?;
                    walk(&entry.path(), &target, &file, context)?;
                } else {
                    if target.exists() {
                        if file_digest(&entry.path())? != file_digest(&target)? {
                            return Err("Conflicting Git object storage".into());
                        }
                    } else {
                        clone_file(&file, &dest_fd, &cname)?;
                    }
                    fs::set_permissions(&target, fs::Permissions::from_mode(before.mode() & 0o777))
                        .map_err(err)?;
                }
                let after = file.metadata().map_err(err)?;
                if kind == libc::S_IFREG
                    && !runtime
                    && (before.len() != after.len()
                        || before.mtime() != after.mtime()
                        || before.mtime_nsec() != after.mtime_nsec()
                        || before.ctime() != after.ctime()
                        || before.ctime_nsec() != after.ctime_nsec())
                {
                    return Err(format!("Source changed while cloning: {relative}; retry"));
                }
                if kind == libc::S_IFDIR && identity(&entry.path())? != (before.dev(), before.ino())
                {
                    return Err(format!(
                        "Source directory replaced while cloning: {relative}; retry"
                    ));
                }
            } else {
                return Err(format!(
                    "Unsupported special file: {}",
                    entry.path().display()
                ));
            }
        }
        let metadata = source_fd.metadata().map_err(err)?;
        if identity(src)? != (metadata.dev(), metadata.ino()) {
            return Err(format!(
                "Source directory replaced while cloning: {}; retry",
                src.display()
            ));
        }
        Ok(())
    }
    let eligible = if exclude_git {
        Some(paths(source)?)
    } else {
        None
    };
    private_dir(dest)?;
    let fd = open_dir(source)?;
    let opened = fd.metadata().map_err(err)?;
    if (opened.dev(), opened.ino()) != expected_identity {
        return Err(format!(
            "Source directory replaced while cloning: {}; retry",
            source.display()
        ));
    }
    let context = Context {
        root: source,
        out: dest,
        root_dev: fd.metadata().map_err(err)?.dev(),
        eligible: eligible.as_ref(),
        ancestors,
        materialized,
    };
    walk(source, dest, &fd, &context)
}
#[cfg(not(unix))]
fn clone_tree(_: &Path, _: &Path, _: bool) -> Result<()> {
    Err(UNSUPPORTED_FILESYSTEM.into())
}
#[cfg(target_os = "macos")]
fn check_filesystem(root: &Path) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(root.as_os_str().as_bytes()).map_err(err)?;
    let mut s = std::mem::MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::statfs(c.as_ptr(), s.as_mut_ptr()) } != 0 {
        return Err(err(std::io::Error::last_os_error()));
    }
    let s = unsafe { s.assume_init() };
    let name = unsafe { std::ffi::CStr::from_ptr(s.f_fstypename.as_ptr()) };
    if name.to_bytes() != b"apfs" {
        return Err(UNSUPPORTED_FILESYSTEM.into());
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn check_filesystem(_: &Path) -> Result<()> {
    Err(UNSUPPORTED_FILESYSTEM.into())
}
fn capability(cwd: &Path) -> Value {
    let result: Result<()> = (|| {
        let root = repo(cwd)?;
        validate_repo(&root)?;
        check_filesystem(&root)?;
        Ok(())
    })();
    match result {
        Ok(()) => json!({"supported":true}),
        Err(reason) => json!({"supported":false,"reason":reason}),
    }
}
fn init_repo(root: &Path, format: &str) -> Result<()> {
    private_dir(root)?;
    text(
        root,
        &[
            "init",
            "--quiet",
            "--template=",
            &format!("--object-format={format}"),
        ],
    )?;
    Ok(())
}
fn private_git(source: &Path, dest: &Path, head: &str, branch: &str) -> Result<()> {
    let format = text(source, &["rev-parse", "--show-object-format"])?;
    init_repo(dest, &format)?;
    let common = PathBuf::from(text(
        source,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);
    clone_tree(&common.join("objects"), &dest.join(".git/objects"), false)?;
    copy_git_rules(&common, dest, false)?;
    // Copy refs as Git metadata, never shared worktree registrations or pointers.
    let refs = text(
        source,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(objectname)%00%(symref)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    )?;
    for line in refs.lines() {
        let fields = line.split('\0').collect::<Vec<_>>();
        if fields.len() != 3 {
            return Err("Invalid Git reference metadata".into());
        }
        if fields[2].is_empty() {
            text(dest, &["update-ref", fields[0], fields[1]])?;
        } else {
            text(dest, &["symbolic-ref", fields[0], fields[2]])?;
        }
    }
    text(dest, &["update-ref", &format!("refs/heads/{branch}"), head])?;
    text(
        dest,
        &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")],
    )?;
    text(dest, &["read-tree", head])?;
    copy_git_configuration(source, dest, branch, false)?;
    Ok(())
}
fn copy_git_rules(common: &Path, dest: &Path, migrate: bool) -> Result<()> {
    #[cfg(not(unix))]
    let _ = (common, dest, migrate);
    // Preserve repository-local ignore/attribute rules without sharing Git metadata.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["exclude", "attributes"] {
            let rule = common.join("info").join(name);
            if migrate
                && dest
                    .join(".git/info")
                    .join(name)
                    .try_exists()
                    .map_err(err)?
            {
                continue;
            }
            match fs::symlink_metadata(&rule) {
                Ok(metadata) => {
                    if !metadata.is_file() || metadata.file_type().is_symlink() {
                        return Err("Git ignore and attribute rules must be regular files".into());
                    }
                    let directory = dest.join(".git/info");
                    private_dir(&directory)?;
                    clone_file(
                        &open_regular(common, &format!("info/{name}"))?,
                        &open_dir(&directory)?,
                        &std::ffi::CString::new(name).map_err(err)?,
                    )?;
                    fs::set_permissions(
                        directory.join(name),
                        fs::Permissions::from_mode(metadata.permissions().mode() & 0o777),
                    )
                    .map_err(err)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(err(error)),
            }
        }
    }
    Ok(())
}
fn config_reader(root: &Path) -> Command {
    let mut command = Command::new("git");
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(key);
    }
    command.arg("-C").arg(root);
    command
}
fn config_values(root: &Path, key: &str, local: bool) -> Result<Vec<String>> {
    let mut reader = config_reader(root);
    reader.args(["config", "--includes", "--null"]);
    if local {
        reader.arg("--local");
    }
    let output = reader.args(["--get-all", key]).output().map_err(err)?;
    if !output.status.success() && output.status.code() != Some(1) {
        return Err("Cannot read Git configuration".into());
    }
    Ok(String::from_utf8(output.stdout)
        .map_err(err)?
        .split_terminator('\0')
        .map(str::to_string)
        .collect())
}
fn copy_git_configuration(source: &Path, dest: &Path, branch: &str, migrate: bool) -> Result<()> {
    // Metadata-only read: includes are resolved for the originating Git directory;
    // no credential helper, SSH command, hook, or network request runs here.
    let output = config_reader(source).args(["config", "--includes", "--null", "--get-regexp", r"^(user\.(name|email|signingkey)|commit\.gpgsign|tag\.gpgsign|gpg(\.(openpgp|x509|ssh))?\.(format|program|allowedsignersfile|defaultkeycommand)|core\.(sshcommand|autocrlf|eol|safecrlf|filemode|ignorecase|checkstat|trustctime|precomposeunicode|symlinks|excludesfile|attributesfile)|filter\..*\.(clean|smudge|process|required)|credential(\..*)?\.(helper|usehttppath|username)|remote\..*\.(url|pushurl|fetch|tagopt|prune)|branch\..*\.(remote|merge|pushremote|rebase))$"]).output().map_err(err)?;
    if !output.status.success() && output.status.code() != Some(1) {
        return Err("Cannot read source Git configuration".into());
    }
    let mut configuration: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for field in output.stdout.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        let field =
            std::str::from_utf8(field).map_err(|_| "Non-UTF-8 Git configuration is unsupported")?;
        let (key, value) = field.split_once('\n').ok_or("Invalid Git configuration")?;
        configuration
            .entry(key.to_string())
            .or_default()
            .push(value.to_string());
    }
    let mut remotes = BTreeSet::new();
    for (key, values) in &configuration {
        if key.starts_with(&format!("branch.{branch}.")) {
            continue;
        }
        let auth = key == "core.sshcommand" || key.starts_with("credential.");
        let conversion =
            key.starts_with("filter.") || key.starts_with("core.") || key.starts_with("gpg.");
        if key.starts_with("remote.") && (key.ends_with(".url") || key.ends_with(".pushurl")) {
            let remote = key
                .strip_prefix("remote.")
                .and_then(|name| name.rsplit_once('.').map(|(remote, _)| remote))
                .ok_or("Invalid remote configuration")?;
            if !remotes.insert(remote.to_string()) {
                continue;
            }
            for push in [false, true] {
                let target_key =
                    format!("remote.{remote}.{}", if push { "pushurl" } else { "url" });
                if migrate {
                    let local = config_values(dest, &target_key, true)?;
                    let original = configuration
                        .get(&target_key)
                        .or_else(|| configuration.get(&format!("remote.{remote}.url")));
                    if local.is_empty()
                        || Some(&local) != original
                        || local
                            .iter()
                            .any(|url| url.contains(':') || Path::new(url).is_absolute())
                    {
                        continue;
                    }
                }
                let mut reader = config_reader(source);
                reader.args(["remote", "get-url"]);
                if push {
                    reader.arg("--push");
                }
                let output = reader.args(["--all", remote]).output().map_err(err)?;
                if !output.status.success() {
                    return Err("Cannot resolve source Git remote".into());
                }
                let urls = String::from_utf8(output.stdout).map_err(err)?;
                if migrate {
                    let _ = git(dest, &["config", "--unset-all", &target_key], None);
                }
                for url in urls.lines() {
                    let url = if !url.contains(':') && !Path::new(url).is_absolute() {
                        source.join(url).to_string_lossy().into_owned()
                    } else {
                        url.to_string()
                    };
                    text(dest, &["config", "--add", &target_key, &url])?;
                }
            }
            continue;
        }
        if migrate && !auth && !conversion {
            continue;
        }
        if migrate && !config_values(dest, key, true)?.is_empty() {
            continue;
        }
        if auth || conversion {
            if config_values(dest, key, false)? == *values {
                continue;
            }
            if key.ends_with(".helper") {
                text(dest, &["config", "--add", key, ""])?;
            }
        }
        if key.ends_with(".helper") || key.ends_with(".fetch") {
            for value in values {
                text(dest, &["config", "--add", key, value])?;
            }
        } else if let Some(value) = values.last() {
            let value = if ["core.excludesfile", "core.attributesfile"].contains(&key.as_str()) {
                // --path expands ~ and config-relative path syntax without executing Git helpers.
                let path = config_reader(source)
                    .args(["config", "--includes", "--null", "--path", "--get", key])
                    .output()
                    .map_err(err)?;
                if !path.status.success() {
                    return Err("Cannot resolve Git rule path".into());
                }
                let path = String::from_utf8(path.stdout).map_err(err)?;
                let path = Path::new(path.trim_end_matches('\0'));
                let path = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    source.join(path)
                };
                let resolved = path.canonicalize().unwrap_or(path);
                // Repository-owned rules follow the isolated checkout; external
                // rule files remain in their existing user-configured location.
                resolved
                    .strip_prefix(source)
                    .map(|relative| dest.join(relative))
                    .unwrap_or(resolved)
                    .to_string_lossy()
                    .into_owned()
            } else {
                value.clone()
            };
            text(dest, &["config", "--replace-all", key, &value])?;
        }
    }
    Ok(())
}
fn upgrade_git_configuration(store: &Path, w: &mut Workspace) -> Result<()> {
    if w.git_config_version >= 2 {
        return Ok(());
    }
    let root = Path::new(&w.path);
    identity(&root.join(".git"))?;
    let common = text(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    if Path::new(&common) != root.join(".git") {
        return Err("Isolation Git directory was replaced".into());
    }
    let source = repo(Path::new(&w.source_cwd))?;
    copy_git_configuration(&source, root, w.branch.as_deref().unwrap_or(""), true)?;
    let common = PathBuf::from(text(
        &source,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);
    copy_git_rules(&common, root, true)?;
    w.git_config_version = 2;
    write_json(&store.join(&w.id).join("workspace.json"), w)
}
fn checked_file(root: &Path, p: &str) -> Result<PathBuf> {
    relative(p)?;
    let target = root.join(p);
    let mut parent = target.parent().ok_or("Invalid path")?;
    while parent != root {
        if fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!("Symlink ancestor: {p}"));
        }
        parent = parent.parent().ok_or("Path escaped checkout")?;
    }
    Ok(target)
}
#[cfg(unix)]
fn open_regular(root: &Path, relative_path: &str) -> Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    relative(relative_path)?;
    let components: Vec<_> = Path::new(relative_path).components().collect();
    let mut parent = open_dir(root)?;
    for (index, component) in components.iter().enumerate() {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(component.as_os_str().as_bytes()).map_err(err)?;
        let directory = index + 1 < components.len();
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY
                    | libc::O_NONBLOCK
                    | libc::O_NOFOLLOW
                    | if directory { libc::O_DIRECTORY } else { 0 },
            )
        };
        if fd < 0 {
            return Err(err(std::io::Error::last_os_error()));
        }
        parent = unsafe { File::from_raw_fd(fd) };
    }
    if !parent.metadata().map_err(err)?.is_file() {
        return Err("Not a regular file".into());
    }
    Ok(parent)
}
#[cfg(not(unix))]
fn open_regular(root: &Path, p: &str) -> Result<File> {
    File::open(checked_file(root, p)?).map_err(err)
}
fn blob(root: &Path, p: &str) -> Result<Option<(String, String)>> {
    let target = checked_file(root, p)?;
    let m = match fs::symlink_metadata(&target) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(err(e)),
    };
    if m.file_type().is_symlink() {
        let resolved = canonical(&target)?;
        if !resolved.starts_with(root) {
            return Err(format!("External symlink: {p}"));
        }
        let link = fs::read_link(&target).map_err(err)?;
        let link = if link.is_absolute() {
            relative_link(
                target
                    .parent()
                    .ok_or("Missing symlink parent")?
                    .strip_prefix(root)
                    .map_err(err)?,
                resolved.strip_prefix(root).map_err(err)?,
            )
        } else {
            link
        };
        return Ok(Some((
            "120000".into(),
            link.to_str().ok_or("Non-UTF-8 symlink target")?.to_string(),
        )));
    }
    if !m.is_file() {
        return Err(format!("Unsupported changed file: {p}"));
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        if m.permissions().mode() & 0o111 != 0 {
            "100755"
        } else {
            "100644"
        }
    };
    #[cfg(not(unix))]
    let mode = "100644";
    Ok(Some((mode.into(), target.to_string_lossy().into_owned())))
}
fn tree(
    objects: &Path,
    root: &Path,
    eligible: &BTreeSet<String>,
    excluded: &BTreeSet<String>,
) -> Result<String> {
    let index = objects.join(format!("index-{}", uuid::Uuid::new_v4()));
    let mut c = git_base(objects);
    c.env("GIT_INDEX_FILE", &index)
        .args(["read-tree", "--empty"]);
    if !c.output().map_err(err)?.status.success() {
        return Err("Cannot initialize snapshot index".into());
    }
    let result = (|| {
        // One raw blob import process handles all files and symlinks. Paths never
        // reach Git: regular files are opened through the existing no-follow walk.
        let marks = objects.join(format!("marks-{}", uuid::Uuid::new_v4()));
        let mut importer = git_base(objects)
            .args(["fast-import", "--quiet", "--done"])
            .arg(format!("--export-marks={}", marks.display()))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(err)?;
        let captured: Result<Vec<(String, &String)>> = (|| {
            let mut input = importer.stdin.take().ok_or("Missing blob input")?;
            let mut captured = Vec::new();
            for p in eligible.difference(excluded) {
                let Some((mode, data)) = blob(root, p)? else {
                    continue;
                };
                let mark = captured.len() + 1;
                if mode == "120000" {
                    write!(input, "blob\nmark :{mark}\ndata {}\n", data.len()).map_err(err)?;
                    input.write_all(data.as_bytes()).map_err(err)?;
                } else {
                    let mut file = open_regular(root, p)?;
                    let before = file.metadata().map_err(err)?;
                    write!(input, "blob\nmark :{mark}\ndata {}\n", before.len()).map_err(err)?;
                    let copied = std::io::copy(
                        &mut std::io::Read::by_ref(&mut file).take(before.len()),
                        &mut input,
                    )
                    .map_err(err)?;
                    if copied != before.len()
                        || file.metadata().map_err(err)?.modified().map_err(err)?
                            != before.modified().map_err(err)?
                    {
                        return Err("File changed during snapshot capture; retry".into());
                    }
                }
                input.write_all(b"\n").map_err(err)?;
                captured.push((mode, p));
            }
            input.write_all(b"done\n").map_err(err)?;
            Ok(captured)
        })();
        // Closing stdin lets a failed import terminate too; reap before returning.
        let imported = importer.wait_with_output().map_err(err)?;
        let marks_data = fs::read_to_string(&marks).map_err(err);
        let _ = fs::remove_file(&marks);
        let captured: Vec<(String, &String)> = captured?;
        if !imported.status.success() {
            return Err(String::from_utf8_lossy(&imported.stderr).into_owned());
        }
        let mut oids = BTreeMap::new();
        for line in marks_data?.lines() {
            let (mark, oid) = line.split_once(' ').ok_or("Invalid blob import marks")?;
            oids.insert(mark.to_string(), oid.to_string());
        }
        let mut entries = Vec::new();
        for (index, (mode, p)) in captured.iter().enumerate() {
            let oid = oids
                .get(&format!(":{}", index + 1))
                .ok_or("Missing imported blob")?;
            entries.extend_from_slice(format!("{mode} {oid}\t{p}\0").as_bytes());
        }
        let mut child = git_base(objects)
            .env("GIT_INDEX_FILE", &index)
            .args(["update-index", "-z", "--index-info"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(err)?;
        child
            .stdin
            .take()
            .ok_or("Missing index input")?
            .write_all(&entries)
            .map_err(err)?;
        let updated = child.wait_with_output().map_err(err)?;
        if !updated.status.success() {
            return Err(String::from_utf8_lossy(&updated.stderr).into_owned());
        }
        let out = git_base(objects)
            .env("GIT_INDEX_FILE", &index)
            .arg("write-tree")
            .output()
            .map_err(err)?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
        String::from_utf8(out.stdout)
            .map(|s| s.trim().to_string())
            .map_err(err)
    })();
    let _ = fs::remove_file(index);
    result
}
#[cfg(unix)]
fn file_digest(p: &Path) -> Result<String> {
    let mut h = Sha256::new();
    let mut f = File::open(p).map_err(err)?;
    let mut b = [0; 65536];
    loop {
        let n = f.read(&mut b).map_err(err)?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
#[derive(Debug, PartialEq, Eq)]
struct Fingerprint {
    identity: (u64, u64),
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
    mode: u32,
    link: Option<String>,
    external: bool,
}
#[cfg(unix)]
fn fingerprint(root: &Path, eligible: &BTreeSet<String>) -> Result<BTreeMap<String, Fingerprint>> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    fn walk(
        root: &Path,
        p: &Path,
        eligible: &BTreeSet<String>,
        out: &mut BTreeMap<String, Fingerprint>,
    ) -> Result<()> {
        let dir = open_dir(p)?;
        let directory_identity = (
            dir.metadata().map_err(err)?.dev(),
            dir.metadata().map_err(err)?.ino(),
        );
        for e in fs::read_dir(p).map_err(err)? {
            let e = e.map_err(err)?;
            if p == root && e.file_name() == ".git" {
                continue;
            }
            let path = e.path();
            let rel = path
                .strip_prefix(root)
                .map_err(err)?
                .to_str()
                .ok_or("Non-UTF-8 path")?
                .to_string();
            {
                let prefix = format!("{rel}/");
                if !eligible.contains(&rel)
                    && !eligible
                        .range(prefix.clone()..)
                        .next()
                        .is_some_and(|p| p.starts_with(&prefix))
                {
                    continue;
                }
            }
            checked_file(root, &rel)?;
            let mut m = fs::symlink_metadata(&path).map_err(err)?;
            if m.is_dir() {
                walk(root, &path, eligible, out)?;
                continue;
            }
            let mut external = false;
            let link = if m.file_type().is_symlink() {
                external = !canonical(&path)?.starts_with(root);
                if external {
                    if tracked_path(root, &rel)? {
                        return Err(format!("Tracked external symlink: {rel}"));
                    }
                    Some(
                        fs::read_link(&path)
                            .map_err(err)?
                            .to_str()
                            .ok_or("Non-UTF-8 path")?
                            .to_string(),
                    )
                } else {
                    Some(
                        blob(root, &rel)?
                            .ok_or("Symlink disappeared during capture")?
                            .1,
                    )
                }
            } else if m.is_file() {
                let file = open_regular(root, &rel)?;
                let opened = file.metadata().map_err(err)?;
                if (m.dev(), m.ino()) != (opened.dev(), opened.ino()) {
                    return Err(format!(
                        "Source changed during metadata capture: {rel}; retry"
                    ));
                }
                m = opened;
                None
            } else {
                if m.file_type().is_socket() && ignored_socket(root, &path)? {
                    continue;
                }
                return Err(format!("Unsupported special file: {rel}"));
            };
            out.insert(
                rel,
                Fingerprint {
                    identity: (m.dev(), m.ino()),
                    size: if link.is_some() { 0 } else { m.len() },
                    modified: (m.mtime(), m.mtime_nsec()),
                    changed: (m.ctime(), m.ctime_nsec()),
                    mode: m.mode() & 0o777,
                    link,
                    external,
                },
            );
        }
        if identity(p)? != directory_identity {
            return Err("Source directory changed during metadata capture; retry".into());
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(root, root, eligible, &mut out)?;
    Ok(out)
}
#[cfg(not(unix))]
fn fingerprint(_: &Path, _: &BTreeSet<String>) -> Result<BTreeMap<String, Fingerprint>> {
    Err(UNSUPPORTED_FILESYSTEM.into())
}
impl Fingerprint {
    fn matches_clone(&self, copy: &Self) -> bool {
        self.size == copy.size
            && self.mode == copy.mode
            && self.link == copy.link
            && (self.link.is_some() || self.modified == copy.modified)
    }
}
fn clone_matches(
    before: &BTreeMap<String, Fingerprint>,
    clone: &BTreeMap<String, Fingerprint>,
) -> bool {
    before.values().filter(|f| !f.external).count() == clone.len()
        && before
            .iter()
            .filter(|(_, f)| !f.external)
            .all(|(path, original)| {
                clone
                    .get(path)
                    .is_some_and(|copy| original.matches_clone(copy))
            })
}
fn local_refs(root: &Path) -> Result<BTreeMap<String, String>> {
    text(
        root,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads",
            "refs/tags",
        ],
    )?
    .lines()
    .map(|line| {
        let (name, oid) = line.split_once(' ').ok_or("Invalid Git reference")?;
        Ok((name.to_string(), oid.to_string()))
    })
    .collect()
}

fn load(store: &Path, cwd: &Path, id: &str) -> Result<Workspace> {
    id_valid(id)?;
    let mut w: Workspace = read_json(&store.join(id).join("workspace.json"))?;
    let cwd = canonical(cwd)?;
    if cwd != Path::new(&w.project_cwd)
        && cwd != Path::new(&w.source_cwd)
        && cwd != Path::new(&w.path)
    {
        return Err("Isolation belongs to another project".into());
    }
    let root = Path::new(&w.path).to_path_buf();
    if identity(&root)? != w.identity {
        return Err("Isolation root was replaced".into());
    }
    upgrade_git_configuration(store, &mut w)?;
    Ok(w)
}
fn snapshot_directory(store: &Path, path: &Path, id: &str) -> Result<PathBuf> {
    id_valid(id)?;
    let parent = path
        .parent()
        .ok_or("Invalid isolation directory")?
        .join(".baselines");
    let baseline = parent.join(id);
    // Existing pre-release records used app-data snapshots; keep them recoverable.
    let legacy = store.join(id).join("baseline");
    if !baseline.exists() && legacy.exists() {
        identity(&legacy)?;
        return Ok(legacy);
    }
    private_dir(&parent)?;
    if baseline.exists() {
        identity(&baseline)?;
    }
    Ok(baseline)
}
fn resolve_base(source: &Path, base: &str) -> Result<String> {
    if base == "HEAD" {
        return text(source, &["rev-parse", "--verify", "HEAD^{commit}"]);
    }
    if base.is_empty() || base.len() > 1024 || base.starts_with('-') || base.contains('\0') {
        return Err("Choose an available base branch".into());
    }
    let refs = text(
        source,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    let chosen = refs
        .lines()
        .find(|reference| {
            *reference == base
                || *reference == format!("refs/heads/{base}")
                || *reference == format!("refs/remotes/{base}")
        })
        .ok_or("Choose an available base branch")?;
    text(
        source,
        &["rev-parse", "--verify", &format!("{chosen}^{{commit}}")],
    )
}
fn create(
    store: &Path,
    cwd: &Path,
    session: &str,
    project: Option<&Path>,
    base: Option<&str>,
) -> Result<Workspace> {
    if session.is_empty() || session.len() > 200 || session.contains('\0') {
        return Err("Invalid session ID".into());
    }
    let cap = capability(cwd);
    if cap["supported"] != true {
        return Err(cap["reason"]
            .as_str()
            .unwrap_or("Unsupported filesystem")
            .into());
    }
    let source = repo(cwd)?;
    let project = repo(project.unwrap_or(&source))?;
    for entry in fs::read_dir(store).map_err(err)? {
        let entry = entry.map_err(err)?;
        if let Ok(w) = read_json::<Workspace>(&entry.path().join("workspace.json")) {
            if w.session_id == session {
                if Path::new(&w.source_cwd) != source || Path::new(&w.project_cwd) != project {
                    return Err("Session already owns another isolation workspace".into());
                }
                if identity(Path::new(&w.path))? != w.identity {
                    return Err("Session isolation root was replaced".into());
                }
                return Ok(w);
            }
        }
    }
    let source_head = text(&source, &["rev-parse", "HEAD"])?;
    let head = resolve_base(&source, base.unwrap_or("HEAD"))?;
    if head != source_head
        && config_reader(&source)
            .args([
                "config",
                "--includes",
                "--name-only",
                "--get-regexp",
                "^filter\\.",
            ])
            .output()
            .map_err(err)?
            .status
            .success()
    {
        return Err("Choose the current HEAD for copy-on-write when Git filters are configured; another base would require executing those filters. Use a worktree for that base.".into());
    }
    let initial_refs = local_refs(&source)?;
    let token = session
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(8)
        .collect::<String>()
        .to_ascii_lowercase();
    if token.is_empty() {
        return Err("Session ID must contain letters or numbers".into());
    }
    let branch = Some(format!("mc/{token}"));
    if text(
        &source,
        &[
            "show-ref",
            "--verify",
            &format!("refs/heads/{}", branch.as_deref().unwrap_or_default()),
        ],
    )
    .is_ok()
    {
        return Err("The generated session branch already exists; start a new session".into());
    }
    let eligible = paths(&source)?;
    let before = fingerprint(&source, &eligible)?;
    let id = uuid::Uuid::new_v4().to_string();
    let parent = project.with_file_name(format!(
        "{}-cow",
        project
            .file_name()
            .ok_or("Invalid repository name")?
            .to_string_lossy()
    ));
    private_dir(&parent)?;
    let path = parent.join(&id);
    let record = store.join(&id);
    private_dir(&record)?;
    let mut intent = CreationIntent {
        id: id.clone(),
        path: path.clone(),
        project_cwd: project.clone(),
        identity: None,
        baseline_identity: None,
        removal_path: None,
    };
    write_json(&record.join("creation.json"), &intent)?;
    let result = (|| {
        fs::create_dir(&path).map_err(err)?;
        private_dir(&path)?;
        intent.identity = Some(identity(&path)?);
        write_json(&record.join("creation.json"), &intent)?;
        clone_tree(&source, &path, true)?;
        private_git(
            &source,
            &path,
            &source_head,
            branch.as_deref().unwrap_or("cow"),
        )?;
        let after = fingerprint(&source, &eligible)?;
        if before != after {
            let changed = before
                .keys()
                .chain(after.keys())
                .find(|p| before.get(*p) != after.get(*p))
                .ok_or("Invalid source comparison")?;
            return Err(format!("Source changed while cloning: {changed}; retry"));
        }
        let copied_eligible = paths(&path)?;
        let mut copied = fingerprint(&path, &copied_eligible)?;
        for added in copied_eligible.difference(&eligible) {
            if !before
                .iter()
                .any(|(p, f)| f.external && added.starts_with(&format!("{p}/")))
            {
                return Err(format!(
                    "Clone captured a new repository file: {added}; retry"
                ));
            }
        }
        let excluded = ignored_paths(&path)?;
        copied.retain(|p, _| eligible.contains(p) && before.get(p).is_none_or(|f| !f.external));
        if !clone_matches(&before, &copied) {
            let changed = before
                .iter()
                .filter(|(_, f)| !f.external)
                .find(|(p, original)| {
                    !copied
                        .get(*p)
                        .is_some_and(|copy| original.matches_clone(copy))
                })
                .map(|(p, _)| p)
                .or_else(|| copied.keys().find(|p| !before.contains_key(*p)))
                .ok_or("Invalid clone comparison")?;
            return Err(format!(
                "Clone does not match captured repository file: {changed}; retry"
            ));
        }
        if text(&source, &["rev-parse", "HEAD"])? != source_head {
            return Err("Source HEAD changed while cloning; retry".into());
        }
        if paths(&source)? != eligible {
            return Err("Source file list changed while cloning; retry".into());
        }
        if local_refs(&source)? != initial_refs {
            return Err("Source Git refs changed while cloning; retry".into());
        }
        if head != source_head {
            text(
                &path,
                &[
                    "checkout",
                    "--quiet",
                    "--no-overwrite-ignore",
                    "-B",
                    branch.as_deref().ok_or("Missing session branch")?,
                    &head,
                ],
            )?;
        }
        let eligible = paths(&path)?;
        let base = snapshot_directory(store, &path, &id)?;
        fs::create_dir(&base).map_err(err)?;
        private_dir(&base)?;
        intent.baseline_identity = Some(identity(&base)?);
        write_json(&record.join("creation.json"), &intent)?;
        init_repo(
            &base,
            &text(&source, &["rev-parse", "--show-object-format"])?,
        )?;
        let baseline = tree(&base, &path, &eligible, &excluded)?;
        let w = Workspace {
            id: id.clone(),
            path: path.to_string_lossy().into_owned(),
            source_cwd: source.to_string_lossy().into_owned(),
            project_cwd: project.to_string_lossy().into_owned(),
            session_id: session.into(),
            branch,
            head,
            dirty: None,
            unpushed: None,
            baseline,
            excluded,
            identity: identity(&path)?,
            git_config_version: 2,
            removal_path: None,
            initial_refs: Some(initial_refs),
        };
        write_json(&record.join("workspace.json"), &w)?;
        let _ = fs::remove_file(record.join("creation.json"));
        Ok(w)
    })();
    if result.is_err() {
        if let Err(error) = cleanup_creation(store, &mut intent) {
            eprintln!("Pending copy creation cleanup will retry: {error}");
        }
    }
    result
}
fn current(store: &Path, w: &Workspace) -> Result<(PathBuf, String)> {
    let objects = snapshot_directory(store, Path::new(&w.path), &w.id)?;
    let root = Path::new(&w.path);
    let mut eligible = paths(root)?;
    let original = git(
        &objects,
        &["ls-tree", "-r", "--name-only", "-z", &w.baseline],
        None,
    )?;
    for p in original.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        eligible.insert(String::from_utf8(p.to_vec()).map_err(err)?);
    }
    let t = tree(&objects, root, &eligible, &w.excluded)?;
    Ok((objects, t))
}
fn changed(objects: &Path, base: &str, next: &str) -> Result<Vec<(String, String)>> {
    let b = git(
        objects,
        &[
            "diff",
            "--no-renames",
            "--name-status",
            "-z",
            base,
            next,
            "--",
        ],
        None,
    )?;
    let fields: Vec<_> = b.split(|b| *b == 0).filter(|b| !b.is_empty()).collect();
    let mut out = vec![];
    for pair in fields.chunks_exact(2) {
        out.push((
            String::from_utf8(pair[1].to_vec()).map_err(err)?,
            String::from_utf8(pair[0].to_vec()).map_err(err)?,
        ))
    }
    Ok(out)
}
fn status(store: &Path, w: &Workspace) -> Result<Value> {
    let (o, t) = current(store, w)?;
    let mut counts = BTreeMap::new();
    let stats = git(
        &o,
        &[
            "diff",
            "--numstat",
            "-z",
            "--no-renames",
            &w.baseline,
            &t,
            "--",
        ],
        None,
    )?;
    for record in stats.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        let fields = record.splitn(3, |b| *b == b'\t').collect::<Vec<_>>();
        if fields.len() == 3 {
            let number = |b: &[u8]| String::from_utf8_lossy(b).parse::<i64>().unwrap_or(0);
            counts.insert(
                String::from_utf8(fields[2].to_vec()).map_err(err)?,
                (number(fields[0]), number(fields[1])),
            );
        }
    }
    let files=changed(&o,&w.baseline,&t)?.into_iter().map(|(p,s)|{let (a,d)=counts.get(&p).copied().unwrap_or_default();json!({"path":Path::new(&w.path).join(&p),"relative":p,"status":s,"additions":a,"deletions":d,"staged":false,"unstaged":true})}).collect::<Vec<_>>();
    Ok(json!({"files":files}))
}
fn file_diff(store: &Path, w: &Workspace, p: &str) -> Result<Value> {
    relative(p)?;
    let (o, t) = current(store, w)?;
    let size = |revision: &str| {
        text(&o, &["cat-file", "-s", &format!("{revision}:{p}")])
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    };
    let large = size(&w.baseline) > 2 * 1024 * 1024 || size(&t) > 2 * 1024 * 1024;
    let read = |rev: &str| -> Result<Vec<u8>> {
        if large {
            Ok(vec![])
        } else {
            Ok(git(&o, &["show", &format!("{rev}:{p}")], None).unwrap_or_default())
        }
    };
    let a = read(&w.baseline)?;
    let b = read(&t)?;
    let binary = a.contains(&0)
        || b.contains(&0)
        || std::str::from_utf8(&a).is_err()
        || std::str::from_utf8(&b).is_err();
    let status = changed(&o, &w.baseline, &t)?
        .into_iter()
        .find(|(path, _)| path == p)
        .map(|(_, s)| s)
        .unwrap_or_else(|| "M".into());
    Ok(
        json!({"path":Path::new(&w.path).join(p),"relative":p,"status":status,"original":if binary||large{String::new()}else{String::from_utf8_lossy(&a).into_owned()},"current":if binary||large{String::new()}else{String::from_utf8_lossy(&b).into_owned()},"binary":binary,"tooLarge":large}),
    )
}
#[cfg(unix)]
fn destination_parent(root: &Path, p: &str) -> Result<(File, std::ffi::CString)> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    relative(p)?;
    let parts = Path::new(p).components().collect::<Vec<_>>();
    let mut parent = open_dir(root)?;
    for component in &parts[..parts.len() - 1] {
        let name = std::ffi::CString::new(component.as_os_str().as_bytes()).map_err(err)?;
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o755) } != 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(err(std::io::Error::last_os_error()));
        }
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_DIRECTORY,
            )
        };
        if fd < 0 {
            return Err(err(std::io::Error::last_os_error()));
        }
        parent = unsafe { File::from_raw_fd(fd) };
    }
    Ok((
        parent,
        std::ffi::CString::new(parts.last().ok_or("Missing path")?.as_os_str().as_bytes())
            .map_err(err)?,
    ))
}
#[cfg(unix)]
fn apply(store: &Path, w: &Workspace, destination: &Path) -> Result<Value> {
    let destination = repo(destination)?;
    validate_repo(&destination)?;
    if destination != Path::new(&w.project_cwd) && destination != Path::new(&w.source_cwd) {
        return Err("Worker integration destination is outside its project".into());
    }
    let (objects, next) = current(store, w)?;
    let files = changed(&objects, &w.baseline, &next)?
        .into_iter()
        .map(|(p, _)| p)
        .collect::<Vec<_>>();
    let eligible = files.iter().cloned().collect::<BTreeSet<_>>();
    let target_tree = tree(&objects, &destination, &eligible, &BTreeSet::new())?;
    let entry = |revision: &str, p: &str| -> Result<Option<(String, String)>> {
        let line = text(&objects, &["ls-tree", revision, "--", p])?;
        let mut fields = line.split_whitespace();
        let mode = fields.next();
        fields.next();
        let oid = fields.next();
        Ok(mode.zip(oid).map(|(m, o)| (m.to_string(), o.to_string())))
    };
    let mut pending = Vec::new();
    let mut already = 0;
    for p in &files {
        let target = entry(&target_tree, p)?;
        if target == entry(&next, p)? {
            already += 1;
            continue;
        }
        if target != entry(&w.baseline, p)? {
            return Err(format!(
                "Cannot integrate {p}: lead checkout changed; worker was retained"
            ));
        }
        pending.push(p.clone());
    }
    // Read immutable captured blobs, so later worker edits cannot change this integration.
    #[cfg(unix)]
    for p in &pending {
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::PermissionsExt;
        let (parent, name) = destination_parent(&destination, p)?;
        match entry(&next, p)? {
            None => {
                if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
                    return Err(err(std::io::Error::last_os_error()));
                }
            }
            Some((mode, oid)) => {
                let temporary =
                    std::ffi::CString::new(format!(".monocode-{}", uuid::Uuid::new_v4()))
                        .map_err(err)?;
                if mode == "120000" {
                    let raw = git(&objects, &["cat-file", "blob", &oid], None)?;
                    let link = PathBuf::from(std::ffi::OsStr::from_bytes(&raw));
                    let link = if link.is_absolute() {
                        destination.join(
                            link.strip_prefix(Path::new(&w.path))
                                .map_err(|_| "External integration symlink")?,
                        )
                    } else {
                        link
                    };
                    let link = std::ffi::CString::new(link.as_os_str().as_bytes()).map_err(err)?;
                    if unsafe {
                        libc::symlinkat(link.as_ptr(), parent.as_raw_fd(), temporary.as_ptr())
                    } != 0
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
                    let out = unsafe { File::from_raw_fd(fd) };
                    let output = git_command(&objects)
                        .args(["cat-file", "blob", &oid])
                        .stdout(Stdio::from(out.try_clone().map_err(err)?))
                        .output()
                        .map_err(err)?;
                    if !output.status.success() {
                        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
                    }
                    out.set_permissions(fs::Permissions::from_mode(if mode == "100755" {
                        0o755
                    } else {
                        0o644
                    }))
                    .map_err(err)?;
                    out.sync_all().map_err(err)?;
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
            }
        }
    }
    Ok(json!({"files":files,"alreadyApplied":already}))
}
#[cfg(not(unix))]
fn apply(_: &Path, _: &Workspace, _: &Path) -> Result<Value> {
    Err(UNSUPPORTED_FILESYSTEM.into())
}
fn unpreserved_commits(w: &Workspace) -> Result<u64> {
    let root = Path::new(&w.path);
    let heads = text(
        root,
        &[
            "for-each-ref",
            "--format=%(objectname)",
            "refs/heads",
            "refs/tags",
        ],
    )?;
    let mut retained = BTreeSet::new();
    for oid in heads.lines() {
        let Ok(head) = text(
            root,
            &["rev-parse", "--verify", &format!("{oid}^{{commit}}")],
        ) else {
            continue;
        };
        for source in [Path::new(&w.project_cwd), Path::new(&w.source_cwd)] {
            if source != root
                && text(
                    source,
                    &[
                        "for-each-ref",
                        "--format=%(refname)",
                        "--contains",
                        &head,
                        "refs/heads",
                        "refs/tags",
                        "refs/remotes",
                    ],
                )
                .is_ok_and(|refs| !refs.is_empty())
            {
                retained.insert(head.clone());
                break;
            }
        }
    }
    let mut arguments = vec![
        "rev-list",
        "--count",
        "--branches",
        "--tags",
        "--not",
        "--remotes",
    ];
    arguments.extend(retained.iter().map(String::as_str));
    text(root, &arguments)?.parse().map_err(err)
}
fn git_dirty(root: &Path) -> Result<bool> {
    let dirty = !git(
        root,
        &["status", "--porcelain", "--untracked-files=all"],
        None,
    )?
    .is_empty();
    if dirty && text(root, &["config", "--get-regexp", "^filter\\."]).is_ok() {
        return Err("Cannot verify cleanliness without running Git filters. Review Git Changes and confirm deletion to remove this copy; commits and stashes will be kept.".into());
    }
    Ok(dirty)
}
fn check_remove(w: &Workspace, force: bool) -> Result<Value> {
    if identity(Path::new(&w.path))? != w.identity {
        return Err("Isolation root was replaced".into());
    }
    let source = repo(Path::new(&w.project_cwd))?;
    if source == Path::new(&w.path) {
        return Err("Cannot preserve history in the isolation being removed".into());
    }
    for root in [&source, &PathBuf::from(&w.path)] {
        let common = PathBuf::from(text(
            root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?);
        identity(&common)?;
        identity(&common.join("objects"))?;
        if common.join("objects/info/alternates").exists() {
            return Err("Git object alternates are unsupported".into());
        }
        if root == Path::new(&w.path) && common != root.join(".git") {
            return Err("Isolation Git directory was replaced".into());
        }
    }
    if !force && git_dirty(Path::new(&w.path))? {
        return Err("Copy-on-write has uncommitted changes; explicit force is required".into());
    }
    Ok(Value::Null)
}
fn preserve_history(w: &Workspace) -> Result<()> {
    let root = Path::new(&w.path);
    let source = repo(Path::new(&w.project_cwd))?;
    let mut refs = text(
        root,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads",
            "refs/tags",
        ],
    )?;
    if text(root, &["symbolic-ref", "HEAD"]).is_err() {
        let head = text(root, &["rev-parse", "--verify", "HEAD"])?;
        refs.push_str(&format!("\nHEAD {head}"));
    }
    for line in refs.lines().filter(|line| !line.is_empty()) {
        let (reference, oid) = line.split_once(' ').ok_or("Invalid Git reference")?;
        if w.initial_refs
            .as_ref()
            .is_some_and(|refs| refs.get(reference).map(String::as_str) == Some(oid))
        {
            continue; // An inherited ref that this copy never changed is not session history.
        }
        let existing = text(&source, &["rev-parse", "--verify", reference]).ok();
        if existing.as_deref() == Some(oid) {
            continue;
        }
        if reference.starts_with("refs/heads/")
            && existing.as_ref().is_some_and(|head| {
                git(&source, &["merge-base", "--is-ancestor", oid, head], None).is_ok()
            })
        {
            continue;
        }
        let destination = if reference == "HEAD" {
            format!("refs/heads/mc/kept-{}/detached", w.id)
        } else if existing.is_some()
            || (w.initial_refs.is_none()
                && reference.strip_prefix("refs/heads/") != w.branch.as_deref())
        {
            let (namespace, name) = if let Some(name) = reference.strip_prefix("refs/heads/") {
                ("refs/heads", name)
            } else {
                (
                    "refs/tags",
                    reference.strip_prefix("refs/tags/").ok_or("Invalid tag")?,
                )
            };
            format!("{namespace}/mc/kept-{}/{name}", w.id)
        } else {
            reference.to_string()
        };
        let retained = text(&source, &["rev-parse", "--verify", &destination]).ok();
        if retained.as_deref() == Some(oid) {
            continue;
        }
        let destination = if retained.is_some() {
            format!("{destination}-{oid}")
        } else {
            destination
        };
        // Import objects without changing HEAD, checked-out files, or existing refs.
        // An expected-empty update protects against concurrent ref creation.
        import_objects(&source, root, reference)?;
        text(&source, &["update-ref", &destination, oid, ""])?;
    }
    let mut retained = stash_entries(&source)?
        .into_iter()
        .map(|(oid, _)| oid)
        .collect::<BTreeSet<_>>();
    // Older stashes are reflog entries, not ancestors of the current stash tip.
    // Import oldest first so the stack remains usable after cleanup or a retry.
    for (oid, message) in stash_entries(root)?.into_iter().rev() {
        if retained.insert(oid.clone()) {
            import_objects(&source, root, &oid)?;
            text(&source, &["stash", "store", "-m", &message, &oid])?;
        }
    }
    Ok(())
}
fn stash_entries(root: &Path) -> Result<Vec<(String, String)>> {
    let found = git_command(root)
        .args(["show-ref", "--verify", "--quiet", "refs/stash"])
        .output()
        .map_err(err)?;
    if !found.status.success() {
        if found.status.code() != Some(1) {
            return Err(
                "Cannot read stash reference; history must be retained before deletion".into(),
            );
        }
        let log_path = PathBuf::from(text(
            root,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "logs/refs/stash",
            ],
        )?);
        match fs::symlink_metadata(log_path) {
            Ok(_) => return Err(
                "Stash reflog exists without its reference; recover it before deleting this copy"
                    .into(),
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(error) => return Err(err(error)),
        }
    }
    let tip = text(root, &["rev-parse", "--verify", "refs/stash"])?;
    let log = text(
        root,
        &["reflog", "show", "--format=%H%x00%gs", "refs/stash"],
    )?;
    let mut entries = log
        .lines()
        .map(|line| {
            line.split_once('\0')
                .map(|(oid, message)| (oid.to_string(), message.to_string()))
                .ok_or_else(|| "Invalid stash reflog".to_string())
        })
        .collect::<Result<Vec<_>>>()?;
    if !entries.iter().any(|(oid, _)| oid == &tip) {
        entries.insert(0, (tip, "Retained copy-on-write stash".into()));
    }
    Ok(entries)
}
fn import_objects(destination: &Path, source: &Path, reference: &str) -> Result<()> {
    text(
        destination,
        &[
            "-c",
            "uploadpack.packObjectsHook=git pack-objects",
            "fetch",
            "--no-recurse-submodules",
            "--no-tags",
            "--no-write-fetch-head",
            "--",
            source.to_str().ok_or("Non-UTF-8 Git path")?,
            reference,
        ],
    )?;
    Ok(())
}
fn cleanup_removed(store: &Path, w: &Workspace) -> Result<()> {
    id_valid(&w.id)?;
    match fs::symlink_metadata(&w.path) {
        Ok(_) => return Ok(()), // A crash before rename left the checkout intact.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(err(error)),
    }
    let tombstone = Path::new(w.removal_path.as_deref().ok_or("Missing removal path")?);
    if tombstone.parent() != Path::new(&w.path).parent()
        || !tombstone
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("remove-"))
    {
        return Err("Invalid removal path".into());
    }
    match fs::symlink_metadata(tombstone) {
        Ok(_) => {
            if identity(tombstone)? != w.identity {
                return Err("Isolation root changed during deletion".into());
            }
            fs::remove_dir_all(tombstone).map_err(err)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(err(error)),
    }
    let snapshots = [
        Path::new(&w.path)
            .parent()
            .ok_or("Invalid isolation root")?
            .join(".baselines")
            .join(&w.id),
        store.join(&w.id).join("baseline"),
    ];
    for snapshot in snapshots {
        match fs::symlink_metadata(&snapshot) {
            Ok(_) => {
                // Reject replaced parents/links rather than following them.
                identity(snapshot.parent().ok_or("Invalid baseline parent")?)?;
                identity(&snapshot)?;
                fs::remove_dir_all(snapshot).map_err(err)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(err(error)),
        }
    }
    fs::remove_dir_all(store.join(&w.id)).map_err(err)
}
fn remove(store: &Path, w: &Workspace, force: bool) -> Result<Value> {
    check_remove(w, force)?;
    preserve_history(w)?;
    let path = Path::new(&w.path);
    let tombstone = path.with_file_name(format!("remove-{}", uuid::Uuid::new_v4()));
    let mut removed = w.clone();
    removed.removal_path = Some(tombstone.to_string_lossy().into_owned());
    write_json(&store.join(&w.id).join("workspace.json"), &removed)?;
    fs::rename(path, &tombstone).map_err(err)?;
    if identity(&tombstone)? != w.identity {
        let _ = fs::rename(&tombstone, path);
        return Err("Isolation root changed during deletion".into());
    }
    // The owned checkout is gone. Metadata/leftover directory errors must not
    // cause the session-removal journal to restore sessions onto a missing path.
    // Keep the record so the next native request retries garbage collection.
    if let Err(error) = cleanup_removed(store, &removed) {
        eprintln!("Copy removed; isolation cleanup will retry: {error}");
    }
    Ok(Value::Null)
}
// Authorization reads are independent of Git status and the mutation lock.
// Records publish atomically only after creation; root identities are checked
// again by callers before use, and records for pending removals never authorize.
fn owned_metadata(store: &Path, cwd: &Path, id: Option<&str>, path: Option<&str>) -> Result<Value> {
    let cwd = canonical(cwd)?;
    let valid = |w: &Workspace| {
        w.git_config_version >= 2
            && w.removal_path.is_none()
            && identity(Path::new(&w.path)).is_ok_and(|identity| identity == w.identity)
            && identity(&Path::new(&w.path).join(".git")).is_ok()
    };
    if let Some(id) = id {
        id_valid(id)?;
        let w: Workspace = read_json(&store.join(id).join("workspace.json"))?;
        if w.id != id
            || !valid(&w)
            || ![&w.project_cwd, &w.source_cwd, &w.path]
                .iter()
                .any(|path| Path::new(path) == cwd)
        {
            return Err(
                "Copy-on-write workspace is unavailable or belongs to another project".into(),
            );
        }
        return w.view();
    }
    let registered = fs::read_dir(store)
        .map_err(err)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let w: Workspace = read_json(&entry.path().join("workspace.json")).ok()?;
            (entry.file_name().to_str() == Some(w.id.as_str()) && valid(&w)).then_some(w)
        })
        .collect::<Vec<_>>();
    let project = registered
        .iter()
        .find(|w| Path::new(&w.path) == cwd || Path::new(&w.source_cwd) == cwd)
        .map(|w| Path::new(&w.project_cwd))
        .unwrap_or(&cwd);
    if let Some(path) = path {
        return registered
            .iter()
            .find(|w| Path::new(&w.project_cwd) == project && Path::new(&w.path) == Path::new(path))
            .map(Workspace::view)
            .unwrap_or(Ok(Value::Null));
    }
    registered
        .iter()
        .filter(|w| Path::new(&w.project_cwd) == project)
        .map(Workspace::view)
        .collect::<Result<Vec<_>>>()
        .map(Value::Array)
}
pub fn dispatch(store: &Path, request: Value) -> Result<Value> {
    private_dir(store)?;
    let command = request["command"]
        .as_str()
        .ok_or("Missing isolation command")?;
    let a = &request["args"];
    let string = |key: &str| a[key].as_str().ok_or_else(|| format!("Missing {key}"));
    let cwd = Path::new(string("cwd")?);
    if !cfg!(target_os = "macos") {
        return match command {
            "cow_capability" => Ok(json!({"supported":false,"reason":UNSUPPORTED_FILESYSTEM})),
            "cow_list" | "cow_roots" => Ok(json!([])),
            _ => Err(UNSUPPORTED_FILESYSTEM.into()),
        };
    }
    if command == "cow_roots" {
        return owned_metadata(store, cwd, None, None);
    }
    if command == "cow_owner" {
        if a["cowId"].as_str().is_none() && a["path"].as_str().is_none() {
            return Err("Specify an isolation ID or owned path".into());
        }
        return owned_metadata(store, cwd, a["cowId"].as_str(), a["path"].as_str());
    }
    // ponytail: serialize registry operations; per-workspace locks if large captures contend.
    #[cfg(unix)]
    let _lock = {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .custom_flags(libc::O_NOFOLLOW)
            .mode(0o600)
            .open(store.join(".lock"))
            .map_err(err)?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(err(std::io::Error::last_os_error()));
        }
        lock
    };
    for entry in fs::read_dir(store).map_err(err)? {
        let entry = entry.map_err(err)?;
        if let Ok(mut intent) = read_json::<CreationIntent>(&entry.path().join("creation.json")) {
            if entry.file_name().to_str() == Some(intent.id.as_str()) {
                if let Err(error) = cleanup_creation(store, &mut intent) {
                    eprintln!("Pending copy creation cleanup will retry: {error}");
                }
            }
        }
        if let Ok(w) = read_json::<Workspace>(&entry.path().join("workspace.json")) {
            if w.removal_path.is_some() {
                if let Err(error) = cleanup_removed(store, &w) {
                    eprintln!("Isolation cleanup will retry: {error}");
                }
            }
        }
    }
    if command == "cow_capability" {
        return Ok(capability(cwd));
    }
    if command == "cow_create" {
        return create(
            store,
            cwd,
            string("sessionId")?,
            a["projectCwd"].as_str().map(Path::new),
            a["base"].as_str(),
        )?
        .view();
    }
    if command == "cow_list" {
        let cwd = canonical(cwd)?;
        let mut registered = vec![];
        for entry in fs::read_dir(store).map_err(err)? {
            let entry = entry.map_err(err)?;
            if let Ok(w) = read_json::<Workspace>(&entry.path().join("workspace.json")) {
                if identity(Path::new(&w.path)).is_ok_and(|i| i == w.identity) {
                    registered.push(w);
                }
            }
        }
        let project = registered
            .iter()
            .find(|w| Path::new(&w.path) == cwd || Path::new(&w.source_cwd) == cwd)
            .map(|w| PathBuf::from(&w.project_cwd))
            .unwrap_or(cwd);
        return serde_json::to_value(
            registered
                .into_iter()
                .filter(|w| Path::new(&w.project_cwd) == project)
                .filter_map(|mut w| {
                    if let Err(error) = upgrade_git_configuration(store, &mut w) {
                        eprintln!("Copy unavailable; its files and ownership record were retained ({}): {error}", w.id);
                        None
                    } else {
                        Some(w)
                    }
                })
                .map(|mut w| {
                    w.branch = text(Path::new(&w.path), &["symbolic-ref", "--short", "HEAD"]).ok();
                    w.head = text(Path::new(&w.path), &["rev-parse", "HEAD"]).unwrap_or_default();
                    w.dirty = git_dirty(Path::new(&w.path)).ok();
                    w.unpushed = unpreserved_commits(&w).ok();
                    w.view()
                })
                .collect::<Result<Vec<_>>>()?,
        )
        .map_err(err);
    }
    let w = load(store, cwd, string("cowId")?)?;
    match command {
        "cow_apply" => apply(store, &w, Path::new(string("toCwd")?)),
        "cow_status" => status(store, &w),
        "cow_file_diff" => file_diff(store, &w, string("relative")?),
        "cow_check_remove" => check_remove(&w, a["force"].as_bool().unwrap_or(false)),
        "cow_remove" => remove(store, &w, a["force"].as_bool().unwrap_or(false)),
        _ => Err("Unknown isolation command".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p =
                std::env::temp_dir().join(format!("monocode-cow-test-{}", uuid::Uuid::new_v4()));
            private_dir(&p).unwrap();
            Self(p)
        }
        fn repo(&self) -> PathBuf {
            let p = self.0.join("project");
            init_repo(&p, "sha1").unwrap();
            text(&p, &["config", "user.name", "Test"]).unwrap();
            text(&p, &["config", "user.email", "test@example.invalid"]).unwrap();
            fs::write(p.join("app.txt"), "first\nsecond\nthird\n").unwrap();
            fs::write(p.join(".gitignore"), "deps/\n").unwrap();
            text(&p, &["add", "."]).unwrap();
            text(&p, &["commit", "-m", "initial"]).unwrap();
            p
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn ordinary_git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }
    fn native_supported(source: &Path) -> bool {
        let cap = capability(source);
        if cap["supported"] == true {
            true
        } else {
            assert!(std::env::var_os("MONOCODE_REQUIRE_COW").is_none(), "{cap}");
            eprintln!("SKIP native APFS coverage: {cap}");
            false
        }
    }
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn unsupported_platform_refuses_cow_without_affecting_local_repository() {
        let t = Temp::new();
        let source = t.repo();
        let store = t.0.join("registry");
        let probe = dispatch(
            &store,
            json!({"command":"cow_capability","args":{"cwd":source}}),
        )
        .unwrap();
        assert_eq!(probe["supported"], false);
        assert_eq!(probe["reason"], UNSUPPORTED_FILESYSTEM);
        assert_eq!(
            dispatch(&store, json!({"command":"cow_list","args":{"cwd":source}})).unwrap(),
            json!([])
        );
        assert!(dispatch(
            &store,
            json!({"command":"cow_create","args":{"cwd":source,"sessionId":"unsupported"}})
        )
        .unwrap_err()
        .contains("APFS on macOS"));
        assert!(!source.with_file_name("project-cow").exists());
        assert_eq!(text(&source, &["status", "--porcelain"]).unwrap(), "");
    }
    #[cfg(unix)]
    fn runtime_churn_fixture(kind: &str) {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let t = Temp(PathBuf::from("/tmp").join(format!("cow-{}", uuid::Uuid::new_v4())));
        private_dir(&t.0).unwrap();
        let source = t.repo();
        if !native_supported(&source) {
            return;
        }
        fs::create_dir(source.join("deps")).unwrap();
        fs::write(source.join("deps/runtime.log"), "runtime\n").unwrap();
        fs::write(source.join("deps/cache"), "cache\n").unwrap();
        for n in 0..200 {
            fs::write(source.join(format!("stable-{n}.txt")), "stable\n").unwrap();
        }
        text(&source, &["add", "."]).unwrap();
        text(&source, &["commit", "-m", "stable files"]).unwrap();
        let store = t.0.join("registry");
        private_dir(&store).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_source = source.clone();
        let kind_owned = kind.to_string();
        let (ready, started) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            let directory = open_dir(&worker_source.join("deps")).unwrap();
            let mut log = fs::OpenOptions::new()
                .append(true)
                .open(worker_source.join("deps/runtime.log"))
                .unwrap();
            let mut first = true;
            while !worker_stop.load(Ordering::SeqCst) {
                match kind_owned.as_str() {
                    "log" => {
                        log.write_all(b"background\n").unwrap();
                    }
                    "directory" => {
                        directory
                            .set_times(
                                fs::FileTimes::new().set_modified(std::time::SystemTime::now()),
                            )
                            .unwrap();
                    }
                    "cache" => {
                        fs::write(worker_source.join("deps/new-cache"), "cache\n").unwrap();
                        fs::rename(
                            worker_source.join("deps/new-cache"),
                            worker_source.join("deps/cache"),
                        )
                        .unwrap();
                    }
                    "socket" => {
                        let path = worker_source.join("deps/s");
                        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
                        drop(listener);
                        fs::remove_file(path).unwrap();
                    }
                    "tracked" => {
                        fs::write(worker_source.join("app.txt"), "changing\n").unwrap();
                    }
                    _ => unreachable!(),
                }
                if first {
                    ready.send(()).unwrap();
                    first = false;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });
        started.recv().unwrap();
        let result = create(&store, &source, "churn", None, None);
        stop.store(true, Ordering::SeqCst);
        writer.join().unwrap();
        if kind == "tracked" {
            assert!(result.unwrap_err().contains("Source changed"));
            return;
        }
        let w = result.unwrap();
        let copy = Path::new(&w.path);
        assert_eq!(
            fs::read(copy.join("app.txt")).unwrap(),
            fs::read(source.join("app.txt")).unwrap()
        );
        assert_eq!(text(&source, &["status", "--porcelain"]).unwrap(), "");
        assert_eq!(status(&store, &w).unwrap()["files"], json!([]));
        let original = fs::read(source.join("deps/runtime.log")).unwrap();
        fs::write(copy.join("deps/runtime.log"), "copy only\n").unwrap();
        assert_eq!(fs::read(source.join("deps/runtime.log")).unwrap(), original);
        remove(&store, &w, false).unwrap();
        assert!(!copy.exists());
    }
    #[cfg(unix)]
    #[test]
    fn ignored_log_writes_allow_isolation() {
        runtime_churn_fixture("log");
    }
    #[cfg(unix)]
    #[test]
    fn directory_metadata_churn_allows_isolation() {
        runtime_churn_fixture("directory");
    }
    #[cfg(unix)]
    #[test]
    fn ignored_cache_replacement_allows_isolation() {
        runtime_churn_fixture("cache");
    }
    #[cfg(unix)]
    #[test]
    fn ignored_socket_churn_allows_isolation() {
        runtime_churn_fixture("socket");
    }
    #[cfg(unix)]
    #[test]
    fn tracked_changes_still_refuse_isolation() {
        runtime_churn_fixture("tracked");
    }
    #[cfg(unix)]
    #[test]
    fn ignored_dangling_dependency_links_do_not_block_isolation() {
        use std::os::unix::fs::symlink;
        let t = Temp::new();
        let source = t.repo();
        if !native_supported(&source) {
            return;
        }
        fs::write(source.join(".gitignore"), "deps/\nnode_modules/\n").unwrap();
        text(&source, &["add", ".gitignore"]).unwrap();
        text(&source, &["commit", "-m", "ignore dependencies"]).unwrap();
        let modules = source.join("node_modules/@ast2llm/core/node_modules");
        fs::create_dir_all(&modules).unwrap();
        let link = "../../../node_modules/.pnpm/ts-morph@25.0.1/node_modules/ts-morph";
        symlink(link, modules.join("ts-morph")).unwrap();
        fs::write(modules.join("present.js"), "dependency\n").unwrap();
        fs::create_dir(source.join("deps")).unwrap();
        symlink(t.0.join("missing-external"), source.join("deps/missing")).unwrap();
        let store = t.0.join("registry");
        private_dir(&store).unwrap();
        let w = create(&store, &source, "dangling-dependencies", None, None).unwrap();
        let copy = Path::new(&w.path);
        assert!(fs::symlink_metadata(
            copy.join("node_modules/@ast2llm/core/node_modules/ts-morph")
        )
        .is_err());
        assert!(fs::symlink_metadata(copy.join("deps/missing")).is_err());
        assert_eq!(
            fs::read(copy.join("node_modules/@ast2llm/core/node_modules/present.js")).unwrap(),
            b"dependency\n"
        );
        assert_eq!(status(&store, &w).unwrap()["files"], json!([]));
        assert_eq!(
            fs::read_link(modules.join("ts-morph")).unwrap(),
            PathBuf::from(link)
        );
        assert_eq!(text(&source, &["status", "--porcelain"]).unwrap(), "");
        remove(&store, &w, false).unwrap();
        assert!(!copy.exists());
        symlink("missing", source.join("untracked-link")).unwrap();
        assert!(create(&store, &source, "untracked-dangling", None, None).is_err());
        fs::remove_file(source.join("untracked-link")).unwrap();
        symlink("cycle", source.join("deps/cycle")).unwrap();
        assert!(create(&store, &source, "ignored-cycle", None, None).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn replaced_external_directory_is_not_captured() {
        let t = Temp::new();
        let source = t.repo();
        if !native_supported(&source) {
            return;
        }
        let external = t.0.join("external");
        fs::create_dir(&external).unwrap();
        let expected = identity(&external).unwrap();
        fs::rename(&external, t.0.join("old-external")).unwrap();
        fs::create_dir(&external).unwrap();
        fs::write(external.join("unexpected"), "do not capture").unwrap();
        let copy = t.0.join("copy");
        let error = clone_tree_inner(
            &external,
            &copy,
            false,
            &BTreeSet::from([source]),
            true,
            expected,
        )
        .unwrap_err();
        assert!(error.contains("directory replaced"), "{error}");
        assert!(!copy.join("unexpected").exists());
    }
    #[cfg(unix)]
    #[test]
    fn external_links_are_private_native_copies() {
        use std::os::unix::fs::symlink;
        let t = Temp::new();
        let source = t.repo();
        if !native_supported(&source) {
            return;
        }
        let store = t.0.join("registry");
        private_dir(&store).unwrap();
        let external = t.0.join("external");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("SKILL.md"), "original\n").unwrap();
        fs::write(t.0.join("other"), "other\n").unwrap();
        symlink("SKILL.md", external.join("internal")).unwrap();
        symlink(t.0.join("other"), external.join("chained")).unwrap();
        fs::create_dir(source.join("deps")).unwrap();
        symlink(&external, source.join("deps/skill")).unwrap();
        symlink(&external, source.join("skill")).unwrap();
        fs::write(source.join("skill-other"), "tracked\n").unwrap();
        text(&source, &["add", "skill-other"]).unwrap();
        symlink(&external, source.join("skill*")).unwrap();
        assert!(!tracked_path(&source, "skill*").unwrap());
        symlink(&external, source.join("[s]kill")).unwrap();
        text(&source, &["--literal-pathspecs", "add", "[s]kill"]).unwrap();
        assert!(tracked_path(&source, "[s]kill").unwrap());
        text(&source, &["--literal-pathspecs", "reset", "--", "[s]kill"]).unwrap();
        fs::remove_file(source.join("[s]kill")).unwrap();
        let w = create(&store, &source, "external", None, None).unwrap();
        let copy = Path::new(&w.path);
        assert!(fs::symlink_metadata(copy.join("skill")).unwrap().is_dir());
        assert_eq!(
            fs::read(copy.join("skill/internal")).unwrap(),
            b"original\n"
        );
        assert_eq!(fs::read(copy.join("skill/chained")).unwrap(), b"other\n");
        assert_eq!(
            fs::read(copy.join("skill*/SKILL.md")).unwrap(),
            b"original\n"
        );
        assert_eq!(status(&store, &w).unwrap()["files"], json!([]));
        fs::write(copy.join("skill/SKILL.md"), "copy edit\n").unwrap();
        fs::write(copy.join("deps/skill/SKILL.md"), "ignored edit\n").unwrap();
        assert_eq!(fs::read(external.join("SKILL.md")).unwrap(), b"original\n");
        assert!(apply(&store, &w, &source).is_err());
        assert_eq!(fs::read(external.join("SKILL.md")).unwrap(), b"original\n");
        remove(&store, &w, true).unwrap();
        assert!(!copy.exists());
        assert!(source.join("skill").is_symlink());
        symlink(&source, external.join("cycle")).unwrap();
        assert!(create(&store, &source, "cycle", None, None)
            .unwrap_err()
            .contains("cycle"));
        fs::remove_file(external.join("cycle")).unwrap();
        symlink(&t.0, external.join("ancestor")).unwrap();
        assert!(create(&store, &source, "ancestor", None, None)
            .unwrap_err()
            .contains("ancestor"));
        fs::remove_file(external.join("ancestor")).unwrap();
        #[cfg(target_os = "macos")]
        {
            let alias = PathBuf::from(format!(
                "/System/Volumes/Data{}",
                canonical(&t.0).unwrap().display()
            ));
            assert!(alias.exists(), "APFS alias coverage unavailable");
            symlink(&alias, external.join("alias")).unwrap();
            let error = create(&store, &source, "alias", None, None).unwrap_err();
            assert!(error.contains("ancestor"), "{error}");
            fs::remove_file(external.join("alias")).unwrap();
        }
        init_repo(&external.join("nested"), "sha1").unwrap();
        assert!(create(&store, &source, "nested", None, None)
            .unwrap_err()
            .contains("Nested Git repository"));
    }
    #[cfg(unix)]
    #[test]
    fn ignored_runtime_sockets_do_not_block_isolation() {
        use std::os::unix::net::UnixListener;
        let t = Temp(PathBuf::from("/tmp").join(format!("cow-{}", uuid::Uuid::new_v4())));
        private_dir(&t.0).unwrap();
        let source = t.repo();
        if !native_supported(&source) {
            return;
        }
        // Use a short socket path: macOS limits Unix socket addresses to 104 bytes.
        fs::write(source.join(".gitignore"), "deps/\napp.txt\n").unwrap();
        text(&source, &["add", ".gitignore"]).unwrap();
        text(&source, &["commit", "-m", "ignore runtime files"]).unwrap();
        fs::create_dir(source.join("deps")).unwrap();
        fs::write(source.join("deps/cache"), "keep ignored regular files").unwrap();
        let socket = UnixListener::bind(source.join("deps/s")).unwrap();
        let store = t.0.join("registry");
        private_dir(&store).unwrap();
        let w = create(&store, &source, "socket-session", None, None).unwrap();
        let copy = Path::new(&w.path);
        assert!(!copy.join("deps/s").exists());
        assert_eq!(
            fs::read(copy.join("deps/cache")).unwrap(),
            b"keep ignored regular files"
        );
        assert!(status(&store, &w).is_ok());
        remove(&store, &w, false).unwrap();
        assert!(!copy.exists());
        assert!(source.join("deps/s").exists());
        assert!(socket.local_addr().is_ok());

        let unignored = UnixListener::bind(source.join("s")).unwrap();
        assert!(create(&store, &source, "unignored", None, None)
            .unwrap_err()
            .contains("Unsupported special file"));
        drop(unignored);
        fs::remove_file(source.join("s")).unwrap();
        use std::os::unix::ffi::OsStrExt;
        let fifo = source.join("deps/fifo");
        let fifo_name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_name.as_ptr(), 0o600) }, 0);
        assert!(create(&store, &source, "fifo", None, None)
            .unwrap_err()
            .contains("Unsupported special file"));
        fs::remove_file(fifo).unwrap();
        let object_socket = UnixListener::bind(source.join(".git/objects/s")).unwrap();
        assert!(create(&store, &source, "git-object", None, None)
            .unwrap_err()
            .contains("Unsupported special file"));
        drop(object_socket);
        fs::remove_file(source.join(".git/objects/s")).unwrap();
        fs::remove_file(source.join("app.txt")).unwrap();
        let tracked = UnixListener::bind(source.join("app.txt")).unwrap();
        assert!(create(&store, &source, "tracked", None, None)
            .unwrap_err()
            .contains("Unsupported special file"));
        drop(tracked);
    }
    #[test]
    fn public_metadata_is_compact_owned_and_independent_of_the_mutation_lock() {
        let t = Temp::new();
        let source = t.repo();
        if !native_supported(&source) {
            return;
        }
        let store = t.0.join("registry");
        private_dir(&store).unwrap();
        let mut w = create(&store, &source, "metadata-session", None, None).unwrap();
        w.excluded
            .extend((0..5000).map(|i| format!("deps/private-{i}")));
        write_json(&store.join(&w.id).join("workspace.json"), &w).unwrap();
        let created = dispatch(
            &store,
            json!({"command":"cow_create","args":{"cwd":source,"sessionId":"metadata-session"}}),
        )
        .unwrap();
        assert!(serde_json::to_vec(&created).unwrap().len() < 2000);
        for field in [
            "excluded",
            "baseline",
            "identity",
            "initialRefs",
            "removalPath",
            "gitConfigVersion",
        ] {
            assert!(created.get(field).is_none(), "Internal {field} was exposed");
        }
        let listed = dispatch(&store, json!({"command":"cow_list","args":{"cwd":source}})).unwrap();
        assert!(listed[0].get("excluded").is_none());
        let owner_request = json!({"command":"cow_owner","args":{"cwd":source,"cowId":w.id}});
        let owner = dispatch(&store, owner_request.clone()).unwrap();
        assert_eq!(owner["rootIdentity"][0], w.identity.0.to_string());
        assert!(owner["gitIdentity"][0].is_string());
        assert!(dispatch(
            &store,
            json!({"command":"cow_owner","args":{"cwd":t.0,"cowId":w.id}})
        )
        .is_err());
        assert!(dispatch(&store, json!({"command":"cow_owner","args":{"cwd":source,"cowId":uuid::Uuid::new_v4().to_string()}})).is_err());
        assert_eq!(
            dispatch(
                &store,
                json!({"command":"cow_owner","args":{"cwd":source,"path":source}})
            )
            .unwrap(),
            Value::Null
        );
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let lock = File::open(store.join(".lock")).unwrap();
            assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) }, 0);
            let (sender, receiver) = std::sync::mpsc::channel();
            let worker_store = store.clone();
            let worker_source = source.clone();
            let worker = std::thread::spawn(move || {
                let roots = dispatch(
                    &worker_store,
                    json!({"command":"cow_roots","args":{"cwd":worker_source}}),
                );
                let owner = dispatch(&worker_store, owner_request);
                sender.send((roots, owner)).unwrap();
            });
            let result = receiver.recv_timeout(std::time::Duration::from_secs(3));
            drop(lock);
            worker.join().unwrap();
            let (roots, owner) =
                result.expect("Authorization waited for an unrelated mutation lock");
            assert_eq!(roots.unwrap().as_array().unwrap().len(), 1);
            assert_eq!(owner.unwrap()["id"], w.id);
        }
        w.git_config_version = 1;
        write_json(&store.join(&w.id).join("workspace.json"), &w).unwrap();
        assert!(dispatch(
            &store,
            json!({"command":"cow_owner","args":{"cwd":source,"cowId":w.id}})
        )
        .is_err());
        assert_eq!(
            dispatch(&store, json!({"command":"cow_roots","args":{"cwd":source}})).unwrap(),
            json!([])
        );
        w.git_config_version = 2;
        write_json(&store.join(&w.id).join("workspace.json"), &w).unwrap();
        let saved = Path::new(&w.path).with_file_name("saved-copy");
        fs::rename(&w.path, &saved).unwrap();
        private_dir(Path::new(&w.path)).unwrap();
        init_repo(Path::new(&w.path), "sha1").unwrap();
        assert!(dispatch(
            &store,
            json!({"command":"cow_owner","args":{"cwd":source,"cowId":w.id}})
        )
        .is_err());
        assert_eq!(
            dispatch(&store, json!({"command":"cow_roots","args":{"cwd":source}})).unwrap(),
            json!([])
        );
    }
    #[test]
    fn cleanup_preserves_session_refs_without_restoring_untouched_source_refs() {
        let t = Temp::new();
        let source = t.repo();
        if !native_supported(&source) {
            return;
        }
        let store = t.0.join("registry");
        private_dir(&store).unwrap();
        text(&source, &["branch", "deleted-in-source"]).unwrap();
        text(&source, &["tag", "deleted-tag"]).unwrap();
        let source_branch = text(&source, &["symbolic-ref", "--short", "HEAD"]).unwrap();
        let w = create(&store, &source, "ref-session", None, None).unwrap();
        text(&source, &["branch", "-D", "deleted-in-source"]).unwrap();
        text(&source, &["tag", "-d", "deleted-tag"]).unwrap();
        fs::write(source.join("app.txt"), "rebased source\n").unwrap();
        text(&source, &["add", "."]).unwrap();
        text(&source, &["commit", "--amend", "-m", "rewritten source"]).unwrap();
        fs::write(Path::new(&w.path).join("new.txt"), "new session commit\n").unwrap();
        text(Path::new(&w.path), &["add", "."]).unwrap();
        text(Path::new(&w.path), &["commit", "-m", "session"]).unwrap();
        let session_head = text(Path::new(&w.path), &["rev-parse", "HEAD"]).unwrap();
        remove(&store, &w, false).unwrap();
        assert!(text(
            &source,
            &["rev-parse", "--verify", "refs/heads/deleted-in-source"]
        )
        .is_err());
        assert!(text(&source, &["rev-parse", "--verify", "refs/tags/deleted-tag"]).is_err());
        assert_eq!(
            text(&source, &["rev-parse", w.branch.as_deref().unwrap()]).unwrap(),
            session_head
        );
        assert!(text(&source, &["for-each-ref", "--format=%(refname)"])
            .unwrap()
            .lines()
            .all(|r| !r.contains(&format!("kept-{}/{}", w.id, source_branch))));
        let unchanged = create(&store, &source, "unchanged-session", None, None).unwrap();
        remove(&store, &unchanged, false).unwrap();
        assert!(text(
            &source,
            &["rev-parse", unchanged.branch.as_deref().unwrap()]
        )
        .is_ok());
        let mut legacy = create(&store, &source, "legacy-ref-session", None, None).unwrap();
        legacy.initial_refs = None;
        text(&source, &["branch", "legacy-deleted"]).unwrap();
        text(Path::new(&legacy.path), &["branch", "legacy-deleted"]).unwrap();
        text(&source, &["branch", "-D", "legacy-deleted"]).unwrap();
        preserve_history(&legacy).unwrap();
        assert!(text(
            &source,
            &["rev-parse", "--verify", "refs/heads/legacy-deleted"]
        )
        .is_err());
    }
    #[test]
    fn filtered_other_bases_fail_without_running_filters_or_cloning_secrets() {
        let t = Temp::new();
        let source = t.repo();
        if !native_supported(&source) {
            return;
        }
        let store = t.0.join("registry");
        private_dir(&store).unwrap();
        text(&source, &["branch", "other-base"]).unwrap();
        fs::write(source.join("app.txt"), "new HEAD\n").unwrap();
        text(&source, &["add", "."]).unwrap();
        text(&source, &["commit", "-m", "new HEAD"]).unwrap();
        let marker = t.0.join("filter-ran");
        text(
            &source,
            &[
                "config",
                "filter.example.smudge",
                &format!("touch {}", marker.display()),
            ],
        )
        .unwrap();
        assert!(create(
            &store,
            &source,
            "filtered-base-session",
            None,
            Some("other-base")
        )
        .unwrap_err()
        .contains("Choose the current HEAD"));
        assert!(!marker.exists());
        assert!(!source.with_file_name("project-cow").exists());
        let w = create(&store, &source, "filtered-head-session", None, None).unwrap();
        assert_eq!(
            fs::read(Path::new(&w.path).join("app.txt")).unwrap(),
            b"new HEAD\n"
        );
        assert!(!marker.exists());
        remove(&store, &w, true).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn batched_raw_snapshots_handle_binary_symlinks_special_paths_and_sha256() {
        for format in ["sha1", "sha256"] {
            let t = Temp::new();
            let root = t.0.join("working");
            private_dir(&root).unwrap();
            let root = canonical(&root).unwrap();
            let objects = t.0.join("objects");
            init_repo(&objects, format).unwrap();
            fs::write(root.join("binary\nname\t.txt"), [0, 1, 2, 255]).unwrap();
            fs::write(
                root.join("quote\" and space.txt"),
                b"payload\ndone\nblob\ndata 0\n",
            )
            .unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink("quote\" and space.txt", root.join("link")).unwrap();
            let mut eligible =
                BTreeSet::from(["binary\nname\t.txt".into(), "quote\" and space.txt".into()]);
            #[cfg(unix)]
            eligible.insert("link".into());
            for i in 0..250 {
                let p = format!("file-{i}.txt");
                fs::write(root.join(&p), format!("file {i}\n")).unwrap();
                eligible.insert(p);
            }
            let tree_oid = tree(&objects, &root, &eligible, &BTreeSet::new()).unwrap();
            assert_eq!(
                git(
                    &objects,
                    &["show", &format!("{tree_oid}:binary\nname\t.txt")],
                    None
                )
                .unwrap(),
                [0, 1, 2, 255]
            );
            assert_eq!(
                git(
                    &objects,
                    &["show", &format!("{tree_oid}:quote\" and space.txt")],
                    None
                )
                .unwrap(),
                b"payload\ndone\nblob\ndata 0\n"
            );
            #[cfg(unix)]
            assert_eq!(
                text(&objects, &["show", &format!("{tree_oid}:link")]).unwrap(),
                "quote\" and space.txt"
            );
            assert_eq!(
                tree(&objects, &root, &eligible, &BTreeSet::new()).unwrap(),
                tree_oid
            );
            fs::write(root.join("file-1.txt"), "changed\n").unwrap();
            assert_ne!(
                tree(&objects, &root, &eligible, &BTreeSet::new()).unwrap(),
                tree_oid
            );
        }
    }
    #[test]
    fn abandoned_creation_recovers_secrets_but_preserves_registered_or_replaced_roots() {
        let t = Temp::new();
        let source = canonical(&t.repo()).unwrap();
        if !native_supported(&source) {
            return;
        }
        let store = t.0.join("registry");
        private_dir(&store).unwrap();
        private_dir(&source.join("deps")).unwrap();
        fs::write(source.join("deps/secret.env"), "private checkout secret\n").unwrap();
        for replaced in [false, true] {
            let id = uuid::Uuid::new_v4().to_string();
            let path = source.with_file_name("project-cow").join(&id);
            private_dir(path.parent().unwrap()).unwrap();
            let record = store.join(&id);
            private_dir(&record).unwrap();
            let mut intent = CreationIntent {
                id: id.clone(),
                path: path.clone(),
                project_cwd: source.clone(),
                identity: None,
                baseline_identity: None,
                removal_path: None,
            };
            write_json(&record.join("creation.json"), &intent).unwrap();
            private_dir(&path).unwrap();
            intent.identity = Some(identity(&path).unwrap());
            write_json(&record.join("creation.json"), &intent).unwrap();
            clone_tree(&source, &path, true).unwrap();
            assert!(path.join("deps/secret.env").exists());
            if replaced {
                fs::rename(&path, path.with_file_name(format!("saved-{id}"))).unwrap();
                private_dir(&path).unwrap();
                fs::write(path.join("keep.txt"), "replacement\n").unwrap();
            }
            dispatch(&store, json!({"command":"cow_list","args":{"cwd":source}})).unwrap();
            if replaced {
                assert_eq!(fs::read(path.join("keep.txt")).unwrap(), b"replacement\n");
                assert!(record.join("creation.json").exists());
            } else {
                assert!(!path.exists());
                assert!(!record.exists());
            }
        }
        let w = create(&store, &source, "registered-intent-session", None, None).unwrap();
        let record = store.join(&w.id);
        let intent = CreationIntent {
            id: w.id.clone(),
            path: PathBuf::from(&w.path),
            project_cwd: source.clone(),
            identity: Some(w.identity),
            baseline_identity: Some(
                identity(&snapshot_directory(&store, Path::new(&w.path), &w.id).unwrap()).unwrap(),
            ),
            removal_path: None,
        };
        write_json(&record.join("creation.json"), &intent).unwrap();
        dispatch(&store, json!({"command":"cow_list","args":{"cwd":source}})).unwrap();
        assert!(Path::new(&w.path).join("deps/secret.env").exists());
        assert!(record.join("workspace.json").exists());
        assert!(!record.join("creation.json").exists());
    }
    #[test]
    fn failed_copy_upgrade_does_not_block_healthy_workspaces_or_other_projects() {
        let t = Temp::new();
        let source = t.repo();
        if !native_supported(&source) {
            return;
        }
        let store = t.0.join("registry");
        private_dir(&store).unwrap();
        let donor = create(&store, &source, "donor-session", None, None).unwrap();
        let mut legacy = create(
            &store,
            Path::new(&donor.path),
            "legacy-session",
            Some(&source),
            None,
        )
        .unwrap();
        remove(&store, &donor, false).unwrap();
        legacy.git_config_version = 1;
        write_json(&store.join(&legacy.id).join("workspace.json"), &legacy).unwrap();
        let healthy = create(&store, &source, "healthy-session", None, None).unwrap();
        let listed = dispatch(&store, json!({"command":"cow_list","args":{"cwd":source}})).unwrap();
        assert_eq!(listed.as_array().unwrap().len(), 1);
        assert_eq!(listed[0]["id"], healthy.id);
        assert!(Path::new(&legacy.path).exists());
        assert!(store.join(&legacy.id).join("workspace.json").exists());
        assert!(load(&store, &source, &legacy.id).is_err());
        let ordinary_project = Temp::new();
        let ordinary_root = ordinary_project.repo();
        assert_eq!(
            dispatch(
                &store,
                json!({"command":"cow_list","args":{"cwd":ordinary_root}})
            )
            .unwrap(),
            json!([])
        );
        assert_eq!(
            text(&ordinary_root, &["status", "--porcelain"]).unwrap(),
            ""
        );
    }
    #[test]
    fn cleanup_keeps_every_stash_and_retries_after_irreversible_removal() {
        let t = Temp::new();
        let source = t.repo();
        if !native_supported(&source) {
            return;
        }
        let store = t.0.join("registry");
        private_dir(&store).unwrap();
        fs::write(source.join("app.txt"), "source stash\n").unwrap();
        let source_stash = ordinary_git(&source, &["stash", "push", "-m", "source stash"]);
        assert!(!source_stash.is_empty());
        let original_stash = text(&source, &["rev-parse", "refs/stash"]).unwrap();
        let w = create(&store, &source, "stash-session", None, None).unwrap();
        let root = Path::new(&w.path);
        let mut stash_ids = vec![];
        for message in ["older copy stash", "newer copy stash"] {
            fs::write(root.join("app.txt"), format!("{message}\n")).unwrap();
            ordinary_git(root, &["stash", "push", "-m", message]);
            stash_ids.push(text(root, &["rev-parse", "refs/stash"]).unwrap());
        }
        let stash_ref = root.join(".git/refs/stash");
        let stash_bytes = fs::read(&stash_ref).unwrap();
        fs::remove_file(&stash_ref).unwrap();
        assert!(stash_entries(root).unwrap_err().contains("Stash reflog"));
        fs::write(&stash_ref, "corrupt reference\n").unwrap();
        assert!(remove(&store, &w, true).is_err());
        assert!(
            root.exists(),
            "Corrupt stash metadata must block even forced deletion"
        );
        fs::write(&stash_ref, stash_bytes).unwrap();
        assert!(!git_dirty(root).unwrap());
        preserve_history(&w).unwrap();
        preserve_history(&w).unwrap(); // Retry must not duplicate the stash stack.
        let retained = stash_entries(&source).unwrap();
        assert_eq!(retained.len(), 3);
        assert_eq!(
            retained
                .iter()
                .map(|(oid, _)| oid.clone())
                .collect::<Vec<_>>(),
            vec![stash_ids[1].clone(), stash_ids[0].clone(), original_stash]
        );
        let source_before = fs::read(source.join("app.txt")).unwrap();
        #[cfg(unix)]
        {
            let baseline = snapshot_directory(&store, root, &w.id).unwrap();
            let saved = baseline.with_file_name(format!("saved-{}", w.id));
            fs::rename(&baseline, &saved).unwrap();
            std::os::unix::fs::symlink(&source, &baseline).unwrap();
            remove(&store, &w, false).unwrap();
            assert!(!root.exists());
            assert!(store.join(&w.id).join("workspace.json").exists());
            assert_eq!(fs::read(source.join("app.txt")).unwrap(), source_before);
            fs::remove_file(&baseline).unwrap();
            fs::rename(saved, &baseline).unwrap();
            dispatch(&store, json!({"command":"cow_list","args":{"cwd":source}})).unwrap();
            assert!(!baseline.exists());
            assert!(!store.join(&w.id).exists());
        }
        for (index, oid) in stash_ids.iter().enumerate() {
            assert_eq!(
                text(&source, &["show", &format!("{oid}:app.txt")]).unwrap(),
                if index == 0 {
                    "older copy stash"
                } else {
                    "newer copy stash"
                }
            );
        }
    }
    #[test]
    fn normal_git_keeps_conversion_settings_and_existing_copies_are_upgraded() {
        let t = Temp::new();
        let source = t.repo();
        if !native_supported(&source) {
            return;
        }
        let marker = t.0.join("filter-ran");
        let clean = format!("touch {}; sed s/WORLD/PTR/", marker.display());
        let smudge = format!("touch {}; sed s/PTR/WORLD/", marker.display());
        for (key, value) in [
            ("core.autocrlf", "true"),
            ("core.fileMode", "false"),
            ("core.eol", "crlf"),
            ("filter.convert.clean", clean.as_str()),
            ("filter.convert.smudge", smudge.as_str()),
            ("filter.convert.required", "true"),
            ("gpg.program", "/custom/signing-program"),
        ] {
            text(&source, &["config", key, value]).unwrap();
        }
        fs::write(source.join(".gitattributes"), "app.txt filter=convert\n").unwrap();
        private_dir(&source.join(".git/info")).unwrap();
        fs::write(source.join(".git/info/exclude"), "private.env\n").unwrap();
        fs::write(source.join("ignore-rules"), "outside.env\n").unwrap();
        fs::write(source.join("attribute-rules"), "app.txt text\n").unwrap();
        text(&source, &["config", "core.excludesFile", "ignore-rules"]).unwrap();
        text(
            &source,
            &["config", "core.attributesFile", "attribute-rules"],
        )
        .unwrap();
        fs::write(source.join("private.env"), "PRIVATE_SECRET\n").unwrap();
        fs::write(source.join("outside.env"), "OUTSIDE_SECRET\n").unwrap();
        fs::write(source.join("app.txt"), "WORLD\r\n").unwrap();
        ordinary_git(&source, &["add", "."]);
        ordinary_git(&source, &["commit", "-m", "filtered file"]);
        assert_eq!(ordinary_git(&source, &["status", "--porcelain"]), "");
        fs::remove_file(&marker).unwrap();
        let store = t.0.join("registry");
        private_dir(&store).unwrap();
        let mut w = create(&store, &source, "conversion-session", None, None).unwrap();
        let root = Path::new(&w.path);
        status(&store, &w).unwrap();
        dispatch(&store, json!({"command":"cow_list","args":{"cwd":source}})).unwrap();
        assert!(
            !marker.exists(),
            "Native operations must not run the filter"
        );
        assert_eq!(ordinary_git(root, &["status", "--porcelain"]), "");
        ordinary_git(root, &["add", "."]);
        assert_eq!(ordinary_git(root, &["diff", "--cached", "--name-only"]), "");
        assert_eq!(
            ordinary_git(root, &["check-ignore", "private.env", "outside.env"]),
            "private.env\noutside.env"
        );
        assert_eq!(
            ordinary_git(root, &["config", "--get", "core.excludesFile"]),
            root.join("ignore-rules").to_string_lossy()
        );
        assert_eq!(
            ordinary_git(root, &["config", "--get", "core.attributesFile"]),
            root.join("attribute-rules").to_string_lossy()
        );
        fs::remove_file(&marker).unwrap();
        if let Err(error) = check_remove(&w, false) {
            assert!(error.contains("without running Git filters"));
        }
        assert!(!marker.exists());
        fs::write(root.join("app.txt"), "WORLD changed\r\n").unwrap();
        assert!(check_remove(&w, false).is_err());
        assert!(!marker.exists());
        ordinary_git(root, &["add", "app.txt"]);
        assert_eq!(text(root, &["show", ":app.txt"]).unwrap(), "PTR changed");
        ordinary_git(root, &["commit", "-m", "normal filtered commit"]);
        assert_eq!(ordinary_git(root, &["status", "--porcelain"]), "");
        for key in [
            "core.autocrlf",
            "core.eol",
            "filter.convert.clean",
            "filter.convert.smudge",
            "filter.convert.required",
            "gpg.program",
            "core.excludesFile",
            "core.attributesFile",
        ] {
            text(root, &["config", "--unset-all", key]).unwrap();
        }
        text(root, &["config", "core.fileMode", "true"]).unwrap();
        fs::remove_file(root.join(".git/info/exclude")).unwrap();
        w.git_config_version = 1;
        write_json(&store.join(&w.id).join("workspace.json"), &w).unwrap();
        let upgraded = load(&store, &source, &w.id).unwrap();
        assert_eq!(upgraded.git_config_version, 2);
        assert_eq!(
            ordinary_git(root, &["check-ignore", "private.env", "outside.env"]),
            "private.env\noutside.env"
        );
        assert_eq!(
            text(root, &["config", "--get", "core.autocrlf"]).unwrap(),
            "true"
        );
        assert_eq!(
            config_values(root, "filter.convert.clean", true).unwrap(),
            vec![clean]
        );
        assert_eq!(
            text(root, &["config", "--get", "gpg.program"]).unwrap(),
            "/custom/signing-program"
        );
        assert_eq!(
            text(root, &["config", "--get", "core.fileMode"]).unwrap(),
            "true",
            "Keep explicit configuration chosen in the copy"
        );
        fs::remove_file(&marker).unwrap();
        remove(&store, &upgraded, true).unwrap();
        assert!(!marker.exists(), "Cleanup must not run filters");
    }
    #[test]
    fn trust_boundary_paths_and_git_routing() {
        for p in ["", "../escape", "/tmp/escape", "a/../escape", "a\0b"] {
            assert!(relative(p).is_err(), "{p:?}");
        }
        assert!(relative("normal/file.txt").is_ok());
        assert!(id_valid("../escape").is_err());
        let t = Temp::new();
        let repo = t.repo();
        let cmd = git_command(&repo);
        assert!(
            cmd.get_envs()
                .any(|(k, v)| k == "GIT_CONFIG_GLOBAL"
                    && v == Some(std::ffi::OsStr::new("/dev/null")))
        );
        assert!(cmd.get_args().any(|s| s == "core.hooksPath=/dev/null"));
        text(&repo, &["update-index", "--assume-unchanged", "app.txt"]).unwrap();
        assert!(validate_repo(&repo)
            .unwrap_err()
            .contains("assume-unchanged"));
        text(&repo, &["update-index", "--no-assume-unchanged", "app.txt"]).unwrap();
        text(&repo, &["update-index", "--skip-worktree", "app.txt"]).unwrap();
        assert!(validate_repo(&repo).unwrap_err().contains("Skip-worktree"));
    }
    #[test]
    fn native_clone_delta_worker_and_cleanup() {
        let t = Temp::new();
        let source = t.repo();
        let store = t.0.join("registry");
        private_dir(&store).unwrap();
        if capability(&source)["supported"] != true {
            assert!(
                std::env::var_os("MONOCODE_REQUIRE_COW").is_none(),
                "Native CoW coverage is required: {}",
                capability(&source)
            );
            eprintln!("SKIP native APFS coverage: {}", capability(&source));
            return;
        }
        fs::write(source.join("app.txt"), "first\ninherited\nthird\n").unwrap();
        private_dir(&source.join("deps")).unwrap();
        fs::write(source.join("deps/pkg"), "dependency").unwrap();
        let w = create(&store, &source, "session", None, None).unwrap();
        let clone = Path::new(&w.path);
        let snapshot = snapshot_directory(&store, clone, &w.id).unwrap();
        assert!(snapshot.starts_with(clone.parent().unwrap().join(".baselines")));
        assert!(!snapshot.starts_with(&store));
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                fs::metadata(&snapshot).unwrap().dev(),
                fs::metadata(clone).unwrap().dev()
            );
        }
        assert_eq!(fs::read(clone.join("deps/pkg")).unwrap(), b"dependency");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_ne!(
                fs::metadata(source.join("app.txt")).unwrap().ino(),
                fs::metadata(clone.join("app.txt")).unwrap().ino()
            );
        }
        fs::write(clone.join("new.bin"), [0, 1, 2, 255]).unwrap();
        fs::write(clone.join("deps/pkg"), "mutated ignored dependency").unwrap();
        fs::write(clone.join("app.txt"), "first\ninherited\nthird\nagent\n").unwrap();
        assert_eq!(
            fs::read_to_string(source.join("app.txt")).unwrap(),
            "first\ninherited\nthird\n"
        );
        let snapshot = status(&store, &w).unwrap();
        let files = snapshot["files"].as_array().unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f["relative"] != "deps/pkg"));
        let result = apply(&store, &w, &source).unwrap();
        assert_eq!(result["alreadyApplied"], 0);
        assert_eq!(apply(&store, &w, &source).unwrap()["alreadyApplied"], 2);
        fs::write(clone.join("later.txt"), "later").unwrap();
        assert!(remove(&store, &w, false).is_err());
        text(clone, &["add", "app.txt", "new.bin", "later.txt"]).unwrap();
        text(clone, &["commit", "-m", "session"]).unwrap();
        let head = text(clone, &["rev-parse", "HEAD"]).unwrap();
        remove(&store, &w, false).unwrap();
        assert_eq!(
            text(&source, &["rev-parse", w.branch.as_deref().unwrap()]).unwrap(),
            head
        );
        assert!(!clone.exists());
    }
    #[cfg(unix)]
    #[test]
    fn linked_worktree_private_git_and_tracked_external_symlink_rejection() {
        let t = Temp::new();
        let source = t.repo();
        let store = t.0.join("registry");
        private_dir(&store).unwrap();
        if capability(&source)["supported"] != true {
            assert!(
                std::env::var_os("MONOCODE_REQUIRE_COW").is_none(),
                "Native CoW coverage is required: {}",
                capability(&source)
            );
            eprintln!("SKIP native APFS coverage: {}", capability(&source));
            return;
        }
        let linked = t.0.join("linked");
        text(
            &source,
            &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
        )
        .unwrap();
        std::os::unix::fs::symlink(linked.join("app.txt"), linked.join("internal-link")).unwrap();
        let w = create(&store, &linked, "linked-session", None, None).unwrap();
        let clone = Path::new(&w.path);
        assert!(clone.join(".git").is_dir());
        assert!(!fs::read_link(clone.join("internal-link"))
            .unwrap()
            .is_absolute());
        assert_eq!(
            fs::read_to_string(clone.join("internal-link")).unwrap(),
            "first\nsecond\nthird\n"
        );
        assert_eq!(
            text(clone, &["rev-parse", "--git-common-dir"]).unwrap(),
            ".git"
        );
        let snapshot = snapshot_directory(&store, clone, &w.id).unwrap();
        let legacy = store.join(&w.id).join("baseline");
        fs::rename(snapshot, &legacy).unwrap();
        assert_eq!(snapshot_directory(&store, clone, &w.id).unwrap(), legacy);
        assert_eq!(status(&store, &w).unwrap()["files"], json!([]));
        fs::write(t.0.join("outside"), "secret").unwrap();
        std::os::unix::fs::symlink(t.0.join("outside"), source.join("escaping-link")).unwrap();
        text(&source, &["add", "escaping-link"]).unwrap();
        assert!(create(&store, &source, "unsafe", None, None)
            .unwrap_err()
            .to_lowercase()
            .contains("external symlink"));
    }
    #[test]
    fn complete_large_delta_and_frozen_ignored_files() {
        let t = Temp::new();
        let source = t.repo();
        let store = t.0.join("registry");
        private_dir(&store).unwrap();
        if capability(&source)["supported"] != true {
            assert!(
                std::env::var_os("MONOCODE_REQUIRE_COW").is_none(),
                "Native CoW coverage required"
            );
            eprintln!("SKIP native APFS");
            return;
        }
        private_dir(&source.join("deps")).unwrap();
        fs::write(source.join("deps/secret"), "inherited ignored").unwrap();
        let w = create(&store, &source, "wide", None, None).unwrap();
        let root = Path::new(&w.path);
        for n in 0..501 {
            fs::write(root.join(format!("file-{n}.txt")), format!("{n}\n")).unwrap();
        }
        fs::write(root.join("large.bin"), vec![0u8; 3 * 1024 * 1024]).unwrap();
        fs::write(root.join(".gitignore"), "").unwrap();
        text(root, &["add", "-f", "deps/secret"]).unwrap();
        let result = status(&store, &w).unwrap();
        assert_eq!(result["files"].as_array().unwrap().len(), 503);
        assert!(result["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["relative"] != "deps/secret"));
        assert_eq!(
            file_diff(&store, &w, "large.bin").unwrap()["tooLarge"],
            true
        );
        let listed = dispatch(&store, json!({"command":"cow_list","args":{"cwd":w.path}})).unwrap();
        assert_eq!(listed.as_array().unwrap().len(), 1);
        assert!(load(&store, &t.0, &w.id).is_err());
    }
    #[test]
    fn internal_clone_review_and_cleanup_do_not_execute_filters_or_hooks() {
        let t = Temp::new();
        let source = t.repo();
        let store = t.0.join("registry");
        private_dir(&store).unwrap();
        if capability(&source)["supported"] != true {
            assert!(
                std::env::var_os("MONOCODE_REQUIRE_COW").is_none(),
                "Native CoW coverage required"
            );
            eprintln!("SKIP native APFS");
            return;
        }
        fs::write(source.join(".gitattributes"), "app.txt filter=hostile\n").unwrap();
        text(&source, &["add", "."]).unwrap();
        text(&source, &["commit", "-m", "attributes"]).unwrap();
        let marker = t.0.join("executed");
        let executable = format!("touch {}; cat", marker.display());
        let included = t.0.join("included.gitconfig");
        fs::write(&included, format!("[filter \"hostile\"]\nclean = \"{executable}\"\nsmudge = \"{executable}\"\nrequired = true\n")).unwrap();
        text(
            &source,
            &["config", "include.path", included.to_str().unwrap()],
        )
        .unwrap();
        let w = create(&store, &source, "filters", None, None).unwrap();
        fs::write(Path::new(&w.path).join("new.txt"), "session").unwrap();
        assert_eq!(
            config_values(Path::new(&w.path), "filter.hostile.clean", true).unwrap(),
            vec![executable]
        );
        status(&store, &w).unwrap();
        dispatch(&store, json!({"command":"cow_list","args":{"cwd":source}})).unwrap();
        remove(&store, &w, true).unwrap();
        assert!(!marker.exists());
    }
}
