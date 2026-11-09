// The narrow strip left of the sidebar: workspace, the Home view and you.
import Avatar from "./Avatar";
import { Home } from "./icons";

export default function Rail(props: { team: string; userId: string; user: string }) {
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
      <div class="rail-me" title={props.user}>
        <Avatar id={props.userId} name={props.user} size={36} />
      </div>
    </nav>
  );
}
