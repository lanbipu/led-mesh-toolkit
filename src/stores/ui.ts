import { defineStore } from "pinia";
import { ref, watch } from "vue";

export const useUiStore = defineStore("ui", () => {
  const logOpen = ref(false);
  const theme = ref<"light" | "dark">((localStorage.getItem("lmt.theme") as any) ?? "dark");
  const lang = ref<"en" | "zh">((localStorage.getItem("lmt.lang") as any) ?? "en");
  const toasts = ref<Array<{ id: number; kind: "info" | "error" | "success"; msg: string }>>([]);
  let toastSeq = 0;

  watch(
    theme,
    (v) => {
      localStorage.setItem("lmt.theme", v);
      document.documentElement.classList.toggle("dark", v === "dark");
    },
    { immediate: true },
  );
  watch(lang, (v) => localStorage.setItem("lmt.lang", v));

  function toast(kind: "info" | "error" | "success", msg: string) {
    const id = ++toastSeq;
    toasts.value.push({ id, kind, msg });
    setTimeout(() => {
      toasts.value = toasts.value.filter((t) => t.id !== id);
    }, 5000);
  }

  return { logOpen, theme, lang, toasts, toast };
});
