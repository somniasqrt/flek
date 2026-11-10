import type { Conversation, FileRef, Message } from "../api";

export const people: Record<string, string> = {
  U1: "Olena Koval",
  U2: "Taras Melnyk",
  U3: "Me",
  U4: "Iryna Bondar",
  U5: "Andrii Shevchuk",
};
export const conversations: Conversation[] = [
  {
    id: "C1",
    kind: "public_channel",
    label: "general",
    is_archived: false,
    topic: "Announcements",
    messages: 6,
    unread: 2,
    latest_ts: "1762600000.000000",
    dm_user_id: null,
  },
  {
    id: "C2",
    kind: "private_channel",
    label: "course-project",
    is_archived: false,
    topic: null,
    messages: 0,
    unread: 0,
    latest_ts: null,
    dm_user_id: null,
  },
  {
    id: "D1",
    kind: "im",
    label: "Olena Koval",
    is_archived: false,
    topic: null,
    messages: 1,
    unread: 1,
    latest_ts: "1762600500.000000",
    dm_user_id: "U1",
  },
];
conversations.push(
  { ...conversations[2], id: "D2", label: people.U2, dm_user_id: "U2", unread: 0 },
  { ...conversations[2], id: "D4", label: people.U4, dm_user_id: "U4", unread: 0 },
);
conversations.push(
  { ...conversations[2], id: "D3", label: "Me", dm_user_id: "U3", unread: 0 },
  {
    ...conversations[2],
    id: "G1",
    kind: "mpim",
    label: "Olena, Taras",
    dm_user_id: null,
    unread: 2,
  },
);
export const file = (f: Partial<FileRef>): FileRef => ({
  id: "F",
  name: null,
  title: null,
  mimetype: null,
  filetype: null,
  size: null,
  can_open: false,
  local_path: null,
  permalink: "https://example.com/f",
  hidden_by_limit: false,
  width: null,
  height: null,
  preview_path: null,
  preview_width: null,
  preview_height: null,
  ...f,
});
export const msg = (ts: number, user: string, text: string, extra: Partial<Message> = {}): Message => ({
  ts: `${ts}.000000`,
  user_id: user,
  author: people[user],
  text,
  subtype: null,
  reply_count: 0,
  repliers: [],
  latest_reply: null,
  edited: false,
  hidden_by_limit: false,
  reactions: null,
  thread_ts: null,
  files: [],
  unfurls: [],
  ...extra,
});
// The thread under 1762300120: its last reply a few minutes ago.
export const now = Math.floor(Date.now() / 1000);
export const ROOT = "1762300120.000000";
export const replies: Message[] = [
  msg(1762300400, "U5", "Will it be the same link as last week?", { thread_ts: ROOT }),
  msg(
    1762300700,
    "U1",
    "Yes, same Zoom room. I'll also post the recording in #general afterwards :+1:",
    { thread_ts: ROOT, reactions: ":pray: 2" },
  ),
  msg(
    now - 25 * 60,
    "U4",
    "Thanks! Could someone share the slides from the previous lecture too?",
    { thread_ts: ROOT },
  ),
];
export const messages: Message[] = [
  msg(
    1762300000,
    "U1",
    "# Practice moved online\nHi all :wave: tomorrow's practice is *online*: <https://us06web.zoom.us/j/123456|Join Zoom>\nSlides: <https://docs.google.com/presentation/d/abc/edit>",
    { reactions: ":+1: 4  :fire: 2" },
  ),
  msg(
    1762300060,
    "U1",
    "Plan:\n- predicates recap\n- test, _half_ the class\n  - variants at 3 times\n1. read chapter 2\n2. try `forall x. P(x)`",
  ),
  msg(
    1762300120,
    "U2",
    "&gt; Will the recording be on the drive?\nYes, <@U1> said so <!here>. See https://github.com/example/repo and :pink_heart:",
    {
      edited: true,
      reply_count: 3,
      repliers: ["U4", "U1", "U5"],
      latest_reply: `${now - 25 * 60}.000000`,
    },
  ),
  msg(1762400000, "U3", '```fn main() {\n    println!("hi");\n}```'),
  msg(1762400030, "U3", "", {
    files: [
      file({
        id: "F1",
        name: "calendar.png",
        mimetype: "image/png",
        can_open: true,
        local_path: "/tmp/x.png",
        width: 1200,
        height: 800,
      }),
      file({
        id: "F2",
        name: "Schedule_Monday stream 70236.pdf",
        mimetype: "application/pdf",
        filetype: "pdf",
        size: 2_400_000,
        can_open: true,
        local_path: "/tmp/l.pdf",
        preview_path: "/tmp/p.png",
        preview_width: 910,
        preview_height: 1286,
      }),
    ],
  }),
  msg(1762400090, "U1", "Variants for the test", {
    files: [
      file({
        id: "F4",
        name: "Variants 9 points.pdf",
        mimetype: "application/pdf",
        filetype: "pdf",
        size: 300_000,
        can_open: true,
        local_path: "/tmp/v.pdf",
        preview_path: "/tmp/v.png",
        preview_width: 910,
        preview_height: 1286,
      }),
    ],
  }),
  msg(1762400120, "U1", "", {
    files: [
      file({
        id: "F5",
        name: "KM-53 11-30.pdf",
        mimetype: "application/pdf",
        filetype: "pdf",
        can_open: true,
        local_path: "/tmp/a.pdf",
        preview_path: "/tmp/a.png",
      }),
      file({
        id: "F6",
        name: "KM-51 11-30.pdf",
        mimetype: "application/pdf",
        filetype: "pdf",
        can_open: true,
        local_path: "/tmp/b.pdf",
        preview_path: "/tmp/b.png",
      }),
    ],
  }),
  msg(
    1762450000,
    "U1",
    "Lecture recording: https://www.youtube.com/watch?v=abc and the plan https://example.org/plan",
    {
      unfurls: [
        {
          service: "YouTube",
          title: "Lecture 5: predicates",
          title_link: "https://www.youtube.com/watch?v=abc",
          text: null,
          author: null,
          footer: null,
          url: "https://www.youtube.com/watch?v=abc",
          color: null,
          image: "yt",
          image_width: 480,
          image_height: 360,
          thumb: false,
          icon: "ic",
          video: true,
          fields: [],
        },
        {
          service: "Example Org",
          title: "Course plan 2026",
          title_link: "https://example.org/plan",
          text: "Topics, deadlines and *grading* for the autumn term.",
          author: null,
          footer: null,
          url: "https://example.org/plan",
          color: "#36a64f",
          image: "th",
          image_width: 80,
          image_height: 80,
          thumb: true,
          icon: null,
          video: false,
          fields: [],
        },
      ],
    },
  ),
  msg(1762450050, "U1", "Facebook preview regression fixture", {
    unfurls: [
      {
        service: "Facebook",
        title: "&#127757; ML WEEK 2025 &ndash; &#1050;&#1055;&#1030;",
        title_link: "https://example.com/facebook-preview",
        text: "&#127757; ML WEEK 2025 &ndash; &#1091;&#1085;&#1110;&#1082;&#1072;&#1083;&#1100;&#1085;&#1080;&#1081; &#1087;&#1088;&#1086;&#1108;&#1082;&#1090;\n\n&laquo;&#1050;&#1055;&#1030;&raquo; &amp; *&#1053;&#1058;&#1059;* — &#x1F4BB;",
        author: null,
        footer: "&#169; Facebook &amp; example",
        url: "https://example.com/facebook-preview",
        color: null,
        image: null,
        image_width: null,
        image_height: null,
        thumb: false,
        icon: null,
        video: false,
        fields: [],
      },
    ],
  }),
  msg(
    1762450100,
    "U5",
    "Shipping it :shipit: :yes::skin-tone-4: and a :custom_party: with :not_cached:",
    { reactions: ":custom_party: 3  :+1::skin-tone-3: 1  :gdocs: 2  :zoom: 1" },
  ),
  msg(1762500000, "U2", "Old one ~gone~ but kept :custom_party:", {
    hidden_by_limit: true,
    files: [
      file({ id: "F3", name: "notes.docx", filetype: "docx", size: 48_000, hidden_by_limit: true }),
    ],
  }),
];

// Synthetic security fixture: no executable contents or real cache path.
messages.push(msg(1762500050, "U2", "File safety preview", {
  files: [file({ id: "FSECURITY", name: "x.terminal", title: "Розклад.pdf", mimetype: "application/pdf", local_path: "/synthetic/x.terminal", can_open: false })],
}));

// Real-shaped received blocks paired with their mrkdwn fallback for regression QA.
const receivedText =
  "<https://us06web.zoom.us/j/123456|Join Zoom> https://example.org/plan <https://example.org/label|Read this> :wave::skin-tone-4: <@U1> *bold underline*";
messages.push(
  msg(1762500100, "U2", receivedText, {
    blocks: [
      {
        type: "rich_text",
        block_id: "received-fixture",
        elements: [
          {
            type: "rich_text_section",
            elements: [
              { type: "link", url: "https://us06web.zoom.us/j/123456", text: "Join Zoom" },
              { type: "text", text: " " },
              { type: "link", url: "https://example.org/plan", text: "https://example.org/plan" },
              { type: "text", text: " " },
              { type: "link", url: "https://example.org/label", text: "Read this" },
              { type: "text", text: " " },
              { type: "emoji", name: "wave", skin_tone: 4 },
              { type: "text", text: " " },
              { type: "user", user_id: "U1" },
              { type: "text", text: " " },
              { type: "text", text: "bold underline", style: { bold: true, underline: true } },
            ],
          },
        ],
      },
    ],
  }),
  msg(1762500110, "U1", receivedText),
  msg(1762500120, "U2", "Unsupported element fallback <@U1>", {
    blocks: [
      {
        type: "rich_text",
        elements: [
          { type: "rich_text_section", elements: [{ type: "channel", channel_id: "C1" }] },
        ],
      },
    ],
  }),
);

