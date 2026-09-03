# Nexus Showcase Page Override

This page follows the Nexus **Organic Memory Field** master system and remains
the canonical showcase for the visual direction. The approved production
baseline is the current desktop implementation identified in `../MASTER.md`;
this showcase must not be used to justify a parallel product skin.

## Scope

The page is a lightweight style-validation showcase, not a full product shell. Keep the content intentionally small:

`rounded navigation → short Hero → workflow preview → query/result/source trail → dark close`

Do not add pricing, signup, customer logos, feature-grid filler, cloud claims, or a full dashboard wall.

## Page-specific decisions

- Use the real icon from `demo/assets/nexus-brand/nexus-product-icon.png` in the wordmark/nav as the brand anchor. Use the Hero focal visual for a meaningful Nexus product workflow.
- Keep the Hero heading short: one Chinese idea with a blue second line is preferred.
- The Hero visual must show a recognizable Nexus action: a search query, matching documents, and a highlighted original source path.
- Prefer direct UI structure and explicit labels over abstract symbols or connector lines whose endpoints and meaning are unclear.
- The search preview may use demo content, but its status must say `demo`, `sample`, or equivalent when it is not connected to Tauri.
- Keep the search panel light and the provenance panel dark teal/blue to make the source relationship legible.
- Use only the page's three signal colors: blue, deep teal, and coral. Coral is a point signal, not a section background.

## Interaction budget

Only these interactions are required:

1. Primary CTA scrolls to the search preview.
2. Example query buttons update the visible query and selected state.
3. Search submit gives a small visible demo feedback.

All other content may remain static. Preserve visible focus and reduced-motion behavior.
