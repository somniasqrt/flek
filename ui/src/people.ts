// Who is who, for `<@U…>` mentions and avatars. Loaded once per session and
// refreshed when the directory changes.
import { createSignal } from "solid-js";
import { api } from "./api";

const [people, setPeople] = createSignal<Record<string, string>>({});

export { people };

export async function loadPeople() {
  setPeople(await api.people());
}
