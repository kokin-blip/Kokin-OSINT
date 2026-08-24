# Kokin-OSINT Integration Plan

This document outlines a phased, comprehensive architectural and implementation plan to natively integrate features and workflows from industry-standard OSINT tools, websites, and browser extensions into Kokin-OSINT.

## Architectural Changes & Licensing

### 1. GPL & Copyleft Allowance
As of this update, the `deny.toml` and `README.md` have been updated to permit GPL-3.0 and AGPL-3.0 dependencies. This crucial policy shift allows Kokin to statically link or package highly effective, community-maintained Python OSINT tools (like `holehe` and `spiderfoot`) rather than forcing clean-room Rust rewrites for every feature.

### 2. Isolated Docker Web-Crawling Environment
To support dynamic scraping, browser extensions, and active recon (which often require executing JavaScript or interacting with complex DOMs), Kokin will introduce a lightweight, local Docker container (the "Crawler Sandbox").
- **Why?** Tauri’s webview is not designed for stealth scraping, and performing active recon on the host OS can leak the investigator's real IP or fingerprint.
- **How it works:** Kokin-OSINT will spin up an isolated Docker container running Headless Chrome (or Playwright). All network traffic out of this container will be strictly routed through `kokin-net` to preserve SSRF guards, rate limits, and attribution. The container communicates its DOM captures back to Kokin via IPC/gRPC.

---

## Phase 1: Core Target Reconnaissance (The SpiderFoot & Mr. Holmes Integrations)

This phase focuses on the broad "sweep" of a target's digital footprint.

### 1. SpiderFoot (Integration via Connector SDK)
SpiderFoot has 200+ Python modules for sweeping everything from IP blocks to Bitcoin addresses.
- **Implementation:** Instead of running SpiderFoot as a separate web server, Kokin will wrap SpiderFoot’s core python execution engine in a local sandbox. We will map SpiderFoot's event types (e.g., `EMAILADDR`, `IP_ADDRESS`) directly to Kokin’s L0 (raw evidence) and L2 (entity) schema.
- **Feature Gains:** Automated, broad-spectrum sweeping of targets that populates the case database instantly with traceable lineage.

### 2. Mr. Holmes (Domain & IP Recon)
Mr. Holmes specializes in domain reconnaissance, subdomain enumeration, and WHOIS lookups.
- **Implementation:** Rust-native implementation within the `kokin-extract` crate. We will use the native `trust-dns-resolver` and `reqwest` (via `kokin-net`) to perform DNS lookups, banner grabbing, and WHOIS queries.
- **Feature Gains:** Instant mapping of corporate infrastructure.

---

## Phase 2: Social Media & Identity Verification (The TraceLabs Workflow)

TraceLabs CTFs focus heavily on missing persons, where identity, social media footprints, and email associations are critical.

### 1. Holehe (Email-to-Account OSINT)
Holehe checks if an email is attached to an account on 120+ sites (Twitter, Instagram, Imgur, etc.) using forgotten password mechanisms.
- **Implementation:** With GPL now permitted, Kokin will bundle Holehe's Python logic using PyO3 or run it as a subprocess. The output will automatically generate "Claims" (L2) in Kokin linking the Target Email to Social Media Entities.
- **Feature Gains:** Instant pivot from an email address to a massive web of active accounts without alerting the target.

### 2. Toutatis & YesItsMe (Instagram OSINT)
Both tools specialize in extracting specific data from Instagram (user IDs, exact account creation dates, phone number hints, profile pictures).
- **Implementation:** These will be implemented as custom Kokin Connectors that operate within the newly proposed Docker Crawler Sandbox. The sandbox will handle the HTTP headers and session tokens required to safely query IG GraphQL endpoints without burning the user's IP.
- **Feature Gains:** Deep social media forensics, extracting hidden metadata that the web UI obscures.

### 3. TraceLabs Workflows (Native Case Templates)
- **Implementation:** Kokin will introduce "Case Templates." When an investigator starts a case, they can select a "Missing Person / TraceLabs" template. This pre-configures the Entity Graph with required node types (Aliases, Last Known Locations, Vehicle Plates) and sets up automated chronologies based on the TraceLabs intelligence gathering methodology.

---

## Phase 3: Advanced Visual Analysis & Deepfake Detection

This phase bridges the gap between raw data collection and visual intelligence.

### 1. Maltego-Style Graph UI (Native within Kokin)
Maltego’s greatest strength is its visual link analysis graph.
- **Implementation:** We will use `cytoscape.js` or `vis.js` in the Tauri Svelte frontend to render Kokin's L2 entity layer.
- **The "Kokin Advantage":** Unlike Maltego, where the graph *is* the data, Kokin's graph is just a *view* of the data. Clicking any edge or node in the Kokin Graph will instantly open a side-panel showing the immutable L0 raw evidence (the exact JSON response or screenshot) that created that link.

### 2. Deepfake & AI Content Detection (Inspired by Chrome Extensions)
From the `awesome-osint-chrome-extensions` list (specifically Hive AI, Resemble AI, and Copyleaks), detecting manipulated media is critical.
- **Implementation:** We will not install Chrome extensions. Instead, Kokin will integrate local ONNX models into the `kokin-ai` crate.
  - **Audio/Voice:** Implement a local inference model (like a quantized version of Resemblyzer) to score the probability of AI voice generation in ingested video files.
  - **Images:** Implement a local spatial analysis model (e.g., observing face warping, irregular noise patterns, or GAN artifacts).
- **Feature Gains:** When an investigator drags a suspicious image or video into Kokin, the `kokin-ai` crate automatically scans it and attaches a Confidence Claim ("85% probability of AI generation"). All processing happens locally on the investigator's GPU/CPU.