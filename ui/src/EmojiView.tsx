// One `:name:` emoji: a standard character, a workspace (custom) emoji image
// from the cache, or muted `:name:` text when neither is known (yet).
import { createSignal, type JSX } from "solid-js";
import { api, linkImageUrl, type CustomEmoji } from "./api";
import { emoji } from "./emoji";

const [custom, setCustom] = createSignal<CustomEmoji>({ images: {}, aliases: {} });

/// Loaded with the people (once per session, again on directory changes:
/// emoji images arrive with them).
export async function loadCustomEmoji() {
  // Not fatal: unknown emoji just stay `:name:` text.
  setCustom(await api.customEmoji().catch(() => custom()));
}

type Resolved = { char: string } | { image: string } | null;

/// `party_parrot`, `yes::skin-tone-3` (an alias of a standard emoji) → what to draw.
export function resolveEmoji(name: string): Resolved {
  const char = emoji(name);
  if (char) return { char };
  const [base, tone] = name.split("::");
  const { images, aliases } = custom();
  const image = images[base];
  if (image) return { image };
  const target = aliases[base];
  const aliased = target ? emoji(tone ? `${target}::${tone}` : target) : undefined;
  return aliased ? { char: aliased } : null;
}

/// `inText`: sized to the line and muted when unknown; otherwise a reaction chip.
export default function Emoji(props: { name: string; inText?: boolean }): JSX.Element {
  const label = () => `:${props.name}:`;
  // Re-resolves when the custom set loads or changes.
  const view = () => {
    const r = resolveEmoji(props.name);
    if (r && "image" in r) {
      return (
        <img
          class={props.inText ? "emoji-img" : "emoji-img reaction-img"}
          src={linkImageUrl(r.image)}
          alt={label()}
          title={label()}
          draggable={false}
        />
      );
    }
    if (r) {
      return (
        <span class="emoji" title={props.inText ? label() : undefined}>
          {r.char}
        </span>
      );
    }
    return <span class={props.inText ? "emoji custom" : "emoji"}>{label()}</span>;
  };
  return <>{view()}</>;
}
