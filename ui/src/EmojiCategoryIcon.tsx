const paths: Record<string, string> = {
  search: "M15 15l6 6M17 10a7 7 0 1 1-14 0 7 7 0 0 1 14 0",
  people: "M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0M8 14c2 3 6 3 8 0M8 9h.01M16 9h.01",
  nature: "M5 20 17 6M5 14C1 6 10 2 20 2c0 12-5 19-13 15M9 10l1 5M13 6l1 5",
  food: "M3 11h18M4 10c0-9 16-9 16 0M2 14h20M4 18h16l-1 3H5l-1-3M8 7h.01M13 5h.01M16 8h.01",
  travel: "m21 3-7 8 4 7-2 2-6-6-5 4-2-2 4-5-6-6 2-2 7 4 8-7Z",
  activities: "M4 20C-2 5 10-2 20 4c6 15-6 22-16 16ZM6 18 18 6M9 10l5 5M12 7l5 5M6 13l5 5",
  objects: "M9 18h6M9 22h6M8 15c-8-8 0-17 7-12 5 3 4 8 1 12l-1 3H9Z",
  symbols: "M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0M12 3v18M12 12 5 18M12 12l7 6",
  flags: "M5 22V3c5-4 9 5 15 0v12c-6 5-10-4-15 0",
  custom: "M5 3v18M12 3v18M19 3v18M3 7h18M3 15h18",
};
export default function EmojiCategoryIcon(props: { name: string }) {
  return (
    <svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
      <path d={paths[props.name] ?? paths.search} />
    </svg>
  );
}
