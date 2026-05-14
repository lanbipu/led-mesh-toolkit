import { defineStore } from "pinia";
import { ref, watch } from "vue";

// Theme is owned by `composables/useColorMode.ts` (key: lmt-theme).
// This store intentionally does not touch theme; an earlier copy here
// was dead code that fought useColorMode's html class on init.

export const useUiStore = defineStore("ui", () => {
  const logOpen = ref(false);
  const lang = ref<"en" | "zh">((localStorage.getItem("lmt.lang") as any) ?? "en");
  const toasts = ref<Array<{ id: number; kind: "info" | "error" | "success"; msg: string }>>([]);
  let toastSeq = 0;

  watch(lang, (v) => localStorage.setItem("lmt.lang", v));

  function toast(kind: "info" | "error" | "success", msg: string) {
    const id = ++toastSeq;
    toasts.value.push({ id, kind, msg });
    setTimeout(() => {
      toasts.value = toasts.value.filter((t) => t.id !== id);
    }, 5000);
  }

  return { logOpen, lang, toasts, toast };
});
