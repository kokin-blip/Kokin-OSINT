"""Reconcile each original inventory item without treating a link as a connector."""
import json
from pathlib import Path
root=Path(__file__).parent
rows=json.loads((root/'feature-matrix.json').read_text())
# Explicit dispositions replace the initial keyword-based phase draft.
groups=[
(range(1,17),1,'native','Reviewed public JSON definitions; optional registrar handoff', 'Public username/domain research', 'crates/kokin-connectors/src/research.rs; src-tauri/src/research.rs; ui/src/lib/features/sources/ResearchWorkspace.svelte', 'research::tests; research_is_grounded_encrypted_cached_and_restorable; ResearchWorkspace.test.ts; desktop/research.spec.mjs'),
(range(17,23),0,'manual workflow','No undocumented API or unknown-license code', 'Public provider information and unsupported profile workflows', 'docs/expansion/licenses.md; src-tauri/src/discovery.rs', 'Review capability labels and prove handoffs do not upload query material'),
(range(23,37),1,'native','Independent permissive source definitions; no WhatsMyName.io dataset', 'Explicit username search with honest source coverage', 'crates/kokin-connectors/src/research.rs; ui/src/lib/features/sources/ResearchWorkspace.svelte', 'every_native_profile_rule_has_positive_negative_and_challenge_contracts; ResearchWorkspace.test.ts; desktop/research.spec.mjs'),
(range(37,45),3,'native','Existing optional YuNet/SFace and contained media workers; public web coverage requires provider', 'Local image/face query preparation and results', 'src-tauri/src/faces.rs; src-tauri/src/media.rs; crates/kokin-faces', 'Existing tests/faces.rs and tests/media.rs; new preparation/category acceptance pending'),
(range(45,49),5,'native','Validated report model; additional renderers pending', 'Cited reports and local research history', 'src-tauri/src/reports.rs; crates/kokin-export/src/research.rs; crates/kokin-store/src/research.rs', 'research_is_grounded_encrypted_cached_and_restorable; report tests; renderer acceptance pending'),
(range(49,55),6,'native','Unlocked-case scheduler and review-only removal workflows pending', 'Monitoring, suppression and removal dossiers', 'crates/kokin-store/src/research_schema.sql', 'Pending: lock/quit cancellation, deduplicated notifications, suppression and removal-scope acceptance'),
(range(55,67),3,'native','Existing media/face workers; CLIP and additional decoders require review/evaluation', 'Local image import, preparation, similarity categories and filters', 'src-tauri/src/media.rs; src-tauri/src/faces.rs; crates/kokin-media', 'Existing media/face tests; new visual-index, edit-recipe and filter acceptance pending'),
(range(67,71),5,'native','Local encrypted packages and research collections; extra renderers pending', 'Selective sharing and collection management', 'crates/kokin-store/src/research.rs; crates/kokin-export/src/research.rs', 'research_is_grounded_encrypted_cached_and_restorable; pending move/copy and full report-format acceptance'),
(range(71,76),6,'native','Unlocked-case scheduling and manual correspondence; no automatic legal sending', 'Monitoring preferences and removal workflow', 'crates/kokin-store/src/research_schema.sql', 'Pending monitoring, catch-up, opt-out dossier and privacy acceptance'),
(range(76,80),4,'provider-backed','Official Lenso/EYEMATCH account, regional eligibility and Keychain credential', 'Official image-category/face API adapter', 'crates/kokin-net/src/provider.rs; crates/kokin-connectors/src/research.rs', 'provider_plans_bind_calls_pages_eligibility_and_premium; provider::tests; live/usage/expanded-response acceptance pending'),
([80],7,'native','Chromium active-tab extension and native messaging not built', 'User-invoked browser capture', '', 'Pending: active-tab permissions, capture-session validation, provenance and no credential/history transfer'),
(range(81,86),2,'native','Validation is separate from discovery; enrichment requires official provider', 'Eight selector classes and source-aware result interpretation', 'crates/kokin-connectors/src/research.rs; crates/kokin-connectors/src/tabular.rs', 'normalization_preserves_identity_and_requires_country; pending wallet checksums and typed account/breach normalization'),
([86,87],5,'native','Existing maps and reports; timeline/additional renderers pending', 'Evidence-linked geography, chronology and reports', 'src-tauri/src/maps.rs; src-tauri/src/reports.rs; crates/kokin-export/src/research.rs', 'Existing maps/report tests; pending timeline and DOCX/PDF/XLSX rendering acceptance'),
(range(88,93),2,'provider-backed','Official provider entitlement; provider assertions require analyst review', 'Progressive enrichment, field filters and coverage', 'src-tauri/src/research.rs; crates/kokin-connectors/src/research.rs', 'Research run/evidence tests; pending source-module normalization and enriched field-filter acceptance'),
(range(93,99),2,'native','Text previews local; PDF/OCR uses existing Evidence workers', 'Mixed-type bulk research with preview and column mapping', 'crates/kokin-connectors/src/tabular.rs; ui/src/lib/features/sources/ResearchWorkspace.svelte', 'tabular::tests; large previews retain all rows while mounting only 50 and selecting at most 15; pending PDF extraction-to-batch and graph staging'),
(range(99,104),3,'native','Contained worker and optional official image providers', 'Image preparation, face selection and experimental content analysis', 'src-tauri/src/media.rs; src-tauri/src/faces.rs', 'Existing media/face tests; pending edit recipes, more import formats, C2PA and representative detector evaluation'),
(range(104,110),5,'native','Existing case boards/review model; no automatic identity acceptance', 'Investigation boards, annotations and chronology', 'ui/src/lib/GraphPanel.svelte; src-tauri/src/workspace.rs', 'Existing graph/workspace tests; pending presentation nodes, grouping, alignment and cross-filter acceptance'),
([110],5,'native','Local encrypted case equivalent; hosted storage/collaboration excluded', 'Organize evidence in encrypted personal cases', 'crates/kokin-export/src/package.rs; crates/kokin-store/src/workspace.rs', 'Existing package/workspace tests; no hosted collaboration claim'),
(range(111,117),4,'provider-backed','Official OSINT Industries v2 contract and Keychain credential', 'Official provider lookups and errors', 'crates/kokin-net/src/provider.rs; crates/kokin-connectors/src/research.rs', 'provider_plans_bind_calls_pages_eligibility_and_premium; pending typed module responses, usage states, stream framing and authorized live checks'),
(range(117,125),0,'blocked','Commercial, enterprise or hosted account scope excluded', 'Retain local case/security equivalents only', 'docs/expansion/implementation-status.md', 'Verify no commercial account, payment, organization, tracking or support-upload subsystem added'),
(range(125,137),1,'native','Pinned MIT Framework catalog and attribution; ratings/notes stay private', 'Local tool directory, details and private review', 'crates/kokin-connectors/src/directory.rs; ui/src/lib/features/sources/DirectoryTree.svelte; ui/src/lib/features/sources/ResearchWorkspace.svelte', 'catalog_is_complete_and_not_executable; ResearchWorkspace.test.ts; desktop/research.spec.mjs; pending full keyboard/theme review'),
(range(137,143),0,'blocked','Research limitations and provider policy; not replicable product guarantees', 'Preserve uncertain ownership, coverage and retention disclosures', 'docs/expansion/licenses.md; docs/expansion/implementation-status.md', 'Review disclosure language against inventory; do not copy proprietary model, quota, accuracy or retention claims'),
]
by_id={}
for ids,phase,delivery,dependency,change,paths,test in groups:
 for n in ids:by_id[n]=(phase,delivery,dependency,change,paths,test)
assert len(by_id)==142
excluded={17,19,20,22,24,40,117,118,119,120,121,123,135}
existing={38,86,105,107,109,110,124,134}
partial=set(range(1,17))|set(range(23,37))|{37,41,42,44,45,46,47,48,55,56,57,58,61,62,63,64,65,67,68,69,70,76,77,78,79,81,82,83,84,85,87,88,89,90,91,92,93,94,95,96,97,98,99,100,101,102,103,104,106,108,111,112,113,114,115,116,122}|set(range(125,137))
for n,row in enumerate(rows,1):
 phase,delivery,dependency,change,paths,test=by_id[n]
 row.update(phase=phase,delivery=delivery,dependency=dependency,required_change=change,implementation_paths=paths,acceptance_test=test)
 row['status']='excluded' if n in excluded else 'existing-partial' if n in existing else 'partial' if n in partial else 'pending'
 row['acceptance_results']='See validation.md. Existing/core checks are recorded separately from new macOS acceptance. No live provider smoke test performed.'
 row['coverage_handling']='Explicit delivered scope only; unavailable sources are not empty results, and similarity/provider assertions do not create identities.'
 row['remaining']='All original clauses must pass their listed acceptance checks before this entry can be marked complete.'
 # Mixed commercial/analyst rows retain relevant analyst work explicitly.
 if n==122:row['required_change']='Native credential presence; retain theme and local issue drafts. Provider-side key management, tokens and report uploads remain manual.'
 if n==124:row['status']='existing-partial';row['delivery']='native';row['required_change']='Preserve existing offline broker, encryption, local case authorization and audit. Hosted Ory/MFA/account sessions excluded.'
 if n in (137,138,139,140,141,142):row['status']='research-limitation';row['required_change']='Do not implement this as a capability or make the disputed proprietary claim. Preserve the inventory limitation in disclosures and delivery status.'
 if n in (15,18,31,65):row['delivery']='manual workflow';row['dependency']='Official browser workflow and explicit Evidence import; no licensed automated consumer API established'
 if n==132:row['required_change']='Private half-star ratings and removal in encrypted case; omit aggregate/community voting.'
 if n==133:row['required_change']='Private dead-link/paywall/incorrect-info draft; submission remains user controlled.'
 if n==136:row['delivery']='manual workflow';row['required_change']='Provide catalog provenance/contribution destination; do not send GitHub messages or install update watchers automatically.'
# Phase 1 dispositions are scoped explicitly; do not erase original inventory text.
phase1=json.loads((root/'phase-1-matrix-overrides.json').read_text())
receipt=root/'phase-1-validation.json'
phase1_passed=receipt.exists() and json.loads(receipt.read_text()).get('exit_criteria_passed') is True
for row in rows:
 if row['id'] in phase1:
  row['required_change']=phase1[row['id']]
  row['coverage_handling']='Approved desktop adaptation. See phase-1-contract.md; vendor counts, undocumented endpoints and exact website mechanics are not copied.'
  row['acceptance_results']='See phase-1-validation.json and phase-1-contract.md. Offline deterministic fixtures and macOS synthetic acceptance; no live source-health or paid-provider claim.'
  row['status']='implemented (desktop scope)' if phase1_passed else 'partial'
  row['remaining']='No Phase 1 delivery work within the approved desktop adaptation. Windows runtime, manual VoiceOver and production packaging retain release gates.' if phase1_passed else 'Final Phase 1 acceptance receipt pending.'
phase2=json.loads((root/'phase-2-matrix-overrides.json').read_text())
for row in rows:
 if row['id'] in phase2:
  row.update(phase2[row['id']])
  row['implementation_paths']='crates/kokin-connectors/src/{research,wallet,tabular,candidates,enrichment}.rs; crates/kokin-store/src/research.rs; src-tauri/src/{research,research_import,research_preview}.rs; ui/src/lib/features/sources/{ResearchWorkspace,ResearchCards,ResearchFilters}.svelte'
  row['acceptance_results']='See phase-2-validation.json. Native/offline tests and separately recorded macOS/owned-loopback acceptance; no paid provider smoke test.'
  row['coverage_handling']='Validation, search snippets, archive indexes and provider assertions remain distinct. Candidates require explicit selection; staging does not create identities or relationships.'
phase3=json.loads((root/'phase-3-matrix-overrides.json').read_text())
for row in rows:
 if row['id'] in phase3:row.update(phase3[row['id']])
(root/'feature-matrix.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n')
lines=['# Expansion traceability — 142 inventory entries','','Each row retains the complete original feature text. **Partial** does not mean feature parity. The source websites’ counts, commercial controls and proprietary claims are not KOKINOSINT guarantees. Detailed dependencies, existing capability, paths, coverage handling and acceptance criteria are in [feature-matrix.json](feature-matrix.json).','','| ID | Source | Required change | Phase | Delivery | Status |','|---|---|---|---:|---|---|']
for r in rows:lines.append('| '+ ' | '.join(str(r[k]).replace('|','\\|').replace('\n',' ') for k in ['id','source','required_change','phase','delivery','status'])+' |')
(root/'feature-matrix.md').write_text('\n'.join(lines)+'\n')
