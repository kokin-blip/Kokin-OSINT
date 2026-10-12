# Motion specification

## Foundation implementation status

Continuation 2026-10-12: sidebar layout transition, initial detail/case-information/operation-panel/settings entry, control border/press feedback and OS/user reduced-motion handling are implemented. Theme backgrounds change immediately to maintain contrast. Closing cases and hiding inspectors remain immediate. See ../increments/ui-night-ledger-settings-layout.md for measured frame/response timing and remaining motion work.

Shared motion tokens, short control hover feedback and reduced-motion CSS are implemented. Sidebar, case and confidence disclosures currently switch immediately. Inspector transitions/resizing, route transitions and graph focus animation below remain proposed; no complete motion-engine or measured frame-rate claim is made.

## Source classification

- **A — Explicit Figma:** no prototype transitions/timings verified in the inspected reusable components. The motion-context query for SideMenu returned no animation nodes.
- **B — Implied states:** Ghost/Menu Normal, Hover, Selected; toggle selections and segmented controls have verified visual states. States do not prove an easing or duration.
- **C — Proposed:** all timings below are Kokin-specific proposals.
- **D — Unavailable:** Pro preview interactions and Code Connect mappings; do not describe these as source-defined motion.

## Shared tokens and behavior

Feedback 120 ms, disclosure 160 ms, layout/navigation 200 ms; easing `cubic-bezier(.2,.8,.2,1)`. Prefer opacity/transform; no animated blur, shadows, ambient image, perpetual particles or pulsing evidence states. Selected route marker responds immediately. Do not animate a verdict as though confidence increased.

Sidebar collapse uses bounded width/layout with content protected from clipping; inspector entry uses opacity and at most 8px translation. Resizing has no easing while dragging. Dialog/menus use opacity and small translation, restore initiating focus, and close by keyboard. Focus rings remain immediate. Graph focus is bounded/cancellable and layout never conveys confidence. Suspend inactive motion; transitions do not delay reads, cancellation or writes.

`prefers-reduced-motion` removes translation and duration. A later persisted preference may further reduce motion; it must not override an OS reduced-motion request. Preserve visible loading text and progress information when motion is disabled. CSS/Svelte transitions only initially; no animation dependency.

Measure resize/selection/input and repeated navigation in the real webview. A 60 FPS goal is not a verified claim until frame measurements exist on documented hardware.
