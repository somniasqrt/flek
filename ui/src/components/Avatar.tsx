import { Show } from "solid-js";
import { linkImageUrl } from "../api";
import { avatars } from "../stores/people";
import PresenceIndicator from "./PresenceIndicator";
import type { Indicator } from "../models/presence";

// A person's picture from the cache, or their initials on a colour picked
// from the user id (stable per person) until it is downloaded.

export default function Avatar(props: {
  id: string | null;
  name: string;
  size?: number;
  presence?: Indicator;
  photo?: string | null;
}) {
  const initials = () =>
    props.name
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((w) => w[0]!.toUpperCase())
      .join("");
  // FNV-1a: ids that differ in one character still land far apart.
  const hue = () => {
    let h = 0x811c9dc5;
    for (const c of props.id ?? props.name) h = Math.imul(h ^ c.charCodeAt(0), 0x01000193);
    return (h >>> 0) % 360;
  };
  const image = () => props.photo ?? (props.id ? avatars()[props.id] : undefined);
  const px = () => props.size ?? 36;
  // Rounded square like Slack's, scaled with the size.
  const box = () => ({
    width: `${px()}px`,
    height: `${px()}px`,
    "border-radius": `${Math.round(px() * 0.22)}px`,
    "font-size": `${Math.round(px() * 0.36)}px`,
  });
  return (
    <span
      class="avatar-wrap"
      style={{ "--presence-size": `${Math.max(8, Math.min(22, px() * 0.3))}px` }}
    >
      <Show
        when={image()}
        fallback={
          <div class="avatar" style={{ "--hue": hue(), ...box() }}>
            {initials()}
          </div>
        }
      >
        {(hash) => <img class="avatar" src={linkImageUrl(hash())} alt="" style={box()} />}
      </Show>
      <Show when={props.presence}>{(p) => <PresenceIndicator presence={p()} />}</Show>
    </span>
  );
}
