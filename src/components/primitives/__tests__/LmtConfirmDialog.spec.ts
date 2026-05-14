import { describe, it, expect } from "vitest";
import { mount } from "@vue/test-utils";
import { defineComponent, h } from "vue";
import LmtConfirmDialog from "../LmtConfirmDialog.vue";

// reka-ui Dialog uses Teleport which is brittle under happy-dom.
// Stub the reka-ui parts; DialogRoot forwards its `open` prop to a data attribute
// AND gates child rendering so tests can detect actual open/close transitions.
const dialogRoot = defineComponent({
  props: ["open"],
  setup(props, { slots }) {
    return () =>
      h(
        "div",
        { "data-dialog-root": true, "data-dialog-open": String(!!props.open) },
        props.open ? slots.default?.() : [],
      );
  },
});
const passthrough = defineComponent({
  setup(_, { slots }) {
    return () => h("div", { "data-stub": true }, slots.default?.());
  },
});

const stubs = {
  DialogRoot: dialogRoot,
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
    expect(w.find("[data-dialog-root]").attributes("data-dialog-open")).toBe("true");
    expect(w.text()).toContain("Switch?");
    expect(w.text()).toContain("Are you sure?");
  });

  it("does not render children when open=false", () => {
    const w = mountDlg({ open: false });
    expect(w.find("[data-dialog-root]").attributes("data-dialog-open")).toBe("false");
    expect(w.find("button[data-confirm-ok]").exists()).toBe(false);
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
