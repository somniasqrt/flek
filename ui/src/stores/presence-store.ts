// Interest is owned by Workspace. Every refresh below reads SQLite over IPC.
import { createSignal } from "solid-js";
import { api } from "../api";
import { indicator, type CustomStatus, type Presence } from "../models/presence";

const [rows, setRows] = createSignal<Record<string, Presence>>({});
const [statuses, setStatuses] = createSignal<Record<string, CustomStatus>>({});
const [now, setNow] = createSignal(Math.floor(Date.now() / 1000));
const [revision, setRevision] = createSignal(0);
let ids: string[] = [];
let generation = 0;
export { rows as presenceRows, statuses, now as presenceNow, revision as profileRevision };
export const tickPresence = () => setNow(Math.floor(Date.now() / 1000));
export const presenceOf = (id: string | null) => indicator(id ? rows()[id] : undefined, now());
export const directoryChanged = () => setRevision((n) => n + 1);

export async function watchPresence(users: string[]) {
  ids = [...new Set(users)].sort();
  generation++;
  await api.subscribePresence(ids);
  await refreshPresence();
}

export async function refreshPresence() {
  const gen = ++generation;
  const [p, s] = await Promise.all([api.presenceFor(ids), api.statusesFor(ids)]);
  if (gen !== generation) return;
  setRows(Object.fromEntries(p.map((x) => [x.user_id, x])));
  setStatuses(s);
}
