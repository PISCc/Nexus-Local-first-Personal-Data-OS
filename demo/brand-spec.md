# Nexus demo brand spec

## Identity source

- Product: Nexus — local-first personal data OS.
- Existing wordmark reference: `apps/desktop/src/App.tsx` (`N / 01` + `Nexus`).
- Visual direction icon: `demo/assets/nexus-brand/nexus-product-icon.png` (user-provided reference, copied locally for offline preview).
- The existing functional search demo remains independent; the new `Nexus Visual Direction.html` is a deliberately small style-validation page.

## Visual direction

The current visual reference is **Organic Memory Field**: the supplied icon's cream/mint field, cobalt blue shapes, deep-teal shapes, and one coral signal become the product's visual grammar. The page is intentionally small: a rounded navigation surface, a short split Hero, a meaningful workflow preview, one search preview, and a visible `query → result → source` trail.

- Palette: `#F7FBF6` paper, `#0B2D35` ink, `#155FE8` primary blue, `#07584F` local teal, `#BFE8D9` soft teal, `#E86D4E` coral signal, and `#082F3B` dark surface.
- Surfaces: translucent white navigation and panels, 23–28px panel radius, 24px navigation radius, quiet hairlines, and low blue/teal shadows. Avoid a card wall.
- Typography: Poppins for headings and numeric emphasis, DM Sans for interface copy, and IBM Plex Mono for operational labels, paths, and index status, with local system fallbacks for Chinese.
- Structure: rounded top navigation → short Hero + workflow preview → one search surface → source trail → dark close. The visual field only appears where it explains the product's information relationship.
- Icons and imagery: use the copied user-provided icon asset and inline SVG interface icons; no emoji, stock imagery, user files, or fabricated product screenshots.
- Motion: one small Hero entrance and minimal state feedback; no perpetual background animation. Reduced-motion mode renders the final state.

The full implementation rules are in `design-system/nexus/MASTER.md`; the page-specific constraints are in `design-system/nexus/pages/showcase.md`.

## Asset boundary

The pages are self-contained demos and use no user files, network data, fabricated customer logos, or fake product screenshots. The visual direction page may load optional Google Fonts and otherwise falls back to local system fonts.
