use crate::{io, Error, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

const MAX_ITEMS: usize = 4096;
const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DEPTH: usize = 32;
const MAX_INSTRUCTIONS: usize = 1024 * 1024;

#[derive(Debug)]
pub(crate) struct Bundle {
    pub name: String,
    pub description: String,
    pub digest: String,
    pub warnings: Vec<String>,
    items: Vec<Item>,
}

#[derive(Debug)]
struct Item {
    path: PathBuf,
    kind: Kind,
}

#[derive(Debug)]
enum Kind {
    Directory,
    File { bytes: Vec<u8>, executable: u32 },
}

impl Bundle {
    pub fn read(root: &Path, allow_internal_links: bool) -> Result<Self> {
        let metadata = fs::symlink_metadata(root).map_err(|e| io("Read bundle", root, e))?;
        if !allow_internal_links && metadata.file_type().is_symlink() {
            return Err(Error::InvalidBundle("Export is a symbolic link".into()));
        }
        let root = fs::canonicalize(root).map_err(|e| io("Resolve bundle", root, e))?;
        if !root.is_dir() {
            return Err(Error::InvalidBundle("Source must be a directory".into()));
        }
        let mut items = Vec::new();
        let mut total_bytes = 0;
        let mut ancestors = BTreeSet::new();
        walk(
            &root,
            &root,
            Path::new(""),
            allow_internal_links,
            &mut ancestors,
            &mut items,
            &mut total_bytes,
        )?;
        items.sort_by(|a, b| a.path.cmp(&b.path));
        let instructions = items
            .iter()
            .find(|item| item.path == Path::new("SKILL.md"))
            .and_then(|item| match &item.kind {
                Kind::File { bytes, .. } => Some(bytes),
                Kind::Directory => None,
            })
            .ok_or_else(|| Error::InvalidBundle("Missing root SKILL.md".into()))?;
        let (name, description, warnings) = frontmatter(instructions)?;
        let mut hash = Sha256::new();
        hash.update(b"monocode-skill-bundle-v1\0");
        for item in &items {
            let relative = portable_path(&item.path)?;
            hash.update((relative.len() as u64).to_le_bytes());
            hash.update(relative.as_bytes());
            match &item.kind {
                Kind::Directory => hash.update([0]),
                Kind::File { bytes, executable } => {
                    hash.update([1]);
                    hash.update(executable.to_le_bytes());
                    hash.update((bytes.len() as u64).to_le_bytes());
                    hash.update(bytes);
                }
            }
        }
        Ok(Self {
            name,
            description,
            digest: format!("{:x}", hash.finalize()),
            warnings,
            items,
        })
    }

    /// Write a materialized copy. Symlinks are never created.
    pub fn write_new(&self, destination: &Path) -> Result<()> {
        fs::create_dir(destination).map_err(|e| io("Create bundle copy", destination, e))?;
        for item in &self.items {
            let path = destination.join(&item.path);
            match &item.kind {
                Kind::Directory => {
                    fs::create_dir(&path).map_err(|e| io("Create resource directory", &path, e))?;
                }
                Kind::File { bytes, executable } => {
                    let mut file = File::create_new(&path)
                        .map_err(|e| io("Create resource file", &path, e))?;
                    file.write_all(bytes)
                        .map_err(|e| io("Write resource file", &path, e))?;
                    set_executable(&file, *executable)
                        .map_err(|e| io("Set resource permissions", &path, e))?;
                    file.sync_all()
                        .map_err(|e| io("Flush resource file", &path, e))?;
                }
            }
        }
        // Flush child directory entries before their parents and the bundle root.
        for item in self.items.iter().rev() {
            if matches!(item.kind, Kind::Directory) {
                crate::persistence::sync_directory(&destination.join(&item.path))?;
            }
        }
        crate::persistence::sync_directory(destination)?;
        Ok(())
    }
}

fn walk(
    root: &Path,
    physical: &Path,
    relative: &Path,
    allow_internal_links: bool,
    ancestors: &mut BTreeSet<PathBuf>,
    items: &mut Vec<Item>,
    total_bytes: &mut u64,
) -> Result<()> {
    if relative.components().count() > MAX_DEPTH {
        return Err(Error::InvalidBundle(
            "Bundle exceeds 32 directory levels".into(),
        ));
    }
    let canonical = fs::canonicalize(physical).map_err(|e| io("Resolve resource", physical, e))?;
    if !canonical.starts_with(root) {
        return Err(Error::InvalidBundle(format!(
            "Symbolic link leaves the skill directory: {}",
            relative.display()
        )));
    }
    if !ancestors.insert(canonical.clone()) {
        return Err(Error::InvalidBundle(format!(
            "Symbolic link cycle: {}",
            relative.display()
        )));
    }
    let mut children = fs::read_dir(&canonical)
        .map_err(|e| io("Read resource directory", &canonical, e))?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|e| io("Read resource directory", &canonical, e))?;
    children.sort_by_key(|entry| entry.file_name());
    let mut portable_names = BTreeSet::new();
    for child in children {
        if items.len() >= MAX_ITEMS {
            return Err(Error::InvalidBundle(
                "Bundle exceeds 4096 files and directories".into(),
            ));
        }
        let relative = relative.join(child.file_name());
        portable_path(&relative)?;
        let name = child.file_name().to_string_lossy().to_lowercase();
        if !portable_names.insert(name) {
            return Err(Error::InvalidBundle(
                "Resource paths collide on a case-insensitive filesystem".into(),
            ));
        }
        let path = child.path();
        let metadata = fs::symlink_metadata(&path).map_err(|e| io("Read resource", &path, e))?;
        if metadata.file_type().is_symlink() && !allow_internal_links {
            return Err(Error::InvalidBundle(format!(
                "Export contains a symbolic link: {}",
                relative.display()
            )));
        }
        let resolved = fs::canonicalize(&path).map_err(|e| io("Resolve resource", &path, e))?;
        if !resolved.starts_with(root) {
            return Err(Error::InvalidBundle(format!(
                "Symbolic link leaves the skill directory: {}",
                relative.display()
            )));
        }
        let metadata = fs::metadata(&resolved).map_err(|e| io("Read resource", &resolved, e))?;
        if metadata.is_dir() {
            items.push(Item {
                path: relative.clone(),
                kind: Kind::Directory,
            });
            walk(
                root,
                &resolved,
                &relative,
                allow_internal_links,
                ancestors,
                items,
                total_bytes,
            )?;
        } else if metadata.is_file() {
            if metadata.len() > MAX_BYTES.saturating_sub(*total_bytes) {
                return Err(Error::InvalidBundle("Bundle exceeds 64 MiB".into()));
            }
            let mut file = File::open(&resolved).map_err(|e| io("Read resource", &resolved, e))?;
            let mut bytes = Vec::new();
            Read::by_ref(&mut file)
                .take(MAX_BYTES.saturating_sub(*total_bytes) + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| io("Read resource", &resolved, e))?;
            *total_bytes += bytes.len() as u64;
            if *total_bytes > MAX_BYTES {
                return Err(Error::InvalidBundle("Bundle exceeds 64 MiB".into()));
            }
            items.push(Item {
                path: relative,
                kind: Kind::File {
                    bytes,
                    executable: executable(&metadata),
                },
            });
        } else {
            return Err(Error::InvalidBundle(format!(
                "Resource must be a file or directory: {}",
                relative.display()
            )));
        }
    }
    ancestors.remove(&canonical);
    Ok(())
}

fn portable_path(path: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(part) = component else {
            return Err(Error::InvalidBundle(
                "Invalid relative resource path".into(),
            ));
        };
        let part = part
            .to_str()
            .ok_or_else(|| Error::InvalidBundle("Resource paths must be valid UTF-8".into()))?;
        if part.contains(['\\', ':', '*', '?', '"', '<', '>', '|'])
            || part.ends_with(['.', ' '])
            || part.chars().any(char::is_control)
        {
            return Err(Error::InvalidBundle(format!(
                "Resource path is not portable: {part}"
            )));
        }
        let stem = part
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(Error::InvalidBundle(format!(
                "Resource path is reserved on Windows: {part}"
            )));
        }
        parts.push(part);
    }
    Ok(parts.join("/"))
}

pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

fn frontmatter(bytes: &[u8]) -> Result<(String, String, Vec<String>)> {
    if bytes.len() > MAX_INSTRUCTIONS {
        return Err(Error::InvalidBundle("SKILL.md exceeds 1 MiB".into()));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::InvalidBundle("SKILL.md must be UTF-8".into()))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return Err(Error::InvalidBundle(
            "SKILL.md must start with YAML frontmatter".into(),
        ));
    }
    let mut yaml = String::new();
    let mut closed = false;
    for line in lines {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    if !closed {
        return Err(Error::InvalidBundle(
            "SKILL.md frontmatter has no closing delimiter".into(),
        ));
    }
    let fields = crate::metadata::parse(&yaml)?;
    let name = fields.name.as_str();
    if !valid_name(name) {
        return Err(Error::InvalidBundle(
            "Name must contain 1 to 64 lowercase letters, digits, or hyphens, with no leading, trailing, or repeated hyphens".into(),
        ));
    }
    portable_path(Path::new(name))?;
    let description = fields.description.as_str();
    if description.trim().is_empty() || description.chars().count() > 1024 {
        return Err(Error::InvalidBundle(
            "Description must contain 1 to 1024 characters".into(),
        ));
    }
    let mut warnings = Vec::new();
    if !fields.extensions.is_empty() {
        warnings.push(format!(
            "Preserved provider metadata: {}. Provider behavior has not been verified.",
            fields.extensions.join(", ")
        ));
    }
    if fields.has_dependencies {
        warnings
            .push("Sharing files does not install required tools, plugins, or credentials.".into());
    }
    Ok((name.into(), description.into(), warnings))
}

#[cfg(unix)]
fn executable(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111
}

#[cfg(not(unix))]
fn executable(_: &fs::Metadata) -> u32 {
    0
}

#[cfg(unix)]
fn set_executable(file: &File, executable: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o644 | executable))
}

#[cfg(not(unix))]
fn set_executable(_: &File, _: u32) -> std::io::Result<()> {
    Ok(())
}
