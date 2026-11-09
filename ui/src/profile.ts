// Whose profile the right-side panel shows. A global signal, so any name or
// avatar (timeline, thread panel) can open it without threading props.
import { createSignal } from "solid-js";

const [profileUser, setProfileUser] = createSignal<string | null>(null);

export { profileUser };

export const openProfile = (userId: string) => setProfileUser(userId);
export const closeProfile = () => setProfileUser(null);
