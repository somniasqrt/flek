// Time labels shared by the timeline and the thread panel. Pure (no Solid),
// so `node --test` can load it.

const DAY = 86_400_000;

const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();

/// "just now", "5 minutes ago", "today at 14:03", "yesterday at 09:10",
/// "3 days ago", "2 months ago": how Slack labels a thread's last reply.
export function relativeTime(date: Date, now: Date, locale?: string): string {
  const diff = now.getTime() - date.getTime();
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  if (diff < 60_000) return "just now";
  if (diff < 3_600_000) return rtf.format(-Math.floor(diff / 60_000), "minute");
  const clock = date.toLocaleTimeString(locale, {
    hour: "2-digit",
    minute: "2-digit",
  });
  const days = Math.round((startOfDay(now) - startOfDay(date)) / DAY);
  if (days <= 0) return `today at ${clock}`;
  if (days === 1) return `yesterday at ${clock}`;
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
  locale?: string,
): string | null {
  const opts: Intl.DateTimeFormatOptions = {
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

/// Compact rail-list time; dates use English Slack-style day/month order.
export function listTime(date: Date, now: Date): string {
  const minutes = Math.max(0, Math.floor((now.getTime() - date.getTime()) / 60_000));
  if (minutes < 1) return "Just now";
  if (minutes < 60) return `${minutes} ${minutes === 1 ? "min" : "mins"}`;
  if (minutes < 24 * 60) {
    const hours = Math.floor(minutes / 60);
    return `${hours} ${hours === 1 ? "hour" : "hours"}`;
  }
  const days = Math.round((startOfDay(now) - startOfDay(date)) / DAY);
  if (days < 7) return date.toLocaleDateString("en-GB", { weekday: "long" });
  return date.toLocaleDateString("en-GB", {
    day: "numeric",
    month: "long",
    year: date.getFullYear() === now.getFullYear() ? undefined : "numeric",
  });
}

export function activityDay(date: Date, now: Date): string {
  const days = Math.round((startOfDay(now) - startOfDay(date)) / DAY);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days > 1 && days < 7) return date.toLocaleDateString("en-GB", { weekday: "long" });
  return date.toLocaleDateString("en-GB", {
    day: "numeric",
    month: "long",
    year: date.getFullYear() === now.getFullYear() ? undefined : "numeric",
  });
}
