import { createI18n } from "vue-i18n";
import en from "./en.json";
import zh from "./zh.json";

export type MessageSchema = typeof en;
export type Locale = "en" | "zh";
export const SUPPORTED_LOCALES: Locale[] = ["en", "zh"];
export const STORAGE_KEY = "lmt-locale";

function detectInitialLocale(): Locale {
  if (typeof window === "undefined") return "en";
  const storage = window.localStorage;
  const stored =
    storage && typeof storage.getItem === "function"
      ? storage.getItem(STORAGE_KEY)
      : null;
  if (stored === "en" || stored === "zh") return stored;
  const nav = window.navigator?.language?.toLowerCase() ?? "";
  return nav.startsWith("zh") ? "zh" : "en";
}

const initialLocale = detectInitialLocale();

if (typeof document !== "undefined") {
  document.documentElement.setAttribute(
    "lang",
    initialLocale === "zh" ? "zh-CN" : "en",
  );
}

export const i18n = createI18n<[MessageSchema], Locale>({
  legacy: false,
  globalInjection: true,
  locale: initialLocale,
  fallbackLocale: "en",
  messages: { en, zh },
});

declare module "vue-i18n" {
  export interface DefineLocaleMessage extends MessageSchema {}
}
