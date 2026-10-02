//! Command-line flags.

use std::path::PathBuf;

use anyhow::{Context as _, Result, bail};

pub const USAGE: &str = "\
usage: monocode-app [--view <name>] [--theme dark|light|system] [--size WxH]
                    [--ui-scale <0.5..2>] [--screenshot <out.png>]
                    [--backdrop <#rrggbb|none>]

  --view <name>          Which view fills the window. Default: shell.
                         Run with --list-views to print the names.
  --theme <scheme>       Overrides the color scheme preference.
  --size WxH             Window content size in points. Default: 1280x800.
  --ui-scale <factor>    Interface scale, like the Appearance setting. Default: 1.
  --screenshot <path>    Writes the first settled frame as a PNG and exits.
                         Needs a build with --features screenshot.
  --backdrop <color>     Screenshot only: the color the transparent window is
                         composited over, standing in for the blurred desktop.
                         Default: #5f5560. `none` keeps the alpha channel.
  --list-views           Prints the view names and exits.
";

#[derive(Clone, Debug)]
pub struct Args {
    pub view: String,
    pub theme: Option<String>,
    pub size: (f32, f32),
    pub ui_scale: Option<f32>,
    pub screenshot: Option<PathBuf>,
    pub backdrop: Option<[u8; 3]>,
    pub list_views: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            view: "shell".into(),
            theme: None,
            size: (1280.0, 800.0),
            ui_scale: None,
            screenshot: None,
            backdrop: Some([0x5f, 0x55, 0x60]),
            list_views: false,
        }
    }
}

fn parse_size(value: &str) -> Result<(f32, f32)> {
    let (w, h) = value
        .split_once(['x', 'X'])
        .context("--size takes WxH, for example 1280x800")?;
    let w: f32 = w.trim().parse().context("--size width")?;
    let h: f32 = h.trim().parse().context("--size height")?;
    if w < 200.0 || h < 200.0 {
        bail!("--size must be at least 200x200");
    }
    Ok((w, h))
}

fn parse_color(value: &str) -> Result<Option<[u8; 3]>> {
    if value == "none" {
        return Ok(None);
    }
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() != 6 {
        bail!("--backdrop takes #rrggbb or none");
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).context("--backdrop color");
    Ok(Some([byte(0)?, byte(2)?, byte(4)?]))
}

impl Args {
    pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut out = Args::default();
        while let Some(arg) = args.next() {
            let mut value =
                |name: &str| args.next().with_context(|| format!("{name} needs a value"));
            match arg.as_str() {
                "--view" => out.view = value("--view")?,
                "--theme" => {
                    let theme = value("--theme")?;
                    if !matches!(theme.as_str(), "dark" | "light" | "system") {
                        bail!("--theme takes dark, light, or system");
                    }
                    out.theme = Some(theme);
                }
                "--size" => out.size = parse_size(&value("--size")?)?,
                "--ui-scale" => {
                    let scale: f32 = value("--ui-scale")?
                        .parse()
                        .context("--ui-scale takes a number")?;
                    out.ui_scale = Some(scale);
                }
                "--screenshot" => out.screenshot = Some(PathBuf::from(value("--screenshot")?)),
                "--backdrop" => out.backdrop = parse_color(&value("--backdrop")?)?,
                "--list-views" => out.list_views = true,
                "-h" | "--help" => {
                    print!("{USAGE}");
                    std::process::exit(0);
                }
                other => bail!("unknown argument {other}\n\n{USAGE}"),
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args> {
        Args::parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults() {
        let args = parse(&[]).unwrap();
        assert_eq!(args.view, "shell");
        assert_eq!(args.size, (1280.0, 800.0));
        assert!(args.screenshot.is_none());
    }

    #[test]
    fn reads_every_flag() {
        let args = parse(&[
            "--screenshot",
            "/tmp/a.png",
            "--size",
            "900x600",
            "--view",
            "widgets",
            "--theme",
            "light",
            "--backdrop",
            "none",
        ])
        .unwrap();
        assert_eq!(args.screenshot.unwrap().to_str(), Some("/tmp/a.png"));
        assert_eq!(args.size, (900.0, 600.0));
        assert_eq!(args.view, "widgets");
        assert_eq!(args.theme.as_deref(), Some("light"));
        assert_eq!(args.backdrop, None);
    }

    #[test]
    fn rejects_bad_values() {
        assert!(parse(&["--size", "big"]).is_err());
        assert!(parse(&["--theme", "sepia"]).is_err());
        assert!(parse(&["--backdrop", "#12"]).is_err());
        assert!(parse(&["--nope"]).is_err());
    }
}
