use super::*;
use crate::{agent::Settings, store::Store};

#[test]
fn account_metadata_selection_and_removal_round_trip_without_credentials() {
    let mut book = Book::default();
    let account = book.add(Kind::Codex, "  Work   account  ").unwrap();
    assert_eq!(account.label, "Work account");
    let second = book.add(Kind::Codex, "Second").unwrap();
    book.rename(Kind::Codex, &account.id, "Work account")
        .unwrap();
    assert_eq!(
        book.list(Kind::Codex)
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>(),
        vec![DEFAULT, account.id.as_str(), second.id.as_str()]
    );
    book.select(Kind::Codex, "/work/project/", &account.id)
        .unwrap();
    assert_eq!(book.selected(Kind::Codex, "/work/project"), account.id);
    assert_eq!(book.selected(Kind::Claude, "/work/project"), DEFAULT);
    assert_eq!(book.selected(Kind::Codex, "/other"), DEFAULT);
    book.rename(Kind::Codex, DEFAULT, "Personal").unwrap();
    assert_eq!(book.list(Kind::Codex)[0].label, "Personal");
    assert!(book.remove(Kind::Codex, DEFAULT).is_err());
    assert!(book.add(Kind::Devin, "Second").is_err());
    assert!(book.add(Kind::OpenCode, "Second").is_err());
    let store = Store::open(Path::new(":memory:")).unwrap();
    store.save_provider_accounts(&book).unwrap();
    let mut saved = store.provider_accounts().unwrap();
    assert_eq!(saved.selected(Kind::Codex, "/work/project"), account.id);
    saved.remove(Kind::Codex, &account.id).unwrap();
    assert_eq!(saved.selected(Kind::Codex, "/work/project"), DEFAULT);
    assert!(!saved.exists(Kind::Codex, &account.id));
}

#[test]
fn isolated_profiles_override_environment_and_cannot_escape_their_root() {
    let root = tempfile::tempdir().unwrap();
    let profile = Profile::new("work-1", root.path());
    for kind in [Kind::Codex, Kind::Claude] {
        let mut command = Command::new("provider");
        command
            .env("OPENAI_API_KEY", "fixture")
            .env("ANTHROPIC_API_KEY", "fixture");
        profile.apply(kind, &mut command).unwrap();
        let env: HashMap<_, _> = command.get_envs().collect();
        let (home, secret) = if kind == Kind::Codex {
            ("CODEX_HOME", "OPENAI_API_KEY")
        } else {
            ("CLAUDE_CONFIG_DIR", "ANTHROPIC_API_KEY")
        };
        assert_eq!(
            env[std::ffi::OsStr::new(home)],
            profile
                .directory(kind)
                .unwrap()
                .as_deref()
                .map(Path::as_os_str)
        );
        assert_eq!(env[std::ffi::OsStr::new(secret)], None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(profile.home(kind).unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }
    for id in ["../outside", "a/b", "", "hello world"] {
        assert!(Profile::new(id, root.path())
            .directory(Kind::Codex)
            .is_err());
    }
    assert!(profile.directory(Kind::Devin).is_err());
    assert_eq!(Profile::new(DEFAULT, root.path()), Profile::default());
    assert!(Profile::default().remove_credentials(Kind::Codex).is_err());
    let serialized = serde_json::to_string(&Settings {
        account: profile,
        ..Default::default()
    })
    .unwrap();
    assert!(!serialized.contains(root.path().to_str().unwrap()));
    let saved: Settings = serde_json::from_str(&serialized).unwrap();
    assert_eq!(saved.account.id, "work-1");
    assert!(saved.account.root.is_none());
}

#[test]
fn removing_a_private_codex_profile_preserves_other_accounts() {
    let root = tempfile::tempdir().unwrap();
    let first = Profile::new("first", root.path());
    let second = Profile::new("second", root.path());
    for profile in [&first, &second] {
        profile
            .apply(Kind::Codex, &mut Command::new("fixture"))
            .unwrap();
        std::fs::write(
            profile.home(Kind::Codex).unwrap().join("auth.json"),
            "fixture",
        )
        .unwrap();
    }
    first.remove_credentials(Kind::Codex).unwrap();
    assert!(!first.home(Kind::Codex).unwrap().exists());
    assert!(second.home(Kind::Codex).unwrap().join("auth.json").exists());
}

#[cfg(unix)]
#[test]
fn account_cli_login_models_usage_reset_and_text_jobs_share_the_same_private_home() {
    use crate::{
        catalog,
        provider::{self, Request},
        usage,
    };
    use std::{
        os::unix::fs::PermissionsExt,
        sync::{atomic::AtomicBool, Arc},
    };
    struct Binary(std::ffi::OsString, Option<std::ffi::OsString>);
    impl Drop for Binary {
        fn drop(&mut self) {
            unsafe {
                if let Some(value) = &self.1 {
                    std::env::set_var(&self.0, value);
                } else {
                    std::env::remove_var(&self.0);
                }
            }
        }
    }
    let fixture = tempfile::tempdir().unwrap();
    let binary = fixture.path().join("provider");
    std::fs::write(&binary, r#"#!/usr/bin/env python3
import base64, json, os, pathlib, sys, uuid
home = pathlib.Path(os.environ['CODEX_HOME'])
assert 'OPENAI_API_KEY' not in os.environ
home.mkdir(parents=True, exist_ok=True)
label = home.name
if sys.argv[1:] == ['login']:
    payload = base64.urlsafe_b64encode(json.dumps({'email': label+'@fixture.test'}).encode()).decode().rstrip('=')
    (home/'auth.json').write_text(json.dumps({'tokens':{'id_token':'fixture.'+payload+'.fixture'}}))
    sys.exit(0)
if sys.argv[1:] == ['logout']:
    (home/'auth.json').unlink(missing_ok=True)
    sys.exit(0)
for line in sys.stdin:
    request = json.loads(line)
    method = request.get('method')
    params = request.get('params', {})
    with (home/'trace.jsonl').open('a') as f: f.write(json.dumps(request)+'\n')
    if method == 'initialized': continue
    result = {}
    if method == 'model/list':
        if params.get('cursor') == 'next': result = {'data':[{'model':label+'-page2','displayName':'ZZZ second page'}]}
        else: result = {'data':[{'model': label+'-'+(home/'model').read_text(), 'displayName':label}], 'nextCursor':'next'}
    elif method == 'account/rateLimits/read': result = {'rateLimits':{'primary':{'usedPercent':11 if label=='one' else 88,'windowDurationMins':300}}}
    elif method == 'account/rateLimitResetCredit/consume':
        assert params['creditId'] == 'fixture-credit' and uuid.UUID(params['idempotencyKey']).version == 4
        result = {'outcome':'reset'}
    elif method in ['thread/start','thread/resume']: result = {'thread':{'id':'fixture-thread'}}
    print(json.dumps({'id':request.get('id'), 'result':result}), flush=True)
    if method == 'turn/start':
        print(json.dumps({'method':'item/agentMessage/delta','params':{'itemId':'answer','delta':label}}), flush=True)
        print(json.dumps({'method':'turn/completed','params':{'turn':{'status':'completed'}}}), flush=True)
"#).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let key = std::ffi::OsString::from("MONOCODE_CODEX_BINARY");
    let _restore = Binary(key.clone(), std::env::var_os(&key));
    unsafe {
        std::env::set_var(&key, &binary);
    }
    for (name, used) in [("one", 11.), ("two", 88.)] {
        let profile = Profile::new(name, fixture.path());
        authenticate(
            Kind::Codex,
            &profile,
            AuthAction::SignIn,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(
            profile.identity(Kind::Codex).unwrap().email.unwrap(),
            format!("{name}@fixture.test")
        );
        let model = profile.home(Kind::Codex).unwrap().join("model");
        std::fs::write(&model, "first").unwrap();
        assert_eq!(
            catalog::models_for(Kind::Codex, &profile).unwrap()[0].id,
            format!("{name}-first")
        );
        assert_eq!(catalog::models_for(Kind::Codex, &profile).unwrap().len(), 2);
        assert!(catalog::models_for(Kind::Codex, &profile)
            .unwrap()
            .iter()
            .any(|m| m.id == format!("{name}-page2")));
        std::fs::write(&model, "second").unwrap();
        assert_eq!(
            catalog::models_for(Kind::Codex, &profile).unwrap()[0].id,
            format!("{name}-first")
        );
        catalog::invalidate(Kind::Codex, &profile);
        assert_eq!(
            catalog::models_for(Kind::Codex, &profile).unwrap()[0].id,
            format!("{name}-second")
        );
        assert_eq!(
            usage::fetch_for(Kind::Codex, &profile, None)
                .unwrap()
                .windows[0]
                .used_percent,
            used
        );
        assert_eq!(
            usage::consume_reset(&profile, Some("fixture-credit")).unwrap(),
            "reset"
        );
        let request = Request {
            kind: Kind::Codex,
            binary: binary.clone(),
            cwd: fixture.path().into(),
            prompt: "Title".into(),
            provider_id: None,
            model: None,
            settings: Settings {
                account: profile.clone(),
                ..Default::default()
            },
        };
        assert_eq!(
            provider::run_text(request, Arc::new(AtomicBool::new(false))).unwrap(),
            name
        );
        authenticate(
            Kind::Codex,
            &profile,
            AuthAction::SignOut,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert!(profile.identity(Kind::Codex).is_none());
    }
}

#[test]
fn claude_entrypoint_label_is_opt_in_for_every_profile() {
    let root = tempfile::tempdir().unwrap();
    let label = |on: bool, profile: &Profile| {
        set_claude_entrypoint(on);
        let mut command = Command::new("provider");
        command.env("CLAUDE_CODE_ENTRYPOINT", "sdk-cli");
        profile.apply(Kind::Claude, &mut command).unwrap();
        let env: HashMap<_, _> = command.get_envs().collect();
        env[std::ffi::OsStr::new("CLAUDE_CODE_ENTRYPOINT")].map(|v| v.to_owned())
    };
    for profile in [Profile::default(), Profile::new("work-1", root.path())] {
        assert_eq!(label(true, &profile), Some("monocode".into()));
        assert_eq!(label(false, &profile), Some("sdk-cli".into()));
    }
}
