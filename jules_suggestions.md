# Jules' Suggestions for Kokin-OSINT

This document outlines potential open-source data sources, tools, and local AI features that could be integrated into Kokin-OSINT. These suggestions aim to expand capabilities, bridge gaps identified in the competitor matrix, and maintain the project's local-first, privacy-respecting architecture.

## Suggested Sources (OSINT Data & Tools)

The following sources could be integrated either via Kokin's connector framework (`kokin-net`) or natively within the data model. All suggestions prioritize open-source availability or public access.

### 1. Identity & Breach Data
- **HaveIBeenPwned (HIBP) API**: A classic source for breach data. While the full database is commercial, the public API (with an API key) can verify if an email or phone number has appeared in a breach.
- **DeHashed (API)**: Similar to HIBP but offers more granular search capabilities (e.g., searching by name, address, or IP).
- **Epieos**: Open-source tool (and website) for reverse email search, extracting data from Google accounts, Skype, etc., without alerting the target.

### 2. Infrastructure & Network Data
- **Shodan / Censys / Zoomeye**: Essential for mapping IP addresses, open ports, and vulnerable infrastructure. Their APIs can be connected to Kokin to map out digital footprints.
- **BGPView / Hurricane Electric BGP Toolkit**: For ASN and IP prefix routing data. Completely free and public, useful for attributing IP blocks to organizations.
- **crt.sh**: Certificate Transparency logs. Excellent for subdomain enumeration and tracking domain ownership changes over time.
- **URLScan.io**: A sandbox for scanning URLs. It provides historical snapshots, DOM data, and IP connections for a given domain, which could be ingested as raw evidence.

### 3. Corporate & Financial Intelligence
- **OpenCorporates**: The largest open database of companies in the world. Their API can trace corporate structures, directors, and shell companies.
- **Aleph (OCCRP)**: As mentioned in the competitor matrix, OCCRP Aleph's public instance holds vast amounts of investigative data. Kokin could pull specific document links or entity references from Aleph via its API to establish cross-platform lineage.

### 4. Geospatial & Flight/Maritime Data
- **OpenStreetMap (OSM) / Overpass API**: For querying specific geographical features or locations tied to an investigation.
- **OpenSky Network / ADS-B Exchange**: Open-source flight tracking data. Useful for tracking aircraft ownership and movement history.
- **AisHub / MarineTraffic (Free Tier)**: For maritime tracking and vessel ownership records.

### 5. Social Media & Archiving
- **Wayback Machine (Internet Archive) API**: To pull historical versions of web pages as immutable raw evidence.
- **Social Analyzer**: An open-source tool for finding social media profiles across 1000+ websites. Similar to WhatsMyName but potentially with a different license structure.
- **GHunt**: An open-source OSINT tool to extract information from any Google Account using an email address.

---

## Suggested Local AI Features

These features leverage Kokin-OSINT's `kokin-ai` crate (which currently uses `ort` and local inference) to keep all processing on the device, ensuring privacy and maintaining the offline-capable invariant.

### 1. Local Entity Extraction & Resolution (NLP)
- **Feature**: Use a lightweight, local LLM (e.g., Llama.cpp, Mistral-7B, or a fine-tuned BERT model) to automatically extract entities (names, organizations, locations, dates) from raw text evidence.
- **How it connects**: Extracted entities would automatically have a provenance lineage (L0 -> L2) linking back to the specific raw text block they were parsed from.
- **Competitor Advantage**: Most competitors either rely on cloud APIs for NLP (breaking privacy) or require manual entity creation. Local, offline entity extraction with explicit lineage is a massive differentiator.

### 2. Cross-Lingual Semantic Search & Offline Translation
- **Feature**: Integrate an offline translation model (e.g., Argos Translate, Helsinki-NLP) and multilingual embeddings (e.g., BGE-m3).
- **How it connects**: Analysts can ingest foreign-language documents, translate them locally, and search across their entire case in English (or their native language). The translation step itself would be recorded as a transformation in the lineage chain.
- **Competitor Advantage**: Essential for international investigations. Doing this offline prevents leaking sensitive documents to Google Translate or DeepL.

### 3. Local Image Classification & Object Detection (Computer Vision)
- **Feature**: Use local ONNX models (like YOLO or CLIP) to automatically scan ingested images for specific objects (e.g., weapons, vehicles, flags, specific buildings) or facial embeddings (for clustering, not identification).
- **How it connects**: The AI observation becomes a "claim" (L2) linked directly to the image (L0 evidence). The analyst can review the bounding box and either accept or reject the AI's confidence score.
- **Competitor Advantage**: Hunchly captures images but doesn't analyze them. OSINTBuddy requires Python plugins that often call out to the web. Built-in, local vision AI natively tied to the evidence graph is unique.

### 4. Automated Timeline Generation
- **Feature**: A local model parses dates and events from text/metadata and constructs a chronological timeline.
- **How it connects**: It builds upon the entity graph by adding a temporal dimension. Every event on the timeline clicks directly back to the source document.
- **Competitor Advantage**: IBM i2 has timeline views, but they are often manually constructed. Automating this locally with verifiable lineage bridges the gap between raw data and analytical storytelling.

### 5. Confidence Grading via "Devil's Advocate" AI
- **Feature**: An optional, local LLM agent that reviews an analyst's derived conclusion (e.g., "Entity A is the same person as Entity B") and suggests alternative hypotheses based on the raw evidence (e.g., "Could it be a relative with the same name? The birth dates are missing").
- **How it connects**: Ties directly into Kokin's multi-dimensional confidence grading. It acts as an automated peer-reviewer that only has access to the local case data.
- **Competitor Advantage**: No existing OSINT tool actively challenges the analyst's cognitive bias. This aligns perfectly with Kokin's philosophy of separating evidence from conclusion.