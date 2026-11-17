# MonoCode's design language

Use this when a draft shows MonoCode itself, or anything that should sit in it
without looking foreign. A draft already receives the real theme through CSS
variables, so prefer the variables to the hex values below; the numbers are what
the variables resolve to and the rules behind them. To show the app in one
theme whatever the person's is, or both side by side, put data-theme="light" or
"dark" on the mock's window box: the variables inside it are that theme's.

## Surfaces

The window is dark frosted glass or solid light, chosen by the user. A draft
paints the content card's solid color as --background. Floating things (menus,
popovers, dialogs) are --popover with a 1px --border, 12px corners and a two
layer shadow: 0 20px 25px -5px and 0 8px 10px -6px, both black at 10%.

## Ink, not grey

Nothing is a fixed grey. Text and every fill are the theme's ink at an alpha, so
they follow dark, light and custom themes.

| Role | Ink alpha | Variable |
| --- | --- | --- |
| Primary text | 100% | --foreground |
| Button text | 80% | |
| Muted text, labels | 60% | --muted-foreground |
| Captions, hints | 45–55% | |
| Pane separator hairline | 4% | |
| Card border | 8–10% | --border |
| Button and input border | 12% | --input |
| Card fill | 3% | --card |
| Hover, chip, secondary fill | 6% | --muted, --secondary |
| Selected row or tab | 10% | |

The accent (default #4da3f5) is --accent. Its tinted fill is --accent-surface
(16%), text on that fill --accent-surface-foreground. Focus is a 1px accent
border, not a glow. Status colors: --success #10b981, --warning #f59e0b,
--destructive #f43f5e, --info #38bdf8, each with a -surface tint at 16%.

## Shape and type

Corners: 6px for controls and tabs (--radius-sm), 8px (--radius), 12px for
cards, popovers and the composer (--radius-lg). Cards are borders and a 3% fill,
never shadows. Type is the system UI font: 13px body (--font-size), 12px for
controls and labels, 11px for captions, 15px for a section title at weight 600.
Numbers that change use tabular figures. Weights are 400 and 500; 600 is for
titles and big numbers only.

## Sizes

Title bar 40px. Pane header 36px, with a 1px bottom hairline. Tab plate 27px,
6px corners, 4% fill that goes to 7% on hover and 10% when selected. Controls
28px (24px small). Icon buttons 20px with a 14px icon at 55% ink that rises to
100% on hover. Right panel 220–640px wide. The conversation column is at most
832px wide and centered. Icons are the hugeicons stroke set at 1.5–1.75px
(mc.icon has the common ones).

## The shell

Left: a project rail and the sidebar of conversations, each row 28px with a
6px status dot (green running, amber waiting). Top: session tabs on the title
bar. Center: the conversation, with the user's messages as right-aligned
bubbles at 8% ink and the agent's as plain text. Bottom: the composer, a 12px
rounded card with the input above a row of small controls (model, effort)
and a primary Send button. Right: the project panel with Explorer, History and
Terminal tabs.

## With the kit

The mc kit already follows all of this: mc-card, mc-button (and primary,
ghost, small), mc-input, mc-tabs and mc-tab, mc-badge, mc-menu, mc-kbd,
mc-stat. Reach for them first, then spacing of 8, 12 and 16.
