// Who is who, for `<@U…>` mentions and avatars. Loaded once per session and
// refreshed when the directory changes (avatars arrive with it).
import { createSignal } from "solid-js";
import { api } from "./api";

const [people, setPeople] = createSignal<Record<string, string>>({});
const [avatars, setAvatars] = createSignal<Record<string, string>>({});

export { avatars, people };

export async function loadPeople() {
  const [p, a] = await Promise.all([api.people(), api.avatars()]);
  setPeople(p);
  setAvatars(a);
}
