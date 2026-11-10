import { createSignal } from "solid-js";
import { en, type Key } from "./en.ts";
import { uk } from "./uk.ts";

export type Locale = "en" | "uk";
export type Language = "system" | Locale;
export type Params = Record<string, string | number>;
export type { Key } from "./en.ts";
const [language, setLanguageSignal] = createSignal<Language>("system");
const [clock, setClock] = createSignal<boolean | null>(null);
export { setClock };

export function resolveLocale(preference: Language, system: string): Locale {
  return preference === "system"
    ? system.toLowerCase().startsWith("uk")
      ? "uk"
      : "en"
    : preference;
}

export function locale(): string {
  const system = typeof navigator === "undefined" ? "en" : navigator.language;
  const resolved = resolveLocale(language(), system);
  // Preserve a matching region, including its calendar/clock conventions.
  return system.toLowerCase().startsWith(resolved) ? system : resolved;
}

export function setLanguage(value: Language) {
  setLanguageSignal(value);
  if (typeof document !== "undefined") document.documentElement.lang = locale();
}

export function clock24(): boolean {
  return (
    clock() ?? !new Intl.DateTimeFormat(locale(), { hour: "numeric" }).resolvedOptions().hour12
  );
}

export function interpolate(text: string, params: Params = {}): string {
  return text.replace(/\{(\w+)\}/g, (match, key: string) => String(params[key] ?? match));
}

export function translate(lang: string, key: Key, params?: Params): string {
  return interpolate((lang.startsWith("uk") ? uk : en)[key], params);
}

export function t(key: Key, params?: Params): string {
  return translate(locale(), key, params);
}

export type PluralKey =
  | "members"
  | "replies"
  | "results"
  | "minutesShort"
  | "hoursShort"
  | "pixels";
export function plural(key: PluralKey, n: number, lang: string = locale()): string {
  const rule = new Intl.PluralRules(lang).select(n);
  const form = rule === "one" || rule === "few" || rule === "many" ? rule : "other";
  return translate(lang, `${key}.${form}`, { n });
}

// Never expose an arbitrary Rust/network error as UI copy. Preserve only an API
// error code (not user content, paths, session values, or an English error body).
export function errorMessage(error: unknown): string {
  const raw = String(error);
  const code = raw.match(
    /^(?:Slack API error: )?([a-z]+(?:_[a-z0-9]+)+)(?: \(needed scope: [a-z:,]+\))?$/,
  )?.[1];
  return code ? t("Something went wrong ({code})", { code }) : t("Something went wrong");
}
