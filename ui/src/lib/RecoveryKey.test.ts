import { describe, expect, it, vi, afterEach } from "vitest";
import { mount, unmount, flushSync } from "svelte";
import RecoveryKey from "./RecoveryKey.svelte";

/**
 * The screen that must not be skippable.
 *
 * `create_case` returns the recovery key once and no command returns it again
 * (D-026). If this screen can be walked past, the case's recovery path is
 * destroyed and nothing says so until a passphrase is forgotten, possibly
 * months later — so the failure is silent, delayed, and unrecoverable, which is
 * the worst combination a UI defect can have.
 *
 * Increment 22 verified all of this by driving the real application and reading
 * screenshots, and said plainly what that was worth: *"a change to
 * RecoveryKey.svelte that made the gate skippable would pass everything in
 * CI."* This file is that sentence being fixed (D-039).
 */

const KEY = "ZBS6VES-FPFB07W-WR4EDYN-J7K6B4Y-TZE360D-6ZHW5NF-SAWGM5W-XF65TS2";
/** Group 7 of 8 — the one the screen asks for. */
const CHALLENGE = "SAWGM5W";

let host: HTMLElement | null = null;
let component: Record<string, unknown> | null = null;

function render(onacknowledge: () => void) {
  host = document.createElement("div");
  document.body.appendChild(host);
  component = mount(RecoveryKey, {
    target: host,
    props: { caseId: "case-under-test", recoveryKey: KEY, onacknowledge },
  });
  flushSync();
  return host;
}

afterEach(() => {
  if (component) void unmount(component);
  host?.remove();
  component = null;
  host = null;
});

function checkbox(root: HTMLElement): HTMLInputElement {
  const found = root.querySelector<HTMLInputElement>('input[type="checkbox"]');
  if (!found) throw new Error("no acknowledgement checkbox");
  return found;
}

function challengeInput(root: HTMLElement): HTMLInputElement {
  const found = root.querySelector<HTMLInputElement>('input[type="text"]');
  if (!found) throw new Error("no challenge input");
  return found;
}

function continueButton(root: HTMLElement): HTMLButtonElement {
  const found = root.querySelector<HTMLButtonElement>("button.continue");
  if (!found) throw new Error("no continue button");
  return found;
}

/** Set a bound input's value the way a person would, then let Svelte react. */
function type(input: HTMLInputElement, value: string) {
  input.value = value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}

function tick(input: HTMLInputElement) {
  input.checked = true;
  input.dispatchEvent(new Event("change", { bubbles: true }));
  flushSync();
}

describe("the recovery key gate", () => {
  it("offers no way out other than acknowledging", () => {
    const onacknowledge = vi.fn();
    const root = render(onacknowledge);

    // No close, no dismiss, no cancel. Enumerated rather than asserted as a
    // count, so a button added later has to be named here to pass.
    const buttons = [...root.querySelectorAll("button")].map((b) =>
      (b.textContent ?? "").trim().toLowerCase(),
    );
    expect(buttons.some((t) => /close|cancel|skip|later|dismiss|not now/.test(t))).toBe(
      false,
    );

    // Escape does nothing. A dialog that closes on Escape is skippable by
    // reflex, which is the same outcome as no screen at all.
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    flushSync();
    expect(root.querySelector('[role="dialog"]')).not.toBeNull();
    expect(onacknowledge).not.toHaveBeenCalled();
  });

  it("does not let the checkbox alone past", () => {
    const onacknowledge = vi.fn();
    const root = render(onacknowledge);

    tick(checkbox(root));

    const button = continueButton(root);
    expect(button.disabled).toBe(true);

    // And clicking it anyway does nothing. `disabled` is the visible half;
    // this is the half that matters if a stylesheet ever hides it.
    button.click();
    flushSync();
    expect(onacknowledge).not.toHaveBeenCalled();
  });

  it("does not let the retyped group alone past", () => {
    const onacknowledge = vi.fn();
    const root = render(onacknowledge);

    type(challengeInput(root), CHALLENGE);

    expect(continueButton(root).disabled).toBe(true);
    continueButton(root).click();
    flushSync();
    expect(onacknowledge).not.toHaveBeenCalled();
  });

  it("asks for a group that a truncated copy would not have kept", () => {
    const root = render(vi.fn());
    // Group 7 of 8. The first is what a screenshot taken before scrolling
    // captures; the last is what a truncated copy most often keeps.
    expect(root.textContent).toContain("Type group 7 of 8 back to confirm");
  });

  it("rejects a different group of the same key", () => {
    const onacknowledge = vi.fn();
    const root = render(onacknowledge);

    tick(checkbox(root));
    // The first group — right key, wrong group. A check that only tested
    // "is this substring somewhere in the key" would pass this.
    type(challengeInput(root), "ZBS6VES");

    expect(continueButton(root).disabled).toBe(true);
    expect(root.textContent).toContain("Does not match yet");
  });

  it("accepts the group in lower case and with read-alike characters", () => {
    const onacknowledge = vi.fn();
    const root = render(onacknowledge);

    tick(checkbox(root));
    // Crockford base32 is case-insensitive and excludes I, L, O and U because
    // they are read-alikes. Someone copying by hand writes what they see, so
    // `5` typed as `s`… is a different letter — but O for 0 is the classic,
    // and the decoder extends that courtesy, so this screen must too.
    type(challengeInput(root), "sawgm5w");

    expect(root.textContent).toContain("Matches.");
    expect(continueButton(root).disabled).toBe(false);
  });

  it("passes only when both conditions hold", () => {
    const onacknowledge = vi.fn();
    const root = render(onacknowledge);

    tick(checkbox(root));
    type(challengeInput(root), CHALLENGE);

    const button = continueButton(root);
    expect(button.disabled).toBe(false);
    button.click();
    flushSync();
    expect(onacknowledge).toHaveBeenCalledTimes(1);
  });

  it("shows the key once and says so", () => {
    const root = render(vi.fn());
    expect(root.textContent).toContain(KEY.split("-")[0]);
    expect(root.textContent).toContain("It is shown once.");
    expect(root.textContent).toContain("case-under-test");
  });
});
