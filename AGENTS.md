# AGENTS.md

## Project

Nexus is a serious long-term, local-first Personal Data OS. It indexes
and retrieves a user's personal digital information. The current
priority is a reliable local indexing/search foundation, not AI
features.

## Product Principles

1.  Local-first by default.
2.  User data ownership is mandatory.
3.  Core functionality must work without AI.
4.  Reliability \> feature count.
5.  Search quality \> visual novelty.
6.  Prefer simple explicit designs over clever designs.
7.  Do not introduce unnecessary dependencies or premature abstractions.

## Approved Product UI Baseline

The Nexus desktop interface approved on 2026-09-03 is the final visual and
interaction baseline. Its visual language is **Organic Memory Field**. “Final”
freezes the style system and core information hierarchy; it does not freeze
product functionality or prevent accessibility, responsive, feedback, or bug
fixes that remain within the system.

Before modifying product UI, read `design-system/nexus/MASTER.md` and the
relevant page override. The production baseline is represented by:

-   `apps/desktop/src/index.css` for tokens, typography, surfaces, and layout;
-   `apps/desktop/src/App.tsx` and `apps/desktop/src/SearchView.tsx` for the
    approved shell and search interaction hierarchy;
-   `apps/desktop/public/nexus-product-icon.png` for the real Nexus product
    icon; and
-   `demo/Nexus Visual Direction.html` as the supporting visual-direction
    reference.

Search remains the interface axis and must visibly preserve
`query → result → source`. Visual work must preserve existing Tauri commands,
Rust/TypeScript data structures, search and indexing logic, routes, form fields,
test selectors, and error handling unless a separately approved product change
explicitly changes them.

Do not introduce a competing palette, typography system, visual language,
information hierarchy, or replacement product mark without explicit human
approval. An approved baseline change must update the product specification,
design-system documents, and implementation together.

## Human / Codex Responsibilities

The human developer owns product direction and major architectural
decisions.

Codex may investigate, implement, test, debug, document, refactor within
scope, and review code.

Do not silently decide major: - dependencies - data-model changes -
public interfaces - concurrency architecture - security/privacy
behavior - cross-module architectural changes

For these, present tradeoffs first unless implementation was explicitly
approved.

## Required Workflow

For non-trivial tasks:

### Investigate

Read relevant code, tests, specs, architecture docs, and existing
utilities before editing.

### Plan

For multi-file or architectural changes, provide a concise plan
including behavior, affected modules, edge cases, and tests.

### Implement

Make the smallest coherent change satisfying the task. Do not perform
unrelated cleanup.

### Validate

Run relevant tests, type checks, formatting, linting, and build checks.
Never claim a check passed unless it actually ran.

### Review

Review the diff for correctness, regression risk, edge cases, error
handling, complexity, duplication, performance, and missing tests.

### Report

Summarize changes, important decisions, tests actually executed, and
unresolved risks.

## Architecture Rules

-   Keep domain logic outside UI components.
-   UI must not directly own database/index business logic.
-   Parsers must not depend on UI behavior.
-   Search should operate on normalized documents rather than individual
    file extensions.
-   Isolate platform-specific behavior where practical.
-   Do not build a plugin framework until a real requirement needs one.

## Domain Direction

Long-term conceptual model:

``` text
Source -> Parser -> Document
```

A Document may eventually represent files, PDFs, source code, webpages,
notes, emails, or repository content.

Do not over-generalize this model during early milestones.

## Error Handling

Expected environmental failures must not crash the application: -
permission denied - deleted/moved files - inaccessible directories -
malformed documents - unsupported formats - temporary filesystem errors

In production Rust paths, avoid panic/unwrap/expect for recoverable
failures. Preserve useful error context.

## Performance

Design with eventual scale in mind: - 100,000+ files - 1,000,000+
indexed records - multi-GB data

Prefer streaming/batching for large operations. Avoid speculative
optimization; measure when practical.

## Concurrency

When concurrency is justified, consider cancellation, partial failure,
ordering, duplicate events, shutdown, and UI blocking. Do not introduce
concurrency merely because it is possible.

## Database

SQLite is the default local metadata database unless an approved ADR
changes it. Schema changes must be explicit and migration-safe. Never
silently destroy user data. Keep DB access outside UI code.

## Search

Deterministic lexical search comes before semantic search. Future
retrieval may combine lexical search, metadata filters, semantic search,
fusion, and reranking. Never replace deterministic search with LLM-based
retrieval.

## Testing

Every non-trivial domain feature should have tests. Bug fixes should
include regression tests when practical. Prefer deterministic tests.
Filesystem tests should use isolated temporary directories/fixtures.
External-network tests must be explicitly separated.

## Security & Privacy

Never upload user files, filenames, contents, embeddings, metadata, or
search history unless an explicit feature requires it and the user has
opted in. Never log sensitive document contents by default. Never commit
secrets.

## AI Integration

AI is an optional layer above the core system. Core indexing/search must
not require an LLM. LLM output is untrusted and must be validated before
being treated as structured data. AI failure must never corrupt the
local data store. Retrieval sources should remain traceable.

## Dependency Policy

Before adding a dependency: 1. search for existing repository
functionality; 2. check whether standard/existing dependencies suffice;
3. evaluate maintenance/security implications; 4. prefer established
packages.

Mention major new dependencies in the completion report.

## Scope Control

Do not implement later milestones unless explicitly requested.

Default order: M0 Engineering Foundation M1 File Scanner M2 Content
Parsing M3 Full-text Search M4 Incremental Indexing M5 Semantic Search
M6 Ask Nexus M7 Personal Timeline M8 Agent Layer

Do not introduce authentication, cloud sync, multi-user systems,
microservices, Kubernetes, or complex plugin infrastructure without an
explicit requirement.

## Definition of Done

A task is done only when required behavior exists, relevant tests exist,
executed checks pass, no known critical regression remains, and
documentation is updated when necessary.

## Final Rule

Codex accelerates implementation; it does not replace engineering
judgment. Prefer a smaller system that is correct and understood over a
larger system that merely looks impressive.
