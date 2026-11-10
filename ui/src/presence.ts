// Shared with core/store/presence.rs: stale is grey; manual away wins for
// self, then current DND/snooze, then active versus away/offline/auto-away.
export type Indicator = "active" | "away" | "dnd" | "unknown";
export type Presence = {
  user_id: string;
  presence: string | null;
  dnd_enabled: boolean;
  dnd_start: number;
  dnd_end: number;
  snooze_enabled: boolean;
  snooze_end: number;
  manual_away: boolean | null;
  auto_away: boolean | null;
  online: boolean | null;
  connection_count: number | null;
  presence_fresh: boolean;
  dnd_fresh: boolean;
  updated_at: number;
};

export function dndUntil(p: Presence | undefined, now: number): number | null {
  if (!p?.dnd_fresh) return null;
  const ends = [
    p.snooze_enabled && p.snooze_end > now ? p.snooze_end : 0,
    p.dnd_enabled && p.dnd_start <= now && p.dnd_end > now ? p.dnd_end : 0,
  ];
  return Math.max(...ends) || null;
}

export function indicator(p: Presence | undefined, now: number): Indicator {
  if (!p?.presence_fresh) return "unknown";
  if (p.manual_away) return "away";
  if (dndUntil(p, now)) return "dnd";
  return p.presence === "active" && !p.auto_away && p.online !== false && p.connection_count !== 0
    ? "active" : "away";
}

export type CustomStatus = {
  status_text: string | null;
  status_emoji: string | null;
  status_expiration: number | null;
};

export function visibleStatus(s: CustomStatus | null | undefined, now: number): CustomStatus | null {
  if (!s || (!s.status_text && !s.status_emoji)) return null;
  return s.status_expiration && s.status_expiration <= now ? null : s;
}

export type ClearAfter = "never" | "30m" | "1h" | "4h" | "today" | "week";
export function statusExpiration(after: ClearAfter, now = new Date()): number {
  if (after === "never") return 0;
  if (after === "30m" || after === "1h" || after === "4h") {
    const seconds = { "30m": 1800, "1h": 3600, "4h": 14400 }[after];
    return Math.floor(now.getTime() / 1000) + seconds;
  }
  // Local calendar arithmetic also handles DST. Today ends at 23:59:59;
  // This week ends on Sunday at 23:59:59 in the user's local timezone.
  const end = new Date(now);
  if (after === "week") end.setDate(end.getDate() + (7 - end.getDay()) % 7);
  end.setHours(23, 59, 59, 0);
  return Math.floor(end.getTime() / 1000);
}

export function tomorrowMinutes(now = new Date()): number {
  const end = new Date(now);
  end.setDate(end.getDate() + 1);
  end.setHours(9, 0, 0, 0);
  return Math.max(1, Math.ceil((end.getTime() - now.getTime()) / 60000));
}
