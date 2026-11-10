// The narrow strip left of the sidebar: workspace, the Home view and you.
import { createSignal, Show } from "solid-js";
import YouMenu from "./YouMenu";
import { presenceOf } from "./presence-store";
import Avatar from "./Avatar";
import { Home } from "./icons";

export default function Rail(props: { team: string; userId: string; user: string }) {
  const [menu, setMenu] = createSignal<DOMRect | null>(null);
  const initials = () =>
    props.team
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((w) => w[0]!.toUpperCase())
      .join("");
  return (
    <nav class="rail" aria-label="Workspace">
      <div class="rail-team" title={props.team}>
        {initials()}
      </div>
      <div class="rail-item active">
        <span class="rail-tile">
          <Home size={20} />
        </span>
        <span>Home</span>
      </div>
      <button
        type="button"
        class="rail-me"
        aria-label="You"
        aria-haspopup="menu"
        aria-expanded={!!menu()}
        title={props.user}
        onClick={(e) => setMenu(menu() ? null : e.currentTarget.getBoundingClientRect())}
      >
        <Avatar id={props.userId} name={props.user} size={36} presence={presenceOf(props.userId)} />
      </button>
      <Show when={menu()}>
        {(anchor) => (
          <YouMenu
            userId={props.userId}
            user={props.user}
            anchor={anchor()}
            onClose={() => setMenu(null)}
          />
        )}
      </Show>
    </nav>
  );
}
