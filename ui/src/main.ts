import { mount } from "svelte";
import App from "./App.svelte";
import "./lib/design/tokens.css";

async function prepareDriver(): Promise<void> {
  if (import.meta.env.VITE_DESKTOP_TEST === "1") await import("@wdio/tauri-plugin");
}
void prepareDriver();

const target = document.getElementById("app");
if (!target) {
  throw new Error("#app mount point missing from index.html");
}

// Vite removes this branch and its lazy module from production builds.
if (import.meta.env.DEV && new URLSearchParams(location.search).has('gallery')) {
  void import('./lib/design/Gallery.svelte').then(({default: Gallery}) => mount(Gallery, {target}));
} else {
  mount(App, { target });
}
