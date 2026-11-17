# Design tokens: taking a look from an image or from code

Read this when a draft has to look like something that already exists: a
screenshot or mock the person gave you, a site or app they name, or the
codebase you are in. Pull the tokens out first, write them down once, and build
the page from them. Never match by eye and never invent a value you could have
read.

## What counts as a token

- Color: page background, surface (card, popover), border, text (primary,
  muted), accent, and the status colors. Light and dark separately.
- Type: family, the size scale, weights, line height, letter spacing.
- Space: the base unit and the scale built on it (4, 8, 12, 16 ...).
- Shape: corner radii, border widths, shadows.
- Motion: durations and easings, if the draft animates.
- Sizes of the recurring parts: control height, row height, icon size.

## From code (exact, so try it first)

Look in this order and stop at the first source of truth:
1. Theme files: tokens.json, theme.ts, design-tokens, Style Dictionary output,
   a Rust or Swift theme module, Android colors.xml or Theme.kt.
2. CSS custom properties: :root and [data-theme], globals.css, app.css.
3. Tailwind: tailwind.config.*, or @theme in a Tailwind 4 stylesheet; shadcn's
   components.json and its CSS variables.
4. The components themselves, for what the tokens do not name (a button's
   height, a card's padding).

Use grep for "--" and color literals rather than reading whole files. Resolve
every alias to its final value (a var() that points at another var(), an hsl
triplet that a wrapper turns into a color) and take both themes. Keep the
project's own names: --surface-raised is better than "grey 2".

## From an image (measured, not guessed)

A picture does not tell you its hex values; measure them.
1. Look at it first with Read, to find the parts: background, cards, text,
   buttons, borders.
2. Sample real pixels with a short script (Python with PIL, or sips or
   ImageMagick). Read the pixel at the middle of a flat area for each fill. For
   a palette, quantize to 8 to 12 colors and name each by the area it covers.
   Text color is the darkest or lightest pixel run inside a glyph, not the
   average. Borders are 1px lines: sample on the line, not beside it.
3. Measure spacing and radii with a pixel scan across an edge (where the color
   changes), not by eye. A Retina screenshot is 2x: halve pixel distances to get
   points. Take the base unit from the gaps that repeat.
4. Fonts: name the closest family you can see (a geometric sans, a grotesque, a
   mono) and pick the nearest one the page can load (see Limits in the draft
   guide) or the system stack. Say which you chose and that it is a match, not
   the original.
5. Check contrast of text on its fill. If the source fails, keep its look and
   say so.

Mark every value you estimated. A measured value is exact; "about 12px" is not.

## Writing them into the draft

Put all of it in one block at the top of the page's style and use only var()
below it:

    :root { --bg:#0e1116; --surface:#161b22; --border:#ffffff1a;
            --text:#e6edf3; --muted:#8b949e; --accent:#4da3f5;
            --radius:10px; --space:8px; --font:"Inter",system-ui,sans-serif; }

The app already supplies --background, --foreground, --border, --radius and the
rest for MonoCode itself. When the subject is something else, define your own
names in the mock's box and use those, and put data-theme="light" (or "dark",
whichever the source is) on that box, so nothing leaks in from the app: text
you leave uncolored then stays that theme's ink when the app's theme changes. The
outermost element still stays without a background; the box inside paints the
tokens. If the person is deciding between values, bind the tokens to controls
(a color input, a slider for radius or space) and keep them in mc.state, so you
can read what they chose.

## Checking and reporting

Take a draft_preview screenshot and put it next to the source: same colors on
the same parts, same rhythm of spacing, same corner radius. Fix the biggest
difference first.

When you are done, say in a few lines what you took and where it came from: the
file and line of a token, or the region of the image you sampled, and which
values are estimates. If the person asked for the tokens themselves, make the
draft a token sheet: swatches with their hex and name, the type scale set in its
own sizes, the spacing and radii drawn to scale, each value as selectable text.
