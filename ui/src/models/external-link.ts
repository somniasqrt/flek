// No href: native menus, modifier clicks and drags cannot navigate the webview.
// The Rust open_url command remains the authoritative scheme allowlist.
export function externalLink(url: string | null, open: (url: string) => void) {
  const prevent = (event: { preventDefault(): void }) => event.preventDefault();
  const activate = (event: { preventDefault(): void }) => {
    event.preventDefault();
    if (url) open(url);
  };
  return {
    role: "link" as const,
    tabIndex: url ? 0 : -1,
    title: url ?? undefined,
    draggable: false,
    "aria-disabled": !url,
    onClick: activate,
    onAuxClick: (event: MouseEvent) => {
      event.preventDefault();
      if (event.button === 1 && url) open(url);
    },
    onKeyDown: (event: KeyboardEvent) => {
      if (event.key === "Enter" && !event.repeat) activate(event);
    },
    "data-external-url": url ?? undefined,
    onDragStart: prevent,
  };
}
