# Hybrid research interfaces and REA basis

The user's final instruction retains official third-party APIs while requesting first-party fallbacks and special-case processing. Local processing is the default; broader external searches remain explicitly reviewed and authorized. There is no automatic remote fallback.

## First-party interfaces

All functions run inside the existing desktop command boundary; none creates a listening HTTP service.

| Command | Local responsibility | Coverage and authorization |
|---|---|---|
| `research_selector` | Validate eight typed inputs, preserve original/normalized values | No network; syntax/checksum validation does not discover accounts |
| `research_table`, `research_table_map`, `research_table_suggestions` | Parse/map bounded text imports and suggest columns | No search; suggestions require review |
| `research_candidates`, `research_table_candidates`, `research_evidence_preview` | Extract syntax candidates from local text/current authenticated PDF/OCR evidence | Candidates unselected; omissions and observation origins preserved |
| `research_local_search` | Version1 local fallback over current encrypted case text index, up to15 queries and50 hits per query/page | Zero egress; local query history; image text lookup explicitly unsupported; indexed leads require inspection |
| `research_import_session` and revisioned documents | Resume encrypted parent import rows and inspect child submission/completion states | Case-owned documents; parent revision and selected queries validated again at start |
| `research_prepare` / existing research-run operation | Plan optional public/official lookups and bind disclosures/call/image/credit budgets | Case-generation check, one-use digest token, explicit consent/start; never automatic |
| `research_results` | Correlated whole-run filters and bounded pages | Unknown is not false; unreadable/shredded source fields excluded before filters |
| `research_review_candidate` | Stage native or selected typed provider account assertions with authenticated source observations | No L2 write; source account never becomes an inferred person |

Existing independently authored native public-source checks remain separate from paid aggregators. Public-source lookups also transmit selectors and therefore stay in the external review panel. Local fallback searches only the case's existing indexed material; it does not promise independent internet-wide account, face, image or breach coverage.

## REA evidence used

REA analyzed the previously acquired, inert OSINT Industries client bundle directory at `research/rea-sources/osint-industries` outside the repository. No target code was executed or incorporated into the app.

- Artifact SHA256: `a418420ea413fcbf2561dcd8c107aab493b3df7fb4069df8fc3cef2ed7018b96`.
- Parent application Evidence: `ev_1a7a43e568b77c9c927a3939a6f7aa6ec6190347717d161bb9552ef8c657c5d4`.
- Summary Evidence: `ev_e1b62d0e39fde1b6e40ce91cb40c9df33859f6cd1d8b2b41d98a640008a111ac`.
- Observed analysis scope:23 JavaScript files,2,064,941 text bytes,527,210 visited AST nodes,50 findings,zero parse failures.
- Coverage is explicitly partial. Worker/semantic resource bounds left14 application and9,076 semantic unknowns; the static graph is not runtime execution evidence. It cannot establish the provider's backend algorithm, corpus, module reliability or coverage.

The existing [142-entry inventory](feature-matrix.md) and earlier retained research describe observed client workflows. The new local API is an independent implementation choice following those workflows, not recovered server implementation or an unofficial internal-endpoint integration.

## Official API contracts

Read through the ChatGPT browser extension in the user-confirmed Jules Work profile on2026-10-20; no Try It/API request was submitted.

- [OI v2 search](https://docs.osint.industries/reference/search-1): documented typed selectors, exact-name and premium options; response module/spec-format envelope.
- [OI spec-format tutorial](https://docs.osint.industries/reference/spec-format): separate module profiles, typed standardized attributes, platform variables, source dates and explicit statement that OI does not supply breach data. The HIBP-compatible breach attribute does not establish an implemented breach source.
- [OI credit lookup](https://docs.osint.industries/reference/credit-lookup): fixed authenticated GET `/misc/credits`, integer credits response.

Offline fixtures establish parser, request and budget behavior. They do not establish current credentials, regional/account eligibility, provider uptime, exhaustive modules or paid-live search accuracy. Phase4 keeps those explicit gates. No proprietary implementation, private index, unknown-license dataset or inspected internal endpoint was copied.
