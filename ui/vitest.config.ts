import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";

/**
 * Component tests.
 *
 * Every UI claim in increments 22 and 23 rested on screenshots I read, which
 * verifies a build and guards nothing. Both increment reports named the same
 * gap: a change that made the recovery-key gate skippable, or that rendered a
 * shredded document as a fault, would pass every check in CI.
 *
 * These tests exist for the handful of components where **the rendering is the
 * product** — where getting the presentation wrong is not a cosmetic bug but a
 * false statement about evidence. They are deliberately not a general UI test
 * suite; there is no value in asserting that a heading says what the source says
 * it says. D-039 records why this shape and not an end-to-end driver.
 *
 * `hot: false` because the Svelte plugin's HMR wrapper interferes with mounting
 * components directly, and nothing here needs hot reload.
 */
export default defineConfig({
  plugins: [svelte({ hot: false })],
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
    // Resolve Svelte's browser entry rather than its SSR one: these tests mount
    // components into a real DOM and assert what a person would see.
    alias: [{ find: /^svelte$/, replacement: "svelte" }],
  },
  resolve: {
    conditions: ["browser"],
  },
});
