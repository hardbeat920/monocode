//! Port of host/runtime.ts.
//!
//! The TypeScript host ran as `node <entry>`; this one runs as
//! `monocode-app host`. [`HostProgram`] names either, so the runtime copy,
//! the launcher, and the service definitions work the same way for both.

use std::fs;
use std::path::{Path, PathBuf};

/// How to start the host: an executable and the arguments before the host
/// command, such as `node /x/monocode-host.mjs` or `monocode-app host`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostProgram {
    pub executable: PathBuf,
    pub args: Vec<String>,
}

impl HostProgram {
    /// The full argument list for a host command, after the executable.
    pub fn args_for(&self, rest: &[String]) -> Vec<String> {
        self.args
            .iter()
            .cloned()
            .chain(rest.iter().cloned())
            .collect()
    }

    /// `serve --data-dir <directory> --port <port>`.
    pub fn serve_args(&self, directory: &Path, port: u16) -> Vec<String> {
        self.args_for(&[
            "serve".into(),
            "--data-dir".into(),
            directory.to_string_lossy().into_owned(),
            "--port".into(),
            port.to_string(),
        ])
    }
}

/// The files a host runtime consists of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeBundle {
    /// Required files. The first is the entry point.
    pub files: Vec<String>,
    /// Files copied when present.
    pub optional: Vec<String>,
}

impl RuntimeBundle {
    /// The Node host's bundle; see host/build.mjs.
    pub fn node() -> Self {
        Self {
            files: vec!["monocode-host.mjs".into(), "provider-guard.mjs".into()],
            optional: vec!["monocode-host.mjs.map".into()],
        }
    }

    /// A single native executable, such as `monocode-app`.
    pub fn native(file: &str) -> Self {
        Self {
            files: vec![file.into()],
            optional: Vec::new(),
        }
    }
}

pub struct InstallRuntime<'a> {
    pub directory: &'a Path,
    pub version: &'a str,
    /// The folder holding the running bundle, such as the copy in npx's cache.
    pub source: &'a Path,
    pub bundle: &'a RuntimeBundle,
    /// The interpreter that runs the entry point, such as Node. `None` runs
    /// the entry point itself.
    pub interpreter: Option<&'a Path>,
    /// Arguments after the entry point, such as `host`.
    pub args: &'a [String],
    /// `process.platform`.
    pub platform: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledRuntime {
    /// Runs the installed copy.
    pub program: HostProgram,
    /// The host entry point inside the data directory.
    pub entry: PathBuf,
    /// A stable command for managing the host, such as
    /// `~/.monocode-host/bin/monocode-host`.
    pub launcher: PathBuf,
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn same_path(a: &Path, b: &Path) -> bool {
    let absolute = |path: &Path| std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    absolute(a) == absolute(b)
}

fn create_private_dir(path: &Path, recursive: bool) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(recursive);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

/// Copies the running host bundle to `<data dir>/runtime/<version>`. `npx`
/// runs packages from a cache that npm may clear at any time, so the
/// background service must not point into it.
pub fn install_runtime(options: InstallRuntime<'_>) -> Result<InstalledRuntime, String> {
    let error = |error: std::io::Error| error.to_string();
    let runtimes = options.directory.join("runtime");
    let target = runtimes.join(options.version);
    // Running `connect` from an installed runtime reuses it in place.
    if !same_path(options.source, &target) {
        create_private_dir(&runtimes, true).map_err(error)?;
        // Copy into a fresh folder and rename it into place, so a concurrent
        // install never leaves a half-copied runtime behind.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis())
            .unwrap_or(0);
        let staging = runtimes.join(format!(
            ".{}-{}-{stamp}",
            options.version,
            std::process::id()
        ));
        create_private_dir(&staging, false).map_err(error)?;
        let copied = (|| -> Result<(), String> {
            for file in options.bundle.files.iter().chain(&options.bundle.optional) {
                let from = options.source.join(file);
                if from.exists() {
                    fs::copy(&from, staging.join(file)).map_err(error)?;
                } else if options.bundle.files.contains(file) {
                    return Err(format!("The host package is missing {file}"));
                }
            }
            if target.exists() {
                fs::remove_dir_all(&target).map_err(error)?;
            }
            fs::rename(&staging, &target).map_err(error)
        })();
        if let Err(message) = copied {
            let _ = fs::remove_dir_all(&staging);
            return Err(message);
        }
    }
    let entry = target.join(&options.bundle.files[0]);
    let program = match options.interpreter {
        Some(interpreter) => HostProgram {
            executable: interpreter.to_path_buf(),
            args: std::iter::once(entry.to_string_lossy().into_owned())
                .chain(options.args.iter().cloned())
                .collect(),
        },
        None => HostProgram {
            executable: entry.clone(),
            args: options.args.to_vec(),
        },
    };
    let bin = options.directory.join("bin");
    create_private_dir(&bin, true).map_err(error)?;
    let windows = options.platform == "win32";
    let launcher = bin.join(if windows {
        "monocode-host.cmd"
    } else {
        "monocode-host"
    });
    let contents = if windows {
        let mut line = format!("\"{}\"", program.executable.display());
        for arg in &program.args {
            line.push_str(&format!(" \"{arg}\""));
        }
        format!("@echo off\r\n{line} %*\r\n")
    } else {
        let mut line = shell_quote(&program.executable.to_string_lossy());
        for arg in &program.args {
            line.push(' ');
            line.push_str(&shell_quote(arg));
        }
        format!("#!/bin/sh\nexec {line} \"$@\"\n")
    };
    let temporary = PathBuf::from(format!("{}.tmp", launcher.display()));
    let mut file_options = fs::OpenOptions::new();
    file_options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        file_options.mode(0o700);
    }
    std::io::Write::write_all(
        &mut file_options.open(&temporary).map_err(error)?,
        contents.as_bytes(),
    )
    .map_err(error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o700)).map_err(error)?;
    }
    fs::rename(&temporary, &launcher).map_err(error)?;
    Ok(InstalledRuntime {
        program,
        entry,
        launcher,
    })
}

/// Removes runtimes other than `keep`, after the new host is running.
pub fn prune_runtimes(directory: &Path, keep: &Path) {
    let runtimes = directory.join("runtime");
    let Ok(entries) = fs::read_dir(&runtimes) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !same_path(&path, keep) {
            let _ = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
        }
    }
    // Hosts installed from release archives kept this pointer beside `bin`.
    let _ = fs::remove_file(directory.join("runtime-path"));
}

/// The Node executable for the background service. Homebrew and similar
/// installs run Node from a versioned folder that an upgrade deletes; a PATH
/// entry that links to the same binary, such as /opt/homebrew/bin/node,
/// survives the upgrade.
pub fn stable_node_path(executable: &Path, path: &str, platform: &str) -> PathBuf {
    let Ok(real) = fs::canonicalize(executable) else {
        return executable.to_path_buf();
    };
    let separator = if platform == "win32" { ';' } else { ':' };
    for folder in path.split(separator) {
        if folder.is_empty() {
            continue;
        }
        let candidate = Path::new(folder).join(if platform == "win32" {
            "node.exe"
        } else {
            "node"
        });
        if same_path(&candidate, executable) {
            continue;
        }
        if fs::canonicalize(&candidate).is_ok_and(|resolved| resolved == real) {
            return candidate;
        }
    }
    executable.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_the_bundle_out_of_npxs_cache_and_writes_a_launcher() {
        let root = crate::host::store::tests::temporary("monocode-runtime-");
        let source = root.path().join("npx-cache");
        let data = root.path().join("data");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&data).unwrap();
        fs::write(source.join("monocode-host.mjs"), "host").unwrap();
        fs::write(source.join("provider-guard.mjs"), "guard").unwrap();
        let bundle = RuntimeBundle::node();
        let install = |source: &Path| {
            install_runtime(InstallRuntime {
                directory: &data,
                version: "1.2.3",
                source,
                bundle: &bundle,
                interpreter: Some(Path::new("/usr/bin/node")),
                args: &[],
                platform: "linux",
            })
            .unwrap()
        };
        let runtime = install(&source);
        assert_eq!(
            runtime.entry,
            data.join("runtime").join("1.2.3").join("monocode-host.mjs")
        );
        assert_eq!(
            fs::read_to_string(
                data.join("runtime")
                    .join("1.2.3")
                    .join("provider-guard.mjs")
            )
            .unwrap(),
            "guard"
        );
        assert_eq!(
            fs::read_to_string(&runtime.launcher).unwrap(),
            format!(
                "#!/bin/sh\nexec '/usr/bin/node' '{}' \"$@\"\n",
                runtime.entry.display()
            )
        );
        assert_eq!(
            runtime.program,
            HostProgram {
                executable: "/usr/bin/node".into(),
                args: vec![runtime.entry.to_string_lossy().into_owned()],
            }
        );
        // Running connect again from the installed copy keeps it in place.
        install(&data.join("runtime").join("1.2.3"));
        assert!(runtime.entry.exists());

        fs::create_dir(data.join("runtime").join("0.4.3-linux-x64-.install.abc")).unwrap();
        fs::write(data.join("runtime-path"), "old").unwrap();
        prune_runtimes(&data, &data.join("runtime").join("1.2.3"));
        assert!(
            !data
                .join("runtime")
                .join("0.4.3-linux-x64-.install.abc")
                .exists()
        );
        assert!(!data.join("runtime-path").exists());
        assert!(runtime.entry.exists());
    }

    #[test]
    fn refuses_an_incomplete_bundle() {
        let root = crate::host::store::tests::temporary("monocode-runtime-");
        fs::write(root.path().join("monocode-host.mjs"), "host").unwrap();
        let error = install_runtime(InstallRuntime {
            directory: &root.path().join("data"),
            version: "1.0.0",
            source: root.path(),
            bundle: &RuntimeBundle::node(),
            interpreter: Some(Path::new("node")),
            args: &[],
            platform: "linux",
        })
        .unwrap_err();
        assert!(error.contains("provider-guard.mjs"), "{error}");
    }

    #[test]
    fn installs_a_native_host_that_runs_itself_with_a_subcommand() {
        let root = crate::host::store::tests::temporary("monocode-runtime-");
        let source = root.path().join("download");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("monocode-app"), "binary").unwrap();
        let runtime = install_runtime(InstallRuntime {
            directory: &root.path().join("data"),
            version: "0.6.0",
            source: &source,
            bundle: &RuntimeBundle::native("monocode-app"),
            interpreter: None,
            args: &["host".into()],
            platform: "win32",
        })
        .unwrap();
        assert_eq!(runtime.program.executable, runtime.entry);
        assert_eq!(runtime.program.args, ["host"]);
        assert_eq!(
            fs::read_to_string(&runtime.launcher).unwrap(),
            format!(
                "@echo off\r\n\"{}\" \"host\" %*\r\n",
                runtime.entry.display()
            )
        );
        assert_eq!(
            runtime.program.serve_args(Path::new("/d"), 3774),
            ["host", "serve", "--data-dir", "/d", "--port", "3774"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn prefers_a_path_link_to_node_over_its_versioned_install_folder() {
        let root = crate::host::store::tests::temporary("monocode-runtime-");
        let cellar = root.path().join("Cellar/node/25.0.0/bin");
        let bin = root.path().join("bin");
        fs::create_dir_all(&cellar).unwrap();
        fs::create_dir(&bin).unwrap();
        fs::write(cellar.join("node"), "").unwrap();
        std::os::unix::fs::symlink(cellar.join("node"), bin.join("node")).unwrap();
        assert_eq!(
            stable_node_path(
                &cellar.join("node"),
                &format!("{}:{}", cellar.display(), bin.display()),
                "darwin"
            ),
            bin.join("node")
        );
        assert_eq!(
            stable_node_path(&cellar.join("node"), &cellar.to_string_lossy(), "darwin"),
            cellar.join("node")
        );
    }
}
