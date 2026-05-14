import { describe, it, expect } from "vitest";
import { mount } from "@vue/test-utils";
import { defineComponent, h } from "vue";
import LmtConfirmDialog from "../LmtConfirmDialog.vue";

// reka-ui Dialog uses Teleport which is brittle under happy-dom.
// Stub the four reka-ui dialog parts as inline passthrough so we can test
// the OK/Cancel emit behavior of LmtConfirmDialog itself.
const passthrough = defineComponent({
  props: ["open"],
  setup(_, { slots }) {
    return () => h("div", { "data-stub": true }, slots.default?.());
  },
});

const stubs = {
  DialogRoot: passthrough,
  DialogPortal: passthrough,
  DialogOverlay: passthrough,
  DialogContent: passthrough,
  DialogTitle: defineComponent({ setup(_, { slots }) { return () => h("h2", slots.default?.()); } }),
  DialogDescription: defineComponent({ setup(_, { slots }) { return () => h("p", slots.default?.()); } }),
};

function mountDlg(props: Record<string, unknown>) {
  return mount(LmtConfirmDialog, {
    props: {
      open: true,
      title: "T",
      body: "B",
      okLabel: "Yes",
      cancelLabel: "No",
      ...props,
    },
    global: { stubs },
  });
}

describe("LmtConfirmDialog", () => {
  it("renders title and body when open=true", () => {
    const w = mountDlg({ title: "Switch?", body: "Are you sure?" });
    expect(w.text()).toContain("Switch?");
    expect(w.text()).toContain("Are you sure?");
  });

  it("emits confirm + update:open(false) when ok clicked", async () => {
    const w = mountDlg({});
    await w.find("button[data-confirm-ok]").trigger("click");
    expect(w.emitted("confirm")).toBeTruthy();
    const updates = w.emitted("update:open") ?? [];
    expect(updates.at(-1)).toEqual([false]);
  });

  it("emits only update:open(false) when cancel clicked", async () => {
    const w = mountDlg({});
    await w.find("button[data-confirm-cancel]").trigger("click");
    expect(w.emitted("confirm")).toBeFalsy();
    const updates = w.emitted("update:open") ?? [];
    expect(updates.at(-1)).toEqual([false]);
  });
});
