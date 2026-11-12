# Synthetic reading preview

Run `npm run dev` from `ui/`, then open `/preview.html`. This is the synthetic
IPC entry point; it does not launch Tauri, access Keychain or contact Slack.
`?lang=uk` switches to Ukrainian.

- The newest **general** messages include XLSX (three sheet tabs including an
  empty sheet), DOCX (heading, bold/italic/underline, list, table, embedded PNG),
  a truncated spreadsheet, a broken DOCX and an over-limit fallback. Try each
  card in the timeline and **Files**, plus the files in the existing thread.
  Test sheet Left/Right/Home/End keys, scrolling to row 1000, Esc, both languages
  and themes. The XLSX has a cached formula result, never formula evaluation;
  the Word hyperlink becomes inert text. `window.__openedFile` records mocked
  “Open in default app” actions. Fixtures are generated locally in documents.ts;
  no fixture fetch or real IPC is needed. Native WebKit worker loading, layout
  and default-app launching remain owner QA.
- In **general**, the newest messages include all supported read-only Block Kit
  types, a two-second blue MP4, a video download card, a huddle and an unsupported
  attachment. Downloads update the synthetic store and emit a message change.
- Use Command+K (Control+K on Windows/Linux) to filter channel, DM or group names;
  full display names of group members also match. Arrows select, Enter opens,
  Escape closes. Link formatting uses Command+Shift+U / Control+Shift+U.
  Test Option+Arrow and Option+Shift+Arrow outside editable fields,
  including after collapsing a section. The selected section opens on navigation.
- **timeline-10000** pages normally. Open `/preview.html?stress=1`, then choose
  that channel to load all 10,000 synthetic records at once. Only the visible
  rows and overscan should exist in the DOM. Inspect `.virtual-row` count while
  scrolling and use the browser performance panel to check frame rate.
- Day labels and the header open the date picker. Test a populated day, a gap,
  a date before the archive and a future date. Date windows page both directions.
  Check author grouping/day boundaries, resizes, media layout changes and scroll
  anchoring on prepends; `__reply()` simulates a live thread update.
- Search results and thread roots still reveal their timeline message. Real
  opening via the OS is mocked: message/header/fallback actions write the URL to
  `window.__openedUrl`; they do not open Slack.

The checked-in `ui/public/preview-video.mp4` is generated from a two-second,
160x90 blue color source (H.264, no audio, no external content). The production
entry point does not import the preview modules; Vite copies the tiny public
fixture to build output.

Native WebKit video decoding, byte-range seeking over `flekfile`, OS deep-link
launching and sustained 60 fps require owner verification. Cached videos whose
codec is unsupported fall back to the file card. To bound responder memory,
a request without a Range header for a video over 8 MiB also falls back to the
card; ranged responses are at most 8 MiB. Protocol containment, extension/MIME
checks, quarantine/download limits and active-document rejection remain in force.
