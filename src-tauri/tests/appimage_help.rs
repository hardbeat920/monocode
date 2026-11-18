#![cfg(unix)]

use std::path::Path;
use std::process::Command;

#[test]
fn standalone_help_uses_only_its_own_appimage() {
    let exe = Path::new(env!("CARGO_BIN_EXE_monocode"))
        .canonicalize()
        .unwrap();
    let mount = exe.parent().unwrap();
    let image = "/opt/Mono Code's.AppImage";
    let help = |mount: Option<&Path>, image: Option<&str>| {
        let mut cmd = Command::new(&exe);
        cmd.env_clear().arg("--help");
        if let Some(mount) = mount {
            cmd.env("APPDIR", mount);
        }
        if let Some(image) = image {
            cmd.env("APPIMAGE", image);
        }
        let output = cmd.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    };

    let plain = help(None, None);
    let packaged = help(Some(mount), Some(image));
    if cfg!(target_os = "linux") {
        for subcommand in ["app", "control"] {
            let example = format!(r"'/opt/Mono Code'\''s.AppImage' {subcommand} --help");
            assert!(
                packaged.lines().any(|line| line.trim() == example),
                "{packaged}"
            );
        }
    } else {
        assert_eq!(packaged, plain);
    }
    assert_eq!(help(Some(Path::new("/dev")), Some(image)), plain);
    assert_eq!(help(None, Some(image)), plain);
    assert_eq!(help(Some(mount), None), plain);
    assert_eq!(help(Some(mount), Some("relative.AppImage")), plain);
}
