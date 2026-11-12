//! Frozen legacy order: changing it changes hashed defaults and Mono allocation.
pub const MASCOTS: [&str; 10] = [
    "invader", "ghost", "robot", "cat", "skull", "crab", "mushroom", "rocket", "dino", "frog",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MascotId {
    Invader,
    Ghost,
    Robot,
    Cat,
    Skull,
    Crab,
    Mushroom,
    Rocket,
    Dino,
    Frog,
}

impl MascotId {
    pub fn find(name: &str) -> Option<Self> {
        CATALOG.iter().find(|m| m.name() == name).map(|m| m.id)
    }
    pub const fn index(self) -> usize {
        self as usize
    }
    pub const fn mascot(self) -> &'static Mascot {
        &CATALOG[self.index()]
    }
    pub const fn name(self) -> &'static str {
        MASCOTS[self.index()]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub rows: [&'static str; 8],
    pub eyes: &'static [[u8; 2]],
}

#[derive(Clone, Copy, Debug)]
pub struct Mascot {
    pub id: MascotId,
    pub rest: Frame,
    pub talk: Frame,
}

impl Mascot {
    pub const fn name(&self) -> &'static str {
        self.id.name()
    }
    pub const fn frame(&self, talk: bool) -> &Frame {
        if talk {
            &self.talk
        } else {
            &self.rest
        }
    }
}

const EYES: &[[u8; 2]] = &[[2, 2], [5, 2]];
pub const CATALOG: [Mascot; 10] = [
    Mascot {
        id: MascotId::Invader,
        rest: Frame {
            rows: [
                "..#..#..", ".######.", "##.##.##", "########", ".######.", ".#.##.#.", "#.#..#.#",
                "........",
            ],
            eyes: EYES,
        },
        talk: Frame {
            rows: [
                "..#..#..", ".######.", "##.##.##", "########", ".######.", "#.####.#", ".#....#.",
                "#......#",
            ],
            eyes: EYES,
        },
    },
    Mascot {
        id: MascotId::Ghost,
        rest: Frame {
            rows: [
                "..####..", ".######.", "##.##.##", "########", "########", "########", "########",
                "#.##.##.",
            ],
            eyes: EYES,
        },
        talk: Frame {
            rows: [
                "..####..", ".######.", "#.##.###", "########", "########", "########", "########",
                ".##.##.#",
            ],
            eyes: &[[1, 2], [4, 2]],
        },
    },
    Mascot {
        id: MascotId::Robot,
        rest: Frame {
            rows: [
                "...#....", ".######.", ".#.##.#.", ".######.", ".#....#.", ".######.", "..#..#..",
                "........",
            ],
            eyes: EYES,
        },
        talk: Frame {
            rows: [
                "....#...", ".######.", ".#.##.#.", ".######.", ".##..##.", ".######.", ".#....#.",
                "........",
            ],
            eyes: EYES,
        },
    },
    Mascot {
        id: MascotId::Cat,
        rest: Frame {
            rows: [
                ".#....#.", ".##..##.", "########", "#.####.#", "########", "###..###", ".######.",
                "..#..#..",
            ],
            eyes: &[[1, 3], [6, 3]],
        },
        talk: Frame {
            rows: [
                ".#....#.", ".##..##.", "########", "#.####.#", "########", "########", ".######.",
                ".#....#.",
            ],
            eyes: &[[1, 3], [6, 3]],
        },
    },
    Mascot {
        id: MascotId::Skull,
        rest: Frame {
            rows: [
                ".######.", "########", "##.##.##", "########", ".##..##.", ".######.", ".#.##.#.",
                "........",
            ],
            eyes: EYES,
        },
        talk: Frame {
            rows: [
                ".######.", "########", "##.##.##", "########", ".##..##.", ".######.", ".#....#.",
                "..####..",
            ],
            eyes: EYES,
        },
    },
    Mascot {
        id: MascotId::Crab,
        rest: Frame {
            rows: [
                "#......#", ".#....#.", ".######.", "##.##.##", "########", "#.####.#", "#......#",
                "........",
            ],
            eyes: &[[2, 3], [5, 3]],
        },
        talk: Frame {
            rows: [
                "#......#", "##....##", ".######.", "##.##.##", "########", ".######.", "#.#..#.#",
                "........",
            ],
            eyes: &[[2, 3], [5, 3]],
        },
    },
    Mascot {
        id: MascotId::Mushroom,
        rest: Frame {
            rows: [
                "..####..", ".######.", "########", "##.##.##", "########", "...##...", "...##...",
                "..####..",
            ],
            eyes: &[[2, 3], [5, 3]],
        },
        talk: Frame {
            rows: [
                "........", "..####..", ".######.", "########", "##.##.##", "...##...", "...##...",
                "..####..",
            ],
            eyes: &[[2, 4], [5, 4]],
        },
    },
    Mascot {
        id: MascotId::Rocket,
        rest: Frame {
            rows: [
                "...##...", "..####..", "..#..#..", "..####..", ".######.", ".######.", "##....##",
                "..####..",
            ],
            eyes: &[[3, 2], [4, 2]],
        },
        talk: Frame {
            rows: [
                "...##...", "..####..", "..#..#..", "..####..", ".######.", ".######.", "##....##",
                "...##...",
            ],
            eyes: &[[3, 2], [4, 2]],
        },
    },
    Mascot {
        id: MascotId::Dino,
        rest: Frame {
            rows: [
                "...#####", "...##.##", "...#####", ".#######", "########", "#####...", ".##.##..",
                "..#..#..",
            ],
            eyes: &[[5, 1]],
        },
        talk: Frame {
            rows: [
                "...#####", "...##.##", "...#####", ".#######", "########", "#####...", "..##.##.",
                "..#...#.",
            ],
            eyes: &[[5, 1]],
        },
    },
    Mascot {
        id: MascotId::Frog,
        rest: Frame {
            rows: [
                "........", "##....##", "#.####.#", "########", "########", ".######.", "##....##",
                "........",
            ],
            eyes: &[[1, 2], [6, 2]],
        },
        talk: Frame {
            rows: [
                "##....##", "#.####.#", "########", "########", ".######.", "##....##", "#......#",
                "........",
            ],
            eyes: &[[1, 1], [6, 1]],
        },
    },
];
