import type { Hit, Profile, Message, DirectMessage, ActivityItem, StoredFile } from "../api";
import type { Presence } from "../models/presence";
import { MARK_CLOSE as C, MARK_OPEN as O } from "../models/snippet";
import { people, conversations, msg, now, ROOT, replies, messages, bigMessages } from "./messages";

// Search: a few fake hits, matched terms marked like core does.
const hit = (
  conversation_id: string,
  conversation: string,
  ts: number,
  user: string,
  snippet: string,
  extra: Partial<Hit> = {},
): Hit => ({
  conversation_id,
  conversation,
  ts: `${ts}.000000`,
  thread_ts: null,
  user_id: user,
  author: people[user],
  snippet,
  hidden_by_limit: false,
  ...extra,
});
export const hits: Hit[] = [
  hit("CBIG", "timeline-10000", Number(bigMessages[1234].ts), "U2", `A ${O}practice${C} jump into the virtualized channel`, { ts: bigMessages[1234].ts }),
  hit(
    "C1",
    "general",
    1762300000,
    "U1",
    `Hi all :wave: tomorrow's ${O}practice${C} is *online*: <https://us06web.zoom.us/j/123456|Join Zoom>`,
  ),
  hit(
    "C1",
    "general",
    1762300060,
    "U1",
    `Plan: - predicates recap - test, _half_ the class … ${O}practice${C} variants at 3 times`,
  ),
  hit(
    "D1",
    "Olena Koval",
    1762600500,
    "U1",
    `Чи буде запис ${O}практики${C} на диску? <@U2> казав, що так`,
  ),
  hit(
    "C2",
    "course-project",
    1755000000,
    "U2",
    `Старий звіт з ${O}практики${C}, Slack його вже ховає`,
    { hidden_by_limit: true },
  ),
  hit("C1", "general", 1762300200, "U2", `Reply in a thread about the ${O}practice${C} room`, {
    thread_ts: "1762300120.000000",
  }),
];

// Profiles: made-up people (fake email), one with a DM and one without.
const profile = (id: string, extra: Partial<Profile>): Profile => ({
  id,
  name: people[id].toLowerCase().replace(" ", "."),
  real_name: people[id],
  display_name: null,
  title: null,
  email: null,
  phone: null,
  pronouns: null,
  status_text: null,
  status_emoji: null,
  status_expiration: 0,
  tz: null,
  tz_label: null,
  tz_offset: null,
  is_bot: false,
  deleted: false,
  avatar: null,
  photo: null,
  has_photo: false,
  dm_conversation_id: null,
  ...extra,
});
export const profiles: Record<string, Profile> = {
  U1: profile("U1", {
    title: "Доцент кафедри математичної логіки",
    email: "olena.koval@example.edu",
    phone: "+380 00 000 0000",
    tz: "Europe/Kyiv",
    tz_label: "Eastern European Time",
    tz_offset: 7200,
    avatar: "a1",
    has_photo: true,
    dm_conversation_id: "D1",
    status_text: "On a lecture",
    status_emoji: ":books:",
  }),
  U3: profile("U3", { display_name: "Me", dm_conversation_id: null }),
  U2: profile("U2", {
    title: "професор",
    tz: "America/New_York",
    tz_label: "Eastern Standard Time",
    tz_offset: -18000,
  }),
  U4: profile("U4", { display_name: "iryna", pronouns: "she/her", tz_offset: 3600, avatar: "a4" }),
};

// Messages "sent" from the preview into conversations other than C1.
export const elsewhere: (Message & { conversation_id: string })[] = [
  { ...msg(now - 600, "U1", "Hi <@U3>, have you seen the updated *plan*?"), conversation_id: "D1" },
  { ...msg(now - 1200, "U3", "Notes for tomorrow"), conversation_id: "D3" },
  { ...msg(now - 1500, "U2", "We can review the slides here."), conversation_id: "G1" },
];
export const presence: Record<string, Presence> = Object.fromEntries(
  ["U1", "U2", "U3", "U4"].map((user_id) => [
    user_id,
    {
      user_id,
      presence: user_id === "U4" ? "away" : "active",
      dnd_enabled: false,
      dnd_start: 0,
      dnd_end: 0,
      snooze_enabled: user_id === "U2",
      snooze_end: user_id === "U2" ? now + 7200 : 0,
      manual_away: false,
      auto_away: false,
      online: user_id !== "U4",
      connection_count: 1,
      presence_fresh: true,
      dnd_fresh: true,
      updated_at: now,
    },
  ]),
);

messages.push(
  msg(now - 1800, "U3", "I have shared the notes", { reactions: ":eyes: 1" }),
  msg(now - 86400, "U2", "<@U3> could you take a look?"),
);
messages.sort((a, b) => a.ts.localeCompare(b.ts));
conversations[0].last_read = messages.at(-3)?.ts ?? null;
conversations[0].latest_ts = messages.at(-1)?.ts ?? null;
conversations[0].starred = true;
conversations[0].is_general = true;
conversations[1].notification = "muted";
conversations[1].topic = "Muted in Flek — expand the Muted group";
for (const c of conversations) {
  c.notification ??= "all";
  c.starred ??= false;
}

export const closed = new Set<string>();
export const directRows = (): DirectMessage[] =>
  conversations
    .filter((c) => !closed.has(c.id) && (c.kind === "im" || c.kind === "mpim"))
    .map((c) => {
      const m = elsewhere.filter((m) => m.conversation_id === c.id).at(-1);
      return {
        id: c.id,
        label: c.label,
        kind: c.kind,
        partner_id: c.dm_user_id,
        members: c.kind === "mpim" ? ["U1", "U2"] : [],
        unread: c.notification === "muted" ? 0 : c.unread,
        ts: m?.ts ?? null,
        user_id: m?.user_id ?? null,
        text: m?.text ?? "",
        file_name: null,
        hidden_by_limit: false,
      };
    })
    .sort((a, b) => (b.ts ?? "").localeCompare(a.ts ?? ""));
export const activityRows: ActivityItem[] = [
  {
    kind: "dm",
    conversation_id: "D1",
    ts: `${now - 600}.000000`,
    event_ts: `${now - 600}.000000`,
    thread_ts: null,
    user_id: "U1",
    text: "Hi <@U3>, can we review the plan?",
    file_name: null,
    hidden_by_limit: false,
    reaction: null,
    unread: true,
    unread_count: 1,
  },
  {
    kind: "reaction",
    conversation_id: "C1",
    ts: `${now - 1800}.000000`,
    event_ts: `${now - 1200}.000000`,
    thread_ts: null,
    user_id: "U2",
    text: "I have shared the notes",
    file_name: null,
    hidden_by_limit: false,
    reaction: "eyes",
    unread: true,
    unread_count: 1,
  },
  {
    kind: "thread",
    conversation_id: "C1",
    ts: replies[2].ts,
    event_ts: replies[2].ts,
    thread_ts: ROOT,
    user_id: "U4",
    text: replies[2].text,
    file_name: null,
    hidden_by_limit: false,
    reaction: null,
    unread: true,
    unread_count: 1,
  },
  {
    kind: "mention",
    conversation_id: "C1",
    ts: `${now - 86400}.000000`,
    event_ts: `${now - 86400}.000000`,
    thread_ts: null,
    user_id: "U2",
    text: "<@U3> could you take a look?",
    file_name: null,
    hidden_by_limit: false,
    reaction: null,
    unread: false,
    unread_count: 0,
  },
];
export const storedFiles = (): StoredFile[] =>
  messages.flatMap((m) =>
    m.files.map((f) => ({
      id: f.id,
      title: f.title,
      name: f.name,
      mimetype: f.mimetype,
      size: f.size,
      can_open: f.can_open,
      viewer_kind: f.viewer_kind,
      local_path: f.local_path,
      conversation_id: "C1",
      ts: m.ts,
      user_id: m.user_id,
      hidden_by_limit: f.hidden_by_limit,
      permalink: f.permalink,
    })),
  );

