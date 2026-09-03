# Nexus Product Style System

> This file is the visual source of truth for Nexus. Read it before designing or modifying any page. A page-specific override may narrow these rules, but must not introduce a conflicting visual language without an explicit product decision.

**Project:** Nexus  
**Version:** 1.1  
**Status:** Final / approved product baseline  
**Adopted:** 2026-09-03  
**Finalized:** 2026-09-03  
**Canonical production implementation:** `apps/desktop/src/index.css`, `apps/desktop/src/App.tsx`, `apps/desktop/src/SearchView.tsx`  
**Visual-direction reference:** `demo/Nexus Visual Direction.html`  
**Canonical product icon:** `apps/desktop/public/nexus-product-icon.png`  
**Source design asset:** `demo/assets/nexus-brand/nexus-product-icon.png`

## 0. Final baseline decision

The current Nexus desktop interface is the approved final visual and
interaction baseline. Future product work starts from this implementation; the
showcase is supporting design evidence, not an invitation to create a parallel
skin.

“Final” applies to the Organic Memory Field visual language, semantic color
roles, typography roles, search-first hierarchy, and visible
`query → result → source` relationship. It does not freeze real product
functionality. Responsive and accessibility improvements, necessary state
feedback, and bug fixes may continue when they preserve this system.

Visual-only work must not alter Tauri commands, Rust/TypeScript data structures,
search or indexing behavior, routes, form fields, test selectors, or error
handling. Replacing the visual language, palette, typography roles, core
information hierarchy, or Nexus product mark requires an explicit product
decision and a coordinated specification update.

## 1. Product and design position

Nexus is a local-first Personal Data OS. The interface should feel like a calm, trustworthy memory field: human enough to invite exploration, structured enough to support serious search, and explicit enough to show where every result came from.

The visual direction is **Organic Memory Field**:

- organic, fluid shapes from the product icon;
- a warm paper/mint environment rather than a cold grey dashboard;
- cobalt blue for search and primary action;
- deep teal for local ownership and provenance;
- coral only for a meaningful signal, focus, or result node;
- operational labels in mono to keep the product precise.

The product is a tool, not a brand poster. Visual character must clarify the local search model, not compete with it.

### Design dials

| Dial | Value | Consequence |
| --- | ---: | --- |
| Visual variance | 6/10 | Use one organic focal visual and varied section rhythm; keep the navigation and data paths stable. |
| Motion | 3/10 | Short state feedback and one restrained entrance; no ambient loops or decorative parallax. |
| Information density | 4/10 | One dominant idea per viewport; compact metadata only where it improves scanning. |
| Asset dependence | 6/10 | Use the real Nexus icon as the brand anchor; interface structure must still work without remote assets. |
| Brand fidelity | 8/10 | Preserve the icon, `N / 01` + `Nexus` wordmark, local-first language, and source traceability. |

## 2. Non-negotiable product principles

1. **Local first is visible.** Show local ownership in the interface state or supporting copy; never imply cloud processing.
2. **Search before AI.** Deterministic search remains the primary interaction and must work without an LLM.
3. **Every result has a trail.** A result, excerpt, file path, or future answer should be able to lead back to its source.
4. **No invented trust.** Do not add fake customer logos, testimonials, pricing, usage numbers, or performance claims.
5. **Content beats ornament.** Remove any visual element that does not improve hierarchy, comprehension, or feedback.

## 3. Color tokens

Use semantic tokens rather than raw hex values inside components.

| Role | Token | Value | Use |
| --- | --- | --- | --- |
| Paper background | `--color-paper` | `#F7FBF6` | Global light background. |
| Paper deep | `--color-paper-deep` | `#EAF6EE` | Soft section tint and organic surfaces. |
| Ink | `--color-ink` | `#0B2D35` | Headings, primary text, high-emphasis metadata. |
| Muted ink | `--color-muted` | `#4D6468` | Supporting copy and secondary metadata. |
| Line | `--color-line` | `rgba(11, 45, 53, .13)` | Hairlines and quiet separators. |
| Surface | `--color-surface` | `rgba(255, 255, 255, .86)` | Panels and navigation glass. |
| Surface solid | `--color-surface-solid` | `#FFFFFF` | Form fields and high-clarity controls. |
| Primary blue | `--color-blue` | `#155FE8` | Search action, links, query nodes, primary CTA. |
| Blue deep | `--color-blue-deep` | `#0F48BE` | Hover/pressed blue state. |
| Local teal | `--color-teal` | `#07584F` | Local-only state, provenance, dark surfaces, secondary CTA. |
| Teal soft | `--color-teal-soft` | `#BFE8D9` | Selected chips, soft fills, light signal rings. |
| Signal coral | `--color-coral` | `#E86D4E` | Result node, focus signal, meaningful attention state. Never use for long body copy. |
| Dark surface | `--color-dark-surface` | `#082F3B` | Trust/provenance closure and contrast sections. |
| Dark text | `--color-dark-text` | `#F6FFF8` | Text on dark surface. |

### Color rules

- Keep the working palette to paper, ink, blue, teal, and coral. Do not add a new hue to solve a local styling problem.
- Blue and teal are structural; coral is sparse and semantic.
- Do not use a purple-pink-blue “AI gradient”. If a gradient is useful, it may move between blue and teal with low saturation and a clear information purpose.
- Body text must meet at least 4.5:1 contrast. Use `--color-muted` only on the paper/surface backgrounds where it remains readable.
- Never communicate status by color alone. Pair color with a label, icon, text, or position.

## 4. Typography

| Role | Font | Fallback | Rules |
| --- | --- | --- | --- |
| Display / Hero | Poppins | `Segoe UI Variable Display`, `Microsoft YaHei UI`, sans-serif | Large, compact, confident; use for Hero headings and key numbers. |
| Interface / body | DM Sans | `Segoe UI Variable`, `Microsoft YaHei UI`, sans-serif | Use for navigation, descriptions, labels, and controls. Base body size is 16px. |
| Operational / path | IBM Plex Mono | `Cascadia Code`, Consolas, monospace | Use for paths, extensions, query syntax, index state, IDs, and small system labels. |

Chinese copy uses the local fallback naturally. Do not rely on a remote font for core comprehension. Avoid Inter, Roboto, and generic system UI as the display choice unless an explicit accessibility or platform constraint requires it.

### Type scale

- Hero: `clamp(3.25rem, 7vw, 6.8rem)`, line-height `.94`, letter-spacing `-.08em` to `-.1em`.
- Section heading: `clamp(2.25rem, 5vw, 4.2rem)`, line-height `.98`.
- Panel heading: 24–48px depending on hierarchy.
- Body: 16–18px, line-height 1.6–1.75.
- Metadata: 10–12px mono, uppercase only for operational labels.
- Never set normal body copy below 14px. Small mono labels may be 9–10px when they are supplementary, never the only way to understand an action.

## 5. Spacing and layout

Use an 8px rhythm with a small 4px subdivision:

| Token | Value |
| --- | ---: |
| `--space-1` | 4px |
| `--space-2` | 8px |
| `--space-3` | 12px |
| `--space-4` | 16px |
| `--space-5` | 24px |
| `--space-6` | 32px |
| `--space-7` | 48px |
| `--space-8` | 64px |
| `--space-9` | 96px |

- Desktop content width: `min(1180px, calc(100% - 48px))`.
- Mobile content width: `calc(100% - 28px)`.
- Keep a stable grid spine: navigation, main content, and closing footer align to the same container.
- Prefer one strong split layout in the Hero: meaning on the left, a meaningful workflow preview plus its source relationship on the right.
- Below the Hero, use one search surface and one source/provenance surface. Do not turn every sentence into a card.
- Use `scroll-padding-top` when a sticky navigation is present so anchored sections are not obscured.

### Canonical page rhythm

`rounded navigation → split Hero → workflow preview → source trail → dark trust close`

For the real product shell, adapt the rhythm to the task while preserving the same hierarchy: search is the main action, local status is visible, and source context is nearby.

## 6. Surfaces, radius, and depth

- Navigation: 24px radius, translucent white, `backdrop-filter: blur(18px)` only when a solid fallback exists.
- Panels: 23–28px radius; hero visual may use 36px on desktop and 28px on mobile.
- Controls: 16–19px radius for fields/buttons; use full pills only for compact tags, nav CTA, and filters.
- Icon frame: let the supplied icon preserve its rounded-square silhouette; do not crop or recolor it.
- Borders: 1px quiet blue/teal-tinted hairlines. Avoid heavy outlines as decoration.
- Main soft shadow: `0 22px 64px rgba(10, 63, 58, .12)`.
- Panel shadow: `0 10px 36px rgba(11, 45, 53, .05)`.
- Button shadow: `0 12px 24px rgba(21, 95, 232, .20)` only for the primary action.
- Avoid stacked shadows, hard black drop shadows, and neumorphic depressions.

## 7. Icon and asset discipline

- The canonical product icon is the user-provided asset at `demo/assets/nexus-brand/nexus-product-icon.png`. Use it as an `<img>` or equivalent real asset; do not redraw it with CSS or a new SVG.
- Use inline SVG for interface icons, with one consistent stroke language. Every icon-only control must have an accessible name.
- Never use emoji as interface icons.
- Do not use stock imagery, fabricated product screenshots, or CSS silhouettes in branded surfaces.
- The icon is a brand anchor, not a product screenshot. Use it in the wordmark/nav; the Hero's main visual should demonstrate a real Nexus workflow such as search, indexing, or source traceability.
- Keep the icon's blue, deep-teal, mint, cream, and coral relationships intact. Do not apply arbitrary filters.
- If the icon is shown outside the wordmark, explain its role briefly; never make users infer product behavior from an abstract mark.
- Product visuals should use real concepts and labels: query/search, result/match, source/path, index/local-only. Do not draw decorative connector lines whose endpoints or meaning are unclear.

## 8. Components and states

### Navigation

- Keep the top-level navigation short: one product/context link and one primary action are enough for a showcase.
- In the app shell, navigation labels must match the existing product journey and preserve deep links.
- Sticky navigation may float above the paper background, but must not hide focused content.

### Buttons and links

- Minimum interactive height: 44px; use 48–52px for primary actions.
- Primary: blue fill with white text; hover uses blue-deep and a small upward transform (max 2px).
- Secondary: transparent/white surface with ink text and a quiet line; hover increases surface clarity rather than scale.
- Every clickable element has `cursor: pointer`, hover, active, disabled, and visible `:focus-visible` states.
- Do not make the only action available on hover.

### Search

- Search is a labeled form, not placeholder-only UI.
- The field may use a mono prefix such as `query /`, but the field's natural-language label remains available to assistive technology.
- Submit feedback is visible in or beside the search surface and uses `aria-live` when it changes.
- Example query chips are native buttons with `aria-pressed`; selected state must not rely on color alone.
- No-result state must suggest a next action or query; never show a blank panel.

### Results and provenance

- A result row exposes title, source path, and type/extension in a stable order.
- Provenance is a visible trail: `query → result → source` or an equivalent semantic path.
- The source path is selectable/copyable where the real product supports it; do not fake an “open source” action in a static showcase.

### Local state

- “Local only”, “index ready”, “syncing”, “degraded”, and “needs attention” must have text labels plus a non-color signal.
- Loading and degraded states must be understandable in the browser preview where Tauri is unavailable.
- Recoverable filesystem failures should be presented as actionable messages, not uncaught errors.

## 9. Motion and performance

- Motion intensity is intentionally low. Use CSS transitions for controls: 150–220ms, ease-out.
- One Hero entrance may use opacity + `translateY(8–12px)` and a 500–700ms duration.
- One restrained signal pulse is allowed for an active index state; do not animate the whole background.
- Animate `transform` and `opacity`, not layout dimensions.
- `prefers-reduced-motion: reduce` must remove non-essential entrance, pulse, and smooth scrolling while preserving the final layout.
- Prefer local assets and CSS/SVG. Do not add a dependency for a single visual effect.
- Reserve image space to avoid layout shift. Use `loading="lazy"` for below-fold assets when applicable.

## 10. Accessibility and responsive acceptance

- Keyboard order follows visual order; all interactive controls are reachable without a pointer.
- Focus rings are visible on every link, button, input, and dialog control.
- Touch targets are at least 44×44px with at least 8px separation where practical.
- Use semantic headings, `nav`, `main`, `section`, `form`, `ol/ul`, and labels.
- Test at 375px, 768px, 1024px, and 1440px. There must be no horizontal overflow or content hidden behind the sticky navigation.
- Check Hero heading line breaks in Chinese at mobile widths; shorten copy before shrinking type.

## 11. Copy and content voice

- Chinese first, short, direct, and calm.
- Use English only for compact operational labels or the established product terms: `Nexus`, `query`, `result`, `source`, `local-first`.
- Prefer “找回 / 来源 / 本地 / 原文 / 索引 / 搜索” over vague AI marketing language.
- Do not say “understand everything”, “works magically”, “secure” without a concrete mechanism, or “zero latency” without measured evidence.
- Static demos must label sample states as demo/sample. Do not present fabricated counts as production telemetry.

## 12. Forbidden drift

- Purple-pink-blue AI gradients.
- Dark cyber-neon or terminal cosplay as the default product skin.
- Emoji used as icons.
- Generic Inter/Roboto dashboard defaults.
- Excessive glassmorphism, blur, or floating cards everywhere.
- Repeated organic blobs that do not map to a real information relationship.
- Hard shadows, random border-radius values, and arbitrary new accent colors.
- Hidden labels, hover-only actions, missing empty/error states, or inaccessible icon buttons.
- Cloud upload, account, pricing, customer logo, or “AI assistant” claims unless the product actually implements and documents them.

## 13. Change protocol for future work

Before editing a page:

1. Read this file and the relevant page override.
2. Identify which product contract is being preserved: route, navigation, search, source path, state, or form behavior.
3. Reuse the tokens and component rules above; do not create a parallel palette.
4. If a new pattern is needed, document its information value, dependency cost, and motion cost.
5. Run the relevant frontend checks and inspect the result at desktop and mobile widths.

The approved baseline may only be replaced after an explicit human product
decision. When that happens, update this file's version, the project brief,
relevant page overrides, canonical references, and production implementation
together. Do not silently fork the style system.
