// Who is who, for `<@U…>` mentions and avatars. Loaded once per session and
// refreshed when the directory changes (including channels and custom emoji).
import { createMemo, createSignal } from "solid-js";
import { api, type ChannelRow } from "../api";
import { loadCustomEmoji } from "../components/EmojiView";

const [people, setPeople] = createSignal<Record<string, string>>({});
const [avatars, setAvatars] = createSignal<Record<string, string>>({});

const [channels, setChannels] = createSignal<ChannelRow[]>([]);
export const channelNames = createMemo(() =>
  Object.fromEntries(channels().map((c) => [c.id, c.name])),
);
export { avatars, people, channels };

export async function loadPeople() {
  const [p, a, c] = await Promise.all([
    api.people(),
    api.avatars(),
    api
      .channelDirectory(false)
      .then((d) => d.channels)
      .catch(() => channels()),
    loadCustomEmoji(),
  ]);
  setChannels(c);
  setPeople(p);
  setAvatars(a);
}
