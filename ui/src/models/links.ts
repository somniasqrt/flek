// What a link points to, so the timeline can badge meetings, docs and videos.

export type LinkKind = "meeting" | "doc" | "video" | "code" | "web";

export type LinkInfo = {
  kind: LinkKind;
  /// Service name ("Zoom", "Google Docs"), or the host for other sites.
  service: string;
  /// Accent colour of the badge.
  color: string;
};

type Rule = { host: RegExp; path?: RegExp; kind: LinkKind; service: string; color: string };

const RULES: Rule[] = [
  { host: /(^|\.)zoom\.(us|com)$/, kind: "meeting", service: "Zoom", color: "#2d8cff" },
  { host: /^meet\.google\.com$/, kind: "meeting", service: "Google Meet", color: "#00897b" },
  { host: /^teams\.(microsoft|live)\.com$/, kind: "meeting", service: "Teams", color: "#5b5fc7" },
  { host: /^docs\.google\.com$/, path: /^\/document/, kind: "doc", service: "Google Docs", color: "#4285f4" },
  { host: /^docs\.google\.com$/, path: /^\/spreadsheets/, kind: "doc", service: "Google Sheets", color: "#0f9d58" },
  { host: /^docs\.google\.com$/, path: /^\/presentation/, kind: "doc", service: "Google Slides", color: "#e37400" },
  { host: /^docs\.google\.com$/, path: /^\/forms/, kind: "doc", service: "Google Forms", color: "#7248b9" },
  { host: /^(drive|docs)\.google\.com$/, kind: "doc", service: "Google Drive", color: "#1a73e8" },
  { host: /(^|\.)(youtube\.com|youtu\.be)$/, kind: "video", service: "YouTube", color: "#e62117" },
  { host: /(^|\.)github\.com$/, kind: "code", service: "GitHub", color: "#6e7681" },
];

export function linkInfo(url: string): LinkInfo {
  try {
    const u = new URL(url);
    const host = u.hostname.toLowerCase();
    for (const r of RULES) {
      if (r.host.test(host) && (!r.path || r.path.test(u.pathname))) {
        return { kind: r.kind, service: r.service, color: r.color };
      }
    }
    return { kind: "web", service: host.replace(/^www\./, ""), color: "var(--accent)" };
  } catch {
    return { kind: "web", service: url, color: "var(--accent)" };
  }
}

/// Short form of a bare link: host + a trimmed path.
export function shortUrl(url: string): string {
  try {
    const u = new URL(url);
    const path = u.pathname === "/" ? "" : u.pathname;
    const s = u.hostname.replace(/^www\./, "") + path;
    return s.length > 48 ? s.slice(0, 47) + "…" : s;
  } catch {
    return url;
  }
}
