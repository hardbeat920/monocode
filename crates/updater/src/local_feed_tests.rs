//! End to end against a local HTTP server, with a minisign key generated for
//! each test: check the feed, download with progress, verify, and install
//! into a throwaway app (an `.app` on macOS, an AppImage on Linux). Windows
//! stops before install, because installing starts the NSIS installer and
//! exits the process.

use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::executor::block_on;

use crate::config::{Installer, UpdaterConfig, target};
use crate::error::Error;
use crate::sign::{GeneratedKeys, decode_secret_key, generate_keys, sign_bytes};
use crate::updater::{DownloadEvent, Updater};

#[derive(Clone)]
struct Route {
    status: u16,
    reason: &'static str,
    body: Vec<u8>,
}

impl Route {
    fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            reason: "OK",
            body: body.into(),
        }
    }

    fn status(status: u16, reason: &'static str) -> Self {
        Self {
            status,
            reason,
            body: Vec::new(),
        }
    }
}

/// A one-thread HTTP/1.1 server that answers each request from `routes` and
/// closes the connection. It lives until the test process exits.
struct Server {
    base: String,
    hits: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn start(routes: Vec<(&str, Route)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes: HashMap<String, Route> = routes
            .into_iter()
            .map(|(path, route)| (path.to_string(), route))
            .collect();
        let hits = Arc::new(Mutex::new(Vec::new()));
        let log = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = answer(stream, &routes, &log);
            }
        });
        Self { base, hits }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn hits(&self) -> Vec<String> {
        self.hits.lock().unwrap().clone()
    }
}

fn answer(
    mut stream: TcpStream,
    routes: &HashMap<String, Route>,
    hits: &Mutex<Vec<String>>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" {
            break;
        }
    }
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();
    hits.lock().unwrap().push(path.clone());
    let route = routes
        .get(&path)
        .cloned()
        .unwrap_or_else(|| Route::status(404, "Not Found"));
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        route.status,
        route.reason,
        route.body.len()
    )?;
    stream.write_all(&route.body)?;
    stream.flush()
}

/// A port nothing listens on.
fn closed_port_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{addr}/latest.json")
}

fn keys() -> GeneratedKeys {
    generate_keys(None).unwrap()
}

fn sign(keys: &GeneratedKeys, data: &[u8]) -> String {
    let sk = decode_secret_key(&keys.private_key, None).unwrap();
    sign_bytes(&sk, data, "package").unwrap()
}

/// `latest.json` in the shape release.yml writes, with one entry for this
/// platform.
fn feed(version: &str, package_url: &str, signature: &str) -> String {
    serde_json::json!({
        "version": version,
        "notes": format!("MonoCode {version}"),
        "pub_date": "2026-10-02T12:34:31.123Z",
        "platforms": {
            target().unwrap(): { "signature": signature, "url": package_url },
        },
    })
    .to_string()
}

/// The app being updated, in this platform's install shape.
struct InstalledApp {
    _dir: tempfile::TempDir,
    executable: PathBuf,
    /// What install replaces: the `.app` or the AppImage.
    root: PathBuf,
}

fn installed_app() -> InstalledApp {
    let dir = tempfile::tempdir().unwrap();
    if cfg!(target_os = "macos") {
        let root = dir.path().join("Applications/MonoCode.app");
        let macos = root.join("Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::write(root.join("Contents/Info.plist"), "old plist").unwrap();
        std::fs::write(macos.join("monocode"), "old binary").unwrap();
        InstalledApp {
            executable: macos.join("monocode"),
            root,
            _dir: dir,
        }
    } else {
        let root = dir.path().join("MonoCode_0.1.0_amd64.AppImage");
        std::fs::write(&root, "old appimage").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        InstalledApp {
            executable: root.clone(),
            root,
            _dir: dir,
        }
    }
}

/// The package release.yml publishes for this platform: an `.app.tar.gz`
/// whose entries start with `MonoCode.app/` on macOS, a bare AppImage on
/// Linux, and a stand-in setup.exe on Windows.
fn package() -> Vec<u8> {
    if cfg!(target_os = "macos") {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);
        let mut add = |path: &str, mode: u32, data: &[u8]| {
            let mut header = tar::Header::new_gnu();
            header.set_mode(mode);
            header.set_size(data.len() as u64);
            if path.ends_with('/') {
                header.set_entry_type(tar::EntryType::Directory);
            }
            header.set_cksum();
            tar.append_data(&mut header, path, data).unwrap();
        };
        add("MonoCode.app/", 0o755, b"");
        add("MonoCode.app/Contents/", 0o755, b"");
        add("MonoCode.app/Contents/Info.plist", 0o644, b"new plist");
        add("MonoCode.app/Contents/MacOS/", 0o755, b"");
        add("MonoCode.app/Contents/MacOS/MonoCode", 0o755, b"new binary");
        tar.into_inner().unwrap().finish().unwrap()
    } else if cfg!(windows) {
        b"MZ stand-in installer".to_vec()
    } else {
        b"new appimage".repeat(10_000)
    }
}

fn config(endpoints: Vec<String>, keys: &GeneratedKeys, app: &InstalledApp) -> UpdaterConfig {
    let mut config = UpdaterConfig::new("0.1.0");
    config.endpoints = endpoints;
    config.pubkey = keys.public_key.clone();
    config.executable_path = Some(app.executable.clone());
    config.installer = Some(if cfg!(target_os = "macos") {
        Installer::App
    } else if cfg!(windows) {
        Installer::Nsis
    } else {
        Installer::AppImage
    });
    config.dangerous_insecure_transport_protocol = true;
    config.no_proxy = true;
    config
}

#[test]
fn updates_end_to_end_from_a_local_feed() {
    let keys = keys();
    let package = package();
    let signature = sign(&keys, &package);
    let app = installed_app();
    // The feed needs the package URL, so the package has its own server.
    let package_server = Server::start(vec![("/pkg", Route::ok(package.clone()))]);
    let feed_server = Server::start(vec![(
        "/latest.json",
        Route::ok(feed("0.2.0", &package_server.url("/pkg"), &signature)),
    )]);

    let updater = Updater::new(config(vec![feed_server.url("/latest.json")], &keys, &app)).unwrap();
    let update = block_on(updater.check_async())
        .unwrap()
        .expect("0.2.0 is newer");

    assert_eq!(update.version, "0.2.0");
    assert_eq!(update.current_version, "0.1.0");
    assert_eq!(update.body.as_deref(), Some("MonoCode 0.2.0"));
    assert_eq!(update.download_url.as_str(), package_server.url("/pkg"));

    if cfg!(windows) {
        let bytes = update.download(|_| {}).unwrap();
        assert_eq!(bytes, package);
        return;
    }

    let mut events = Vec::new();
    block_on(update.download_and_install_async(|event| events.push(event))).unwrap();

    assert_eq!(
        events.first(),
        Some(&DownloadEvent::Started {
            content_length: Some(package.len() as u64)
        })
    );
    assert_eq!(events.last(), Some(&DownloadEvent::Finished));
    let downloaded: usize = events
        .iter()
        .map(|event| match event {
            DownloadEvent::Progress { chunk_length } => *chunk_length,
            _ => 0,
        })
        .sum();
    assert_eq!(downloaded, package.len());

    assert_installed(&app.root);
}

#[cfg(target_os = "macos")]
fn assert_installed(root: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let binary = root.join("Contents/MacOS/MonoCode");
    assert_eq!(std::fs::read(&binary).unwrap(), b"new binary");
    assert_eq!(
        std::fs::read(root.join("Contents/Info.plist")).unwrap(),
        b"new plist"
    );
    assert!(
        !root.join("Contents/MacOS/monocode").exists()
            || std::fs::read(root.join("Contents/MacOS/monocode")).unwrap() == b"new binary",
        "the old Tauri binary is gone (case-insensitive file systems see one file)"
    );
    assert_eq!(
        std::fs::metadata(&binary).unwrap().permissions().mode() & 0o111,
        0o111
    );
}

#[cfg(all(unix, not(target_os = "macos")))]
fn assert_installed(root: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    assert_eq!(std::fs::read(root).unwrap(), package());
    assert_eq!(
        std::fs::metadata(root).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[cfg(windows)]
fn assert_installed(_root: &Path) {}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn installs_an_appimage_from_a_tarball() {
    let keys = keys();
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);
    let mut header = tar::Header::new_gnu();
    header.set_mode(0o755);
    header.set_size(9);
    header.set_cksum();
    tar.append_data(
        &mut header,
        "MonoCode_0.2.0_amd64.AppImage",
        &b"appimage!"[..],
    )
    .unwrap();
    let package = tar.into_inner().unwrap().finish().unwrap();
    let signature = sign(&keys, &package);
    let app = installed_app();
    let package_server = Server::start(vec![("/pkg", Route::ok(package))]);
    let feed_server = Server::start(vec![(
        "/latest.json",
        Route::ok(feed("0.2.0", &package_server.url("/pkg"), &signature)),
    )]);

    let updater = Updater::new(config(vec![feed_server.url("/latest.json")], &keys, &app)).unwrap();
    let update = updater.check().unwrap().unwrap();
    update.download_and_install(|_| {}).unwrap();

    assert_eq!(std::fs::read(&app.root).unwrap(), b"appimage!");
}

#[test]
fn rejects_a_package_signed_with_another_key() {
    let keys = keys();
    let other = generate_keys(None).unwrap();
    let package = package();
    let app = installed_app();
    let package_server = Server::start(vec![("/pkg", Route::ok(package.clone()))]);
    let feed_server = Server::start(vec![(
        "/latest.json",
        Route::ok(feed(
            "0.2.0",
            &package_server.url("/pkg"),
            &sign(&other, &package),
        )),
    )]);

    let updater = Updater::new(config(vec![feed_server.url("/latest.json")], &keys, &app)).unwrap();
    let update = updater.check().unwrap().unwrap();
    let err = block_on(update.download_and_install_async(|_| {})).unwrap_err();

    assert!(matches!(err, Error::Minisign(_)), "{err}");
    assert_untouched(&app);
}

#[test]
fn rejects_a_tampered_package() {
    let keys = keys();
    let package = package();
    let signature = sign(&keys, &package);
    let mut tampered = package.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xFF;
    let app = installed_app();
    let package_server = Server::start(vec![("/pkg", Route::ok(tampered))]);
    let feed_server = Server::start(vec![(
        "/latest.json",
        Route::ok(feed("0.2.0", &package_server.url("/pkg"), &signature)),
    )]);

    let updater = Updater::new(config(vec![feed_server.url("/latest.json")], &keys, &app)).unwrap();
    let update = updater.check().unwrap().unwrap();
    let err = update.download_and_install(|_| {}).unwrap_err();

    assert!(
        matches!(
            err,
            Error::Minisign(minisign_verify::Error::InvalidSignature)
        ),
        "{err}"
    );
    assert_untouched(&app);
}

fn assert_untouched(app: &InstalledApp) {
    let old: &[u8] = if cfg!(target_os = "macos") {
        b"old binary"
    } else {
        b"old appimage"
    };
    assert_eq!(std::fs::read(&app.executable).unwrap(), old);
}

#[test]
fn reports_no_update_when_the_feed_is_not_newer() {
    let keys = keys();
    let app = installed_app();
    for version in ["0.1.0", "0.0.9", "v0.1.0"] {
        let server = Server::start(vec![(
            "/latest.json",
            Route::ok(feed(version, "https://cdn.example/pkg", "c2ln")),
        )]);
        let updater = Updater::new(config(vec![server.url("/latest.json")], &keys, &app)).unwrap();
        assert!(updater.check().unwrap().is_none(), "{version}");
    }
}

#[test]
fn no_content_means_current() {
    let keys = keys();
    let app = installed_app();
    let server = Server::start(vec![("/latest.json", Route::status(204, "No Content"))]);
    let updater = Updater::new(config(vec![server.url("/latest.json")], &keys, &app)).unwrap();
    assert!(updater.check().unwrap().is_none());
}

#[test]
fn falls_back_to_the_next_endpoint() {
    let keys = keys();
    let app = installed_app();
    let server = Server::start(vec![(
        "/good.json",
        Route::ok(feed("0.2.0", "https://cdn.example/pkg", "c2ln")),
    )]);

    let endpoints = vec![
        closed_port_url(),
        server.url("/missing.json"),
        server.url("/good.json"),
    ];
    let updater = Updater::new(config(endpoints, &keys, &app)).unwrap();
    let update = updater.check().unwrap().unwrap();

    assert_eq!(update.version, "0.2.0");
    assert_eq!(server.hits(), ["/missing.json", "/good.json"]);
}

#[test]
fn reports_the_last_transport_error() {
    let keys = keys();
    let app = installed_app();
    let updater = Updater::new(config(vec![closed_port_url()], &keys, &app)).unwrap();
    assert!(matches!(updater.check(), Err(Error::Http(_))));
}

#[test]
fn reports_release_not_found_when_every_endpoint_fails_with_a_status() {
    let keys = keys();
    let app = installed_app();
    let server = Server::start(vec![("/latest.json", Route::status(500, "Server Error"))]);
    let updater = Updater::new(config(vec![server.url("/latest.json")], &keys, &app)).unwrap();
    let err = updater.check().unwrap_err();
    assert!(matches!(err, Error::ReleaseNotFound));
    assert_eq!(
        err.to_string(),
        "Could not fetch a valid release JSON from the remote"
    );
}

#[test]
fn reports_a_feed_without_this_platform() {
    let keys = keys();
    let app = installed_app();
    let body = serde_json::json!({
        "version": "0.2.0",
        "platforms": { "plan9-mips": { "signature": "c2ln", "url": "https://cdn.example/pkg" } },
    })
    .to_string();
    let server = Server::start(vec![("/latest.json", Route::ok(body))]);
    let updater = Updater::new(config(vec![server.url("/latest.json")], &keys, &app)).unwrap();

    let Err(Error::TargetsNotFound(targets)) = updater.check() else {
        panic!("expected TargetsNotFound");
    };
    let base = target().unwrap();
    assert_eq!(targets.last(), Some(&base));
    assert!(targets[0].starts_with(&base));
}

#[test]
fn reports_a_download_status() {
    let keys = keys();
    let app = installed_app();
    let server = Server::start(vec![]);
    let feed_server = Server::start(vec![(
        "/latest.json",
        Route::ok(feed("0.2.0", &server.url("/gone"), "c2ln")),
    )]);
    let updater = Updater::new(config(vec![feed_server.url("/latest.json")], &keys, &app)).unwrap();
    let update = updater.check().unwrap().unwrap();

    let err = update.download(|_| {}).unwrap_err();
    assert_eq!(
        err.to_string(),
        "`Download request failed with status: 404 Not Found`"
    );
}

#[test]
fn fills_endpoint_placeholders() {
    let keys = keys();
    let app = installed_app();
    let server = Server::start(vec![]);
    let endpoint = server.url("/{{target}}/{{arch}}/{{current_version}}/{{bundle_type}}.json");
    let updater = Updater::new(config(vec![endpoint], &keys, &app)).unwrap();

    let _ = updater.check();

    let os = crate::config::updater_os().unwrap();
    let arch = crate::config::updater_arch().unwrap();
    let bundle = config(vec![], &keys, &app).installer.unwrap().name();
    assert_eq!(server.hits(), [format!("/{os}/{arch}/0.1.0/{bundle}.json")]);
}

#[test]
fn a_build_without_endpoints_is_not_configured() {
    let err = Updater::new(UpdaterConfig::new("0.1.0")).unwrap_err();
    assert!(matches!(err, Error::EmptyEndpoints));
    assert_eq!(err.to_string(), "Updater does not have any endpoints set.");
}

#[test]
fn a_bad_current_version_is_an_error() {
    let mut config = UpdaterConfig::new("not a version");
    config.endpoints = vec!["https://cdn.example/latest.json".into()];
    assert!(matches!(Updater::new(config), Err(Error::Semver(_))));
}
