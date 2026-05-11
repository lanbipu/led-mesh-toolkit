import { usePreferredDark } from "@vueuse/core";
import { computed, ref, watch } from "vue";

export type ThemeMode = "light" | "dark" | "system";

function getStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  const storage = window.localStorage;
  return storage && typeof storage.getItem === "function" && typeof storage.setItem === "function" ? storage : null;
}

function applyHtmlClass(value: "light" | "dark") {
  if (typeof document === "undefined") return;
  document.documentElement.classList.toggle("dark", value === "dark");
}

export function useColorMode() {
  const storage = getStorage();
  const stored = storage?.getItem("lmt-theme") as ThemeMode | null;
  const mode = ref<ThemeMode>(stored === "light" || stored === "dark" || stored === "system" ? stored : "dark");
  const preferredDark = usePreferredDark();

  const resolved = computed<"light" | "dark">(() => {
    if (mode.value === "system") return preferredDark.value ? "dark" : "light";
    return mode.value;
  });

  watch(
    [mode, resolved],
    () => {
      storage?.setItem("lmt-theme", mode.value);
      applyHtmlClass(resolved.value);
    },
    { immediate: true },
  );

  return { mode, resolved };
}
