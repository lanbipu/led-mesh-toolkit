import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { SUPPORTED_LOCALES, STORAGE_KEY, type Locale } from "@/locales";

export function useLocale() {
  const { locale } = useI18n();

  const current = computed<Locale>({
    get: () => locale.value as Locale,
    set: (value) => {
      if (!SUPPORTED_LOCALES.includes(value)) return;
      locale.value = value;
      if (typeof window !== "undefined") {
        const storage = window.localStorage;
        if (storage && typeof storage.setItem === "function") {
          storage.setItem(STORAGE_KEY, value);
        }
        document.documentElement.setAttribute("lang", value === "zh" ? "zh-CN" : "en");
      }
    },
  });

  function setLocale(value: Locale) {
    current.value = value;
  }

  return { locale: current, setLocale, supported: SUPPORTED_LOCALES };
}
