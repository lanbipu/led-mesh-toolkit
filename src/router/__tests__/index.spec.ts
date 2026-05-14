import { describe, it, expect, vi } from "vitest";

// Avoid pulling in real view modules (they need Tauri-only globals at parse time).
vi.mock("@/views/Home.vue", () => ({ default: { template: "<div />" } }));
vi.mock("@/views/Design.vue", () => ({ default: { template: "<div />" } }));
vi.mock("@/views/Preview.vue", () => ({ default: { template: "<div />" } }));
vi.mock("@/views/Runs.vue", () => ({ default: { template: "<div />" } }));
vi.mock("@/views/Import.vue", () => ({ default: { template: "<div />" } }));
vi.mock("@/views/Instruct.vue", () => ({ default: { template: "<div />" } }));
vi.mock("@/views/Charuco.vue", () => ({ default: { template: "<div />" } }));
vi.mock("@/views/Photoplan.vue", () => ({ default: { template: "<div />" } }));
vi.mock("@/views/Method.vue", () => ({ default: { template: "<div />" } }));

import { createMemoryHistory, createRouter } from "vue-router";
import { routes } from "../index";

function build() {
  return createRouter({ history: createMemoryHistory(), routes });
}

describe("router routes", () => {
  it("has no /projects/:id/export route", () => {
    const r = build();
    expect(r.hasRoute("export")).toBe(false);
  });

  it("has /projects/:id/method route", () => {
    const r = build();
    expect(r.hasRoute("method")).toBe(true);
  });

  it("catch-all redirects unknown URL (including legacy /export) to /", async () => {
    const r = build();
    await r.push("/projects/5/export");
    await r.isReady();
    expect(r.currentRoute.value.path).toBe("/");
    expect(r.currentRoute.value.name).toBe("home");
  });

  it("catch-all also handles a totally unknown URL", async () => {
    const r = build();
    await r.push("/something/that/never/existed");
    await r.isReady();
    expect(r.currentRoute.value.path).toBe("/");
  });
});
