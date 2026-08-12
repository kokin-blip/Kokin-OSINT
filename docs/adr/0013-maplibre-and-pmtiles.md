# ADR-0013: MapLibre GL JS with PMTiles, self-hosted

- **Status:** accepted
- **Date:** 2026-08-12
- **Reversibility:** easy. The map is an isolated view, not a data dependency.

## Context

Geospatial display is a Phase 2 requirement. The default path — Mapbox or Google
tiles — means an API key, a per-view cost, and, fatally for this product, **a
request to a third party revealing which coordinates an investigator is looking
at**. For a local-first investigation tool that is a privacy leak in the shape
of a feature.

## Decision

MapLibre GL JS rendering PMTiles served from the local filesystem. The
pre-approved z0–z6 world basemap (~80 MB) is downloaded at runtime, never
committed to the repository.

PMTiles is a single-file tile archive addressed by HTTP range request, which
here means local file reads. No tile server, no API key, no per-view egress —
panning a map makes no network requests at all.

## Licensing

- **MapLibre GL JS** reports `NOASSERTION` on GitHub (S-013) because of a
  pre-fork Mapbox header confusing the detector. It is BSD-3-derived and fine to
  ship; `LICENSE.txt` goes in the bundle verbatim.
- **OpenStreetMap-derived tile data is ODbL.** Attribution is **mandatory and
  must be visible in the UI**, not buried in an about box. ODbL share-alike
  attaches to exported map *data*, not to rendered images — so exporting a
  screenshot is unencumbered, while exporting the underlying tile data would
  carry obligations. Phase 1 and 2 export images only.
- The PMTiles format repo also reports `NOASSERTION` (S-014); the format licence
  is separate from the ODbL obligations on the data inside an archive, and the
  two must not be conflated.

## Consequences

- ~80 MB of basemap for offline use, at zoom 0–6 — country and region level.
  Street-level detail requires a larger regional extract the user chooses to
  download; that is a deliberate size trade-off, not an oversight.
- Attribution is a correctness requirement. A missing OSM credit is a licence
  violation, so it belongs in a rendering test, not in a style guide.
- No geocoding service is bundled. Address-to-coordinate lookup would be an
  egress-broker connector with the usual disclosure, not a silent lookup.
