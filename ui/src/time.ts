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
  const clock = date.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });
  const days = Math.round((startOfDay(now) - startOfDay(date)) / DAY);
  if (days <= 0) return `today at ${clock}`;
  if (days === 1) return `yesterday at ${clock}`;
  if (days < 30) return rtf.format(-days, "day");
  const months =
    (now.getFullYear() - date.getFullYear()) * 12 + now.getMonth() - date.getMonth();
  if (months < 12) return rtf.format(-Math.max(1, months), "month");
  return rtf.format(-Math.floor(months / 12), "year");
}
