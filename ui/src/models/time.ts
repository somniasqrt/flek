import { locale as currentLocale, clock24, translate, plural } from "../i18n/index.ts";

// Time formatters take explicit locales for tests; UI defaults read preferences.

function clockPreference(lang: string): boolean {
  // Explicit locale callers (tests/formatters) get that locale's convention;
  // UI callers pass the clock preference where they need an override.
  return !!new Intl.DateTimeFormat(lang, { hour: "numeric" }).resolvedOptions().hour12;
}

const DAY = 86_400_000;

const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();

/// "just now", "5 minutes ago", "today at 14:03", "yesterday at 09:10",
/// "3 days ago", "2 months ago": how Slack labels a thread's last reply.
export function relativeTime(
  date: Date,
  now: Date,
  locale: string = currentLocale(),
  hour12 = locale === currentLocale() ? !clock24() : clockPreference(locale),
): string {
  const diff = now.getTime() - date.getTime();
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  if (diff < 60_000) return translate(locale, "just now");
  if (diff < 3_600_000) return rtf.format(-Math.floor(diff / 60_000), "minute");
  const clock = date.toLocaleTimeString(locale, {
    hour12,
    hour: "2-digit",
    minute: "2-digit",
  });
  const days = Math.round((startOfDay(now) - startOfDay(date)) / DAY);
  if (days <= 0) return translate(locale, "today at {time}", { time: clock });
  if (days === 1) return translate(locale, "yesterday at {time}", { time: clock });
  if (days < 30) return rtf.format(-days, "day");
  const months = (now.getFullYear() - date.getFullYear()) * 12 + now.getMonth() - date.getMonth();
  if (months < 12) return rtf.format(-Math.max(1, months), "month");
  return rtf.format(-Math.floor(months / 12), "year");
}

/// A person's wall-clock time ("5:10 PM"): from their IANA zone, else from
/// the UTC offset Slack recorded (seconds east), else `null`.
export function localTime(
  now: Date,
  tz: string | null,
  offset: number | null,
  locale: string = currentLocale(),
  hour12: boolean = locale === currentLocale() ? !clock24() : clockPreference(locale),
): string | null {
  const opts: Intl.DateTimeFormatOptions = {
    hour12,
    hour: "numeric",
    minute: "2-digit",
  };
  if (tz) {
    try {
      return now.toLocaleTimeString(locale, { ...opts, timeZone: tz });
    } catch {
      // Unknown zone name: fall back to the offset.
    }
  }
  if (offset === null || !Number.isFinite(offset)) return null;
  return new Date(now.getTime() + offset * 1000).toLocaleTimeString(locale, {
    ...opts,
    timeZone: "UTC",
  });
}

/// Compact rail-list time and localised calendar dates.
export function listTime(date: Date, now: Date, locale = currentLocale()): string {
  const minutes = Math.max(0, Math.floor((now.getTime() - date.getTime()) / 60_000));
  if (minutes < 1) return translate(locale, "Just now");
  if (minutes < 60) return plural("minutesShort", minutes, locale);
  if (minutes < 24 * 60) {
    const hours = Math.floor(minutes / 60);
    return plural("hoursShort", hours, locale);
  }
  const days = Math.round((startOfDay(now) - startOfDay(date)) / DAY);
  if (days < 7) return date.toLocaleDateString(locale, { weekday: "long" });
  return date.toLocaleDateString(locale, {
    day: "numeric",
    month: "long",
    year: date.getFullYear() === now.getFullYear() ? undefined : "numeric",
  });
}

export function activityDay(date: Date, now: Date, locale = currentLocale()): string {
  const days = Math.round((startOfDay(now) - startOfDay(date)) / DAY);
  if (days === 0) return translate(locale, "Today");
  if (days === 1) return translate(locale, "Yesterday");
  if (days > 1 && days < 7) return date.toLocaleDateString(locale, { weekday: "long" });
  return date.toLocaleDateString(locale, {
    day: "numeric",
    month: "long",
    year: date.getFullYear() === now.getFullYear() ? undefined : "numeric",
  });
}
