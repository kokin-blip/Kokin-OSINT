# Figma to Svelte mapping — Night Ledger

| Page / node | Component / variants | Visual properties | Svelte target / usage | Asset and motion status | Priority |
|---|---|---|---|---|---|
| DesignCode UI / 23201:64056 | Dark 16 Nov composition preview | Broad lead panel, asymmetric small panels, tall contextual column, black canvas | NightLedger layout / entity workspace | Raster composition verified; editable layers unavailable; motion proposed | P0 |
| Menus / 23204:130790 | SideMenu Dark Glass | Inter 14, active left marker, outline icons, dividers | Workbench navigation | Screenshot verified; adapt quiet surface; timings unspecified | P0 |
| Menus / 23204:129892 | Search; mode/size/style | Bounded input, outline icon, compact controls | SearchField / explicit case search | Variants verified; no per-keystroke search | P0 |
| Buttons / 104:6313 | Primary; mode/size/style | 8px corner, blue action, 28–44px heights | Button / native import, evidence navigation, review | Variants verified; no exact easing verified | P0 |
| Buttons / 184:9849 | Ghost; Normal/Hover/Selected | Quiet borderless controls | Button ghost / back, close, navigation | State variants verified | P0 |
| Cards / 23204:134507 | ListCard; Light/Dark Glass, Light Outline/Flat | Icon, title, text, row structure | Panel and evidence/identifier rows | Structure verified; adapt case data | P0 |
| DesignCode UI / 370:732420; source set 249:28723 | Inspector Detail / available styles | Metadata groups and activity separators | ContextInspector / own evidence and seven dimensions | Light instance screenshot verified; do not copy bank/payment sections | P0 |
| Menus / 181:6332 | SegmentedControl | Selected segment, dividers, compact size | Tabs / source, observations, metadata | State variants verified; keyboard model must be implemented | P1 |
| Cards / 23204:134600 | Notification / mode/style | Grouped content and action | Operation status and error notices | Structure verified; preserve actionable error codes | P1 |
| Typography & Content | Inter styles | Weight/type scale and line height | Native font and semantic tokens | Inter deferred: OFL outside current license allowlist; no external font fetching | P0 |
| Icons / file and communication groups | Outline glyphs | Thin outlines and matching sizes | Licensed icon assets | Exact exported source assets or reviewed small icon set; no hand-drawn approximations | P1 |

Build only useful variants. Start Button, Badge, Panel, EmptyState, Field and Disclosure; add Dialog/Popover/Toast/SplitPane as migrated workflows need them. The development gallery must show focus, disabled, loading, errors, long text and narrow layouts. No fake production data or controls for unimplemented services.
