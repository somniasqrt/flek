// Preferences are persisted by core; per-view layout and drafts remain local.
export type Settings = {
  version: number;
  language: "system" | "en" | "uk";
  theme: "system" | "light" | "dark";
  text_size: "small" | "default" | "large";
  clock_24: boolean | null;
  launch_at_login: boolean;
  desktop_import_on_start: boolean;
  keep_in_menu_bar: boolean;
  dm_sort: "recent" | "name";
  show_muted: boolean;
  presence_dots: boolean;
  link_previews: boolean;
  image_previews: boolean;
  max_file_bytes: number | null;
  refresh_days: number;
  refresh_hours: number;
};

export const defaultSettings: Settings = {
  version: 1,
  language: "system",
  theme: "system",
  text_size: "default",
  clock_24: null,
  launch_at_login: false,
  desktop_import_on_start: true,
  keep_in_menu_bar: true,
  dm_sort: "recent",
  show_muted: true,
  presence_dots: true,
  link_previews: true,
  image_previews: true,
  max_file_bytes: 100 * 1024 * 1024,
  refresh_days: 7,
  refresh_hours: 3,
};

export function sortDms<T extends { label: string; ts?: string | null; latest_ts?: string | null }>(
  rows: readonly T[],
  sort: Settings["dm_sort"],
  locale: string,
): T[] {
  return [...rows].sort((a, b) => {
    if (sort === "name") return a.label.localeCompare(b.label, locale);
    return (
      (b.ts ?? b.latest_ts ?? "").localeCompare(a.ts ?? a.latest_ts ?? "") ||
      a.label.localeCompare(b.label, locale)
    );
  });
}

export function messageSize(size: Settings["text_size"]): string {
  return { small: "13px", default: "15px", large: "18px" }[size];
}
