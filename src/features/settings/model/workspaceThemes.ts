/** Colour themes for the inset workspace. Each one paints the shell around the
 * card, the card itself, text, accent and the markdown link/heading colours,
 * with its own dark and light palette. "Default" sets nothing, so the
 * stylesheet's neutral greys and the hue/saturation tint stay in charge. */

export type WorkspaceThemeId =
  | "default"
  | "harbor"
  | "moss"
  | "ember"
  | "orchid"
  | "tidepool"
  | "dune"
  | "rosewood"
  | "glacier"
  | "arcade"
  | "ink";

export type WorkspacePalette = {
  /** The shell behind the sidebar, tabs and footer. Also the native glass tint. */
  surface: string;
  /** The card, as painted over the window glass (dark mode only uses glass). */
  panel: string;
  /** The card when the window is opaque. */
  panelSolid: string;
  content: string;
  accent: string;
  link: string;
  heading: string;
  /** Code colours, read through the `--syntax-*` variables. */
  syntax: SyntaxPalette;
};

export type SyntaxPalette = Record<(typeof SYNTAX_KEYS)[number], string>;

const SYNTAX_KEYS = [
  "keyword",
  "callable",
  "string",
  "type",
  "tag",
  "number",
  "comment",
  "property",
  "meta",
  "invalid",
] as const;

export type WorkspaceTheme = {
  id: WorkspaceThemeId;
  label: string;
  dark: WorkspacePalette;
  light: WorkspacePalette;
};

export const WORKSPACE_THEME_DEFAULT: WorkspaceThemeId = "default";

// The default palette mirrors the stylesheet's own values (its code colours are
// VS Code's Dark+ and Light+); it is only read for previews and the native
// tint, never written as variables.
export const WORKSPACE_THEMES: readonly WorkspaceTheme[] = [
  {
    id: "default",
    label: "Default",
    dark: {
      surface: "#0d0d0d",
      panel: "#060606",
      panelSolid: "#141414",
      content: "#ebebeb",
      accent: "#4da3f5",
      link: "#7dd3fc",
      heading: "#f9a8c9",
      syntax: {
        keyword: "#569cd6",
        callable: "#dcdcaa",
        string: "#ce9178",
        type: "#4ec9b0",
        tag: "#569cd6",
        number: "#b5cea8",
        comment: "#6a9955",
        property: "#9cdcfe",
        meta: "#dcdcaa",
        invalid: "#f44747",
      },
    },
    light: {
      surface: "#f5f5f5",
      panel: "#ffffff",
      panelSolid: "#ffffff",
      content: "#2e2e2e",
      accent: "#4da3f5",
      link: "#0866c6",
      heading: "#be185d",
      syntax: {
        keyword: "#0000ff",
        callable: "#795e26",
        string: "#a31515",
        type: "#267f99",
        tag: "#800000",
        number: "#098658",
        comment: "#008000",
        property: "#001080",
        meta: "#795e26",
        invalid: "#cd3131",
      },
    },
  },
  {
    id: "harbor",
    label: "Harbor",
    dark: {
      surface: "#0b1018",
      panel: "#060a10",
      panelSolid: "#121a26",
      content: "#dbe4f0",
      accent: "#5b9cf2",
      link: "#8cc4ff",
      heading: "#a3b6ff",
      syntax: {
        keyword: "#bb9af7",
        callable: "#7aa2f7",
        string: "#9ece6a",
        type: "#2ac3de",
        tag: "#f7768e",
        number: "#ff9e64",
        comment: "#6b7599",
        property: "#73daca",
        meta: "#89ddff",
        invalid: "#ff7a93",
      },
    },
    light: {
      surface: "#eef2f7",
      panel: "#ffffff",
      panelSolid: "#ffffff",
      content: "#1b2533",
      accent: "#2f6fd0",
      link: "#1f5fbf",
      heading: "#3b4fb8",
      syntax: {
        keyword: "#7847bd",
        callable: "#2e7de9",
        string: "#587539",
        type: "#007197",
        tag: "#c64343",
        number: "#b15c00",
        comment: "#6c7594",
        property: "#387068",
        meta: "#006a83",
        invalid: "#c64343",
      },
    },
  },
  {
    id: "moss",
    label: "Moss",
    dark: {
      surface: "#0d110e",
      panel: "#070a08",
      panelSolid: "#151b17",
      content: "#dde5dc",
      accent: "#6fbf73",
      link: "#9ad6a0",
      heading: "#c3d98a",
      syntax: {
        keyword: "#e67e80",
        callable: "#a7c080",
        string: "#dbbc7f",
        type: "#83c092",
        tag: "#e69875",
        number: "#d699b6",
        comment: "#859289",
        property: "#7fbbb3",
        meta: "#9da9a0",
        invalid: "#f85552",
      },
    },
    light: {
      surface: "#f1f4ee",
      panel: "#ffffff",
      panelSolid: "#ffffff",
      content: "#1f2a20",
      accent: "#3d8a45",
      link: "#2e7a3a",
      heading: "#5d7a1c",
      syntax: {
        keyword: "#e0444a",
        callable: "#5f7a00",
        string: "#9a7400",
        type: "#2b8a65",
        tag: "#c4621a",
        number: "#a3529b",
        comment: "#7a8778",
        property: "#2f7fae",
        meta: "#6e7a70",
        invalid: "#d0353b",
      },
    },
  },
  {
    id: "ember",
    label: "Ember",
    dark: {
      surface: "#120e0c",
      panel: "#0a0706",
      panelSolid: "#1c1613",
      content: "#ecdfd6",
      accent: "#f08a4b",
      link: "#f6b07c",
      heading: "#f3a37a",
      syntax: {
        keyword: "#fe8019",
        callable: "#8ec07c",
        string: "#b8bb26",
        type: "#fabd2f",
        tag: "#fb4934",
        number: "#d3869b",
        comment: "#928374",
        property: "#83a598",
        meta: "#a89984",
        invalid: "#ff6b5b",
      },
    },
    light: {
      surface: "#f7f2ee",
      panel: "#ffffff",
      panelSolid: "#ffffff",
      content: "#2d211b",
      accent: "#c75c1e",
      link: "#b4501a",
      heading: "#a8461f",
      syntax: {
        keyword: "#af3a03",
        callable: "#427b58",
        string: "#79740e",
        type: "#b57614",
        tag: "#9d0006",
        number: "#8f3f71",
        comment: "#7c6f64",
        property: "#076678",
        meta: "#665c54",
        invalid: "#cc241d",
      },
    },
  },
  {
    id: "orchid",
    label: "Orchid",
    dark: {
      surface: "#110d16",
      panel: "#09070c",
      panelSolid: "#1a1522",
      content: "#e6def0",
      accent: "#b57cf0",
      link: "#cfa6ff",
      heading: "#f2a2d6",
      syntax: {
        keyword: "#cba6f7",
        callable: "#89b4fa",
        string: "#a6e3a1",
        type: "#f9e2af",
        tag: "#f38ba8",
        number: "#fab387",
        comment: "#7f849c",
        property: "#b4befe",
        meta: "#94e2d5",
        invalid: "#ff8fab",
      },
    },
    light: {
      surface: "#f4f0f8",
      panel: "#ffffff",
      panelSolid: "#ffffff",
      content: "#261e33",
      accent: "#8048c8",
      link: "#6f3bb8",
      heading: "#a83a85",
      syntax: {
        keyword: "#8839ef",
        callable: "#1e66f5",
        string: "#2f7d1e",
        type: "#b06a00",
        tag: "#d20f39",
        number: "#c94f05",
        comment: "#7c7f93",
        property: "#5a65d6",
        meta: "#137a85",
        invalid: "#d20f39",
      },
    },
  },
  {
    id: "tidepool",
    label: "Tidepool",
    dark: {
      surface: "#0a1213",
      panel: "#050a0b",
      panelSolid: "#111c1e",
      content: "#d8e8e8",
      accent: "#3fc1b5",
      link: "#7fdcd3",
      heading: "#8ad0e8",
      syntax: {
        keyword: "#f28f6b",
        callable: "#5fd4e0",
        string: "#a8e6a1",
        type: "#ffd580",
        tag: "#7fdcd3",
        number: "#c4a7ff",
        comment: "#6f8d8f",
        property: "#8ad0e8",
        meta: "#8aa3a4",
        invalid: "#ff7a7a",
      },
    },
    light: {
      surface: "#edf5f5",
      panel: "#ffffff",
      panelSolid: "#ffffff",
      content: "#162a2b",
      accent: "#138a80",
      link: "#0f7a71",
      heading: "#1d6f8f",
      syntax: {
        keyword: "#c2502b",
        callable: "#0b7a8a",
        string: "#3c7d2a",
        type: "#8a6500",
        tag: "#0f7a71",
        number: "#6b4fc4",
        comment: "#6b8285",
        property: "#1d6f8f",
        meta: "#5b7072",
        invalid: "#c82828",
      },
    },
  },
  {
    id: "dune",
    label: "Dune",
    dark: {
      surface: "#13110d",
      panel: "#0b0a07",
      panelSolid: "#1d1a14",
      content: "#ebe3d2",
      accent: "#d9b25c",
      link: "#e8cc8a",
      heading: "#e5a86c",
      syntax: {
        keyword: "#957fb8",
        callable: "#7e9cd8",
        string: "#98bb6c",
        type: "#7aa89f",
        tag: "#e46876",
        number: "#d27e99",
        comment: "#8a8170",
        property: "#e6c384",
        meta: "#a6a69c",
        invalid: "#ff5d62",
      },
    },
    light: {
      surface: "#f6f1e7",
      panel: "#fffdf8",
      panelSolid: "#fffdf8",
      content: "#2e2819",
      accent: "#a57d1e",
      link: "#8c6914",
      heading: "#9c5a1c",
      syntax: {
        keyword: "#624c83",
        callable: "#4d699b",
        string: "#6f894e",
        type: "#597b75",
        tag: "#c84053",
        number: "#b35b79",
        comment: "#7d7465",
        property: "#8c6914",
        meta: "#716e61",
        invalid: "#e82424",
      },
    },
  },
  {
    id: "rosewood",
    label: "Rosewood",
    dark: {
      surface: "#140e10",
      panel: "#0b0809",
      panelSolid: "#1e1518",
      content: "#efdfe3",
      accent: "#e7798f",
      link: "#f2a6b4",
      heading: "#f0b38a",
      syntax: {
        keyword: "#eb6f92",
        callable: "#ebbcba",
        string: "#f6c177",
        type: "#9ccfd8",
        tag: "#e7798f",
        number: "#ea9a97",
        comment: "#7d7891",
        property: "#c4a7e7",
        meta: "#908caa",
        invalid: "#ff5c7a",
      },
    },
    light: {
      surface: "#f8f0f2",
      panel: "#ffffff",
      panelSolid: "#ffffff",
      content: "#2f1f24",
      accent: "#c0405c",
      link: "#a8344f",
      heading: "#a3502a",
      syntax: {
        keyword: "#b4637a",
        callable: "#b0605c",
        string: "#a86a0f",
        type: "#286983",
        tag: "#c0405c",
        number: "#9a4d2a",
        comment: "#797593",
        property: "#7a5f9b",
        meta: "#6e6a86",
        invalid: "#b4233c",
      },
    },
  },
  {
    id: "glacier",
    label: "Glacier",
    dark: {
      surface: "#0e1114",
      panel: "#080a0c",
      panelSolid: "#161a1e",
      content: "#e1e7ec",
      accent: "#7fb4d9",
      link: "#a7d0ec",
      heading: "#b9c8e8",
      syntax: {
        keyword: "#81a1c1",
        callable: "#88c0d0",
        string: "#a3be8c",
        type: "#8fbcbb",
        tag: "#d08770",
        number: "#b48ead",
        comment: "#7b88a1",
        property: "#a7c2dc",
        meta: "#7f97b8",
        invalid: "#bf616a",
      },
    },
    light: {
      surface: "#f2f5f8",
      panel: "#ffffff",
      panelSolid: "#ffffff",
      content: "#20262d",
      accent: "#3a7aa8",
      link: "#2d6a96",
      heading: "#44598f",
      syntax: {
        keyword: "#3a6d9f",
        callable: "#2b7a8c",
        string: "#4f7a34",
        type: "#2a7a76",
        tag: "#b25a2e",
        number: "#7b4f8a",
        comment: "#6b7689",
        property: "#44598f",
        meta: "#5a6a80",
        invalid: "#b0353f",
      },
    },
  },
  {
    id: "arcade",
    label: "Arcade",
    dark: {
      surface: "#0f0b1a",
      panel: "#08060f",
      panelSolid: "#181229",
      content: "#ece6ff",
      accent: "#ff5fd2",
      link: "#5ff2ff",
      heading: "#ffd35f",
      syntax: {
        keyword: "#ff5fd2",
        callable: "#5ff2ff",
        string: "#ffd35f",
        type: "#b38cff",
        tag: "#ff8f5f",
        number: "#f97e72",
        comment: "#7d71a8",
        property: "#72f1b8",
        meta: "#8d86c9",
        invalid: "#ff4f6d",
      },
    },
    light: {
      surface: "#f5f2fc",
      panel: "#ffffff",
      panelSolid: "#ffffff",
      content: "#231a3a",
      accent: "#c2189a",
      link: "#0a7f90",
      heading: "#9a6608",
      syntax: {
        keyword: "#c2189a",
        callable: "#0a7f90",
        string: "#8a5a00",
        type: "#6a3fd1",
        tag: "#c2410c",
        number: "#c2304a",
        comment: "#7a6f99",
        property: "#0f7a55",
        meta: "#6b6390",
        invalid: "#c81e3a",
      },
    },
  },
  {
    id: "ink",
    label: "Ink",
    dark: {
      surface: "#000000",
      panel: "#000000",
      panelSolid: "#0a0a0a",
      content: "#f5f5f5",
      accent: "#7aa7ff",
      link: "#9cc0ff",
      heading: "#ffffff",
      syntax: {
        keyword: "#7aa7ff",
        callable: "#ffffff",
        string: "#a8a8a8",
        type: "#d6d6d6",
        tag: "#7aa7ff",
        number: "#9cc0ff",
        comment: "#7a7a7a",
        property: "#d6d6d6",
        meta: "#8f8f8f",
        invalid: "#ff6b6b",
      },
    },
    light: {
      surface: "#ffffff",
      panel: "#ffffff",
      panelSolid: "#ffffff",
      content: "#000000",
      accent: "#0050d8",
      link: "#0040b0",
      heading: "#000000",
      syntax: {
        keyword: "#0050d8",
        callable: "#000000",
        string: "#5a5a5a",
        type: "#333333",
        tag: "#0050d8",
        number: "#0040b0",
        comment: "#767676",
        property: "#2a2a2a",
        meta: "#6a6a6a",
        invalid: "#c00000",
      },
    },
  },
];

const BY_ID = new Map(WORKSPACE_THEMES.map((theme) => [theme.id, theme]));

export function isWorkspaceThemeId(value: unknown): value is WorkspaceThemeId {
  return typeof value === "string" && BY_ID.has(value as WorkspaceThemeId);
}

export function workspaceTheme(id: WorkspaceThemeId): WorkspaceTheme {
  return BY_ID.get(id) ?? WORKSPACE_THEMES[0];
}

const PALETTE_KEYS = [
  "surface",
  "panel",
  "panelSolid",
  "content",
  "accent",
  "link",
  "heading",
] as const satisfies readonly (keyof WorkspacePalette)[];

function variableName(scheme: "dark" | "light", key: string) {
  const kebab = key.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`);
  return `--wt-${scheme}-${kebab}`;
}

/** The `--wt-*` variables the stylesheet reads under `html.layout-inset`, or
 * none for the default theme. */
export function workspaceThemeVariables(
  id: WorkspaceThemeId,
): Record<string, string> {
  if (id === WORKSPACE_THEME_DEFAULT) return {};
  const theme = workspaceTheme(id);
  const variables: Record<string, string> = {};
  for (const scheme of ["dark", "light"] as const) {
    for (const key of PALETTE_KEYS) {
      variables[variableName(scheme, key)] = theme[scheme][key];
    }
    for (const key of SYNTAX_KEYS) {
      variables[variableName(scheme, `syntax-${key}`)] =
        theme[scheme].syntax[key];
    }
  }
  return variables;
}

/** Every variable any theme can set, so switching themes clears the last one. */
export const WORKSPACE_THEME_VARIABLE_NAMES: readonly string[] = (
  ["dark", "light"] as const
).flatMap((scheme) => [
  ...PALETTE_KEYS.map((key) => variableName(scheme, key)),
  ...SYNTAX_KEYS.map((key) => variableName(scheme, `syntax-${key}`)),
]);
