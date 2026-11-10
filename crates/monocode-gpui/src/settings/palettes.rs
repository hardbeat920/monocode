//! Preset colors from workspaceThemes.ts, including surface tints and accents.
#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub surface: u32,
    pub panel: u32,
    pub solid: u32,
    pub content: u32,
    pub accent: u32,
}
pub(crate) const PALETTES: &[(&str, &str, Palette, Palette)] = &[
    (
        "default",
        "Default",
        Palette {
            surface: 0x0d0d0d,
            panel: 0x060606,
            solid: 0x141414,
            content: 0xebebeb,
            accent: 0x4da3f5,
        },
        Palette {
            surface: 0xf5f5f5,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x2e2e2e,
            accent: 0x4da3f5,
        },
    ),
    (
        "harbor",
        "Harbor",
        Palette {
            surface: 0x0b1018,
            panel: 0x060a10,
            solid: 0x121a26,
            content: 0xdbe4f0,
            accent: 0x5b9cf2,
        },
        Palette {
            surface: 0xeef2f7,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x1b2533,
            accent: 0x2f6fd0,
        },
    ),
    (
        "moss",
        "Moss",
        Palette {
            surface: 0x0d110e,
            panel: 0x070a08,
            solid: 0x151b17,
            content: 0xdde5dc,
            accent: 0x6fbf73,
        },
        Palette {
            surface: 0xf1f4ee,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x1f2a20,
            accent: 0x3d8a45,
        },
    ),
    (
        "ember",
        "Ember",
        Palette {
            surface: 0x120e0c,
            panel: 0x0a0706,
            solid: 0x1c1613,
            content: 0xecdfd6,
            accent: 0xf08a4b,
        },
        Palette {
            surface: 0xf7f2ee,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x2d211b,
            accent: 0xc75c1e,
        },
    ),
    (
        "orchid",
        "Orchid",
        Palette {
            surface: 0x110d16,
            panel: 0x09070c,
            solid: 0x1a1522,
            content: 0xe6def0,
            accent: 0xb57cf0,
        },
        Palette {
            surface: 0xf4f0f8,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x261e33,
            accent: 0x8048c8,
        },
    ),
    (
        "tidepool",
        "Tidepool",
        Palette {
            surface: 0x0a1213,
            panel: 0x050a0b,
            solid: 0x111c1e,
            content: 0xd8e8e8,
            accent: 0x3fc1b5,
        },
        Palette {
            surface: 0xedf5f5,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x162a2b,
            accent: 0x138a80,
        },
    ),
    (
        "dune",
        "Dune",
        Palette {
            surface: 0x13110d,
            panel: 0x0b0a07,
            solid: 0x1d1a14,
            content: 0xebe3d2,
            accent: 0xd9b25c,
        },
        Palette {
            surface: 0xf6f1e7,
            panel: 0xfffdf8,
            solid: 0xfffdf8,
            content: 0x2e2819,
            accent: 0xa57d1e,
        },
    ),
    (
        "rosewood",
        "Rosewood",
        Palette {
            surface: 0x140e10,
            panel: 0x0b0809,
            solid: 0x1e1518,
            content: 0xefdfe3,
            accent: 0xe7798f,
        },
        Palette {
            surface: 0xf8f0f2,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x2f1f24,
            accent: 0xc0405c,
        },
    ),
    (
        "glacier",
        "Glacier",
        Palette {
            surface: 0x0e1114,
            panel: 0x080a0c,
            solid: 0x161a1e,
            content: 0xe1e7ec,
            accent: 0x7fb4d9,
        },
        Palette {
            surface: 0xf2f5f8,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x20262d,
            accent: 0x3a7aa8,
        },
    ),
    (
        "arcade",
        "Arcade",
        Palette {
            surface: 0x0f0b1a,
            panel: 0x08060f,
            solid: 0x181229,
            content: 0xece6ff,
            accent: 0xff5fd2,
        },
        Palette {
            surface: 0xf5f2fc,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x231a3a,
            accent: 0xc2189a,
        },
    ),
    (
        "ink",
        "Ink",
        Palette {
            surface: 0x000000,
            panel: 0x000000,
            solid: 0x0a0a0a,
            content: 0xf5f5f5,
            accent: 0x7aa7ff,
        },
        Palette {
            surface: 0xffffff,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x000000,
            accent: 0x0050d8,
        },
    ),
];
pub(crate) fn palette(id: &str, light: bool) -> Palette {
    let row = PALETTES.iter().find(|r| r.0 == id).unwrap_or(&PALETTES[0]);
    if light { row.3 } else { row.2 }
}
