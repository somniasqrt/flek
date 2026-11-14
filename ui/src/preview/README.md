# Synthetic reading preview

Run `npm run dev` from `ui/`, then open `/preview.html`. This is the synthetic
IPC entry point; it does not launch Tauri, access Keychain or contact Slack.
`?lang=uk` switches to Ukrainian.

- `/preview.html?signin&lang=uk` shows a synthetic rejected session. **Clear saved
  session** asks for confirmation; Cancel keeps it, confirmation uses mocked
  sign-out and clears local drafts/outbox while preserving UI preferences.
  **Try the Slack desktop app again** imports a synthetic session.
- `/preview.html?settingsError&preferences&lang=uk` shows the settings-read
  warning in Preferences and refuses settings saves. In the console,
  `__settingsError(false)` clears the diagnostic through mocked settings/change
  events; `__settingsError(true)` restores it. Both flags also work with `lang=en`.

- The newest **general** messages include XLSX (three sheet tabs including an
  empty sheet), DOCX (heading, bold/italic/underline, list, table, embedded PNG),
  a truncated spreadsheet, a broken DOCX and an over-limit fallback. Try each
  card in the timeline and **Files**, plus the files in the existing thread.
  Test sheet Left/Right/Home/End keys, scrolling to row 1000, Esc, both languages
  and themes. The XLSX has a cached formula result, never formula evaluation;
  the Word hyperlink becomes inert text. The Budget sheet demonstrates narrow
  file column widths (F is hidden), a merged title at A4:H5, a hidden row 6,
  bordered tables with bold blue headers, theme/indexed colours, wrapped text at
  A7:C7 and text spilling from A9 up to H9. Saved row heights remain visible while
  scrolling; Notes demonstrates the default 64 px columns. Verify column letters
  and row numbers remain sticky in both scroll directions. The large fixture
  shares this layout and reaches row 1000 / column CV (100 columns) with the
  truncation notice. `window.__openedFile` records mocked
  “Open in default app” actions. Fixtures are generated locally in documents.ts;
  no fixture fetch or real IPC is needed. Native WebKit worker loading, layout
  and default-app launching remain owner QA.
- In **general**, the newest messages include all supported read-only Block Kit
  types, a two-second blue MP4, a video download card, a huddle and an unsupported
  attachment. Downloads update the synthetic store and emit a message change.
- The newest **general** messages also show a cached JPEG, an undownloaded JPEG
  with a thumbnail, two SVG previews and a JPEG that always fails to download. Click
  each in the timeline and **Files**; the existing thread includes the first
  four and a cached video too. The JPEG download card has no permalink. Downloads have a 600 ms mock delay, then return a fresh synthetic
  stored-file view. The failure shows localized copy and remains retryable.
  Expand the inline MP4 using its filename or expand button, then close with Esc,
  backdrop or the close button. Inline playback pauses when the viewer opens;
  closing releases the viewer player. SVG uses the same bounded-IPC contract as
  documents and only an `<img>` Blob URL; the OS action records a reveal request.
  FSVG has no Slack thumbnail: it loads generated SVG bytes through
  `document_data` with `inlinePreview=true` only near the viewport. FSVG_THUMB
  has a cached Slack PNG thumbnail and no cached original; its tile uses
  `/preview-svg-thumb.png` without a document IPC read, then downloads on click
  before opening the viewer. Both have reserved 320×240 dimensions, appear in
  threads and Files, and honor `image_previews`. Disable image previews to see
  cards/icons and verify neither SVG shows the false cannot-open notice. The
  viewer's OS action still reads “Show in Finder” on macOS. Toggle settings or
  unmount the tile during IPC to check cleanup. Native IPC enforces the inline
  1 MiB cap; this small synthetic fixture does not exercise that Rust limit.
  The preview mocks `download_file` and `reveal_download` too.
- Use Command+K (Control+K on Windows/Linux) to filter channel, DM or group names;
  full display names of group members also match. Arrows select, Enter opens,
  Escape closes. Link formatting uses Command+Shift+U / Control+Shift+U.
  Test Option+Arrow and Option+Shift+Arrow outside editable fields,
  including after collapsing a section. The selected section opens on navigation.
- **timeline-10000** pages normally. Open `/preview.html?stress=1`, then choose
  that channel to load all 10,000 synthetic records at once. Only the visible
  rows and overscan should exist in the DOM. Inspect `.virtual-row` count while
  scrolling and use the browser performance panel to check frame rate.
- Day labels and the header kebab menu’s **Jump to date…** open the date picker. Test a populated day, a gap,
  a date before the archive and a future date. Date windows page both directions.
  Check author grouping/day boundaries, resizes, media layout changes and scroll
  anchoring on prepends; `__reply()` simulates a live thread update.
- Search results and thread roots still reveal their timeline message. Real
  opening via the OS is mocked: message/header/fallback actions write the URL to
  `window.__openedUrl`; they do not open Slack.

The checked-in `ui/public/preview-image.jpg` is a generated 32×24 solid blue JPEG
(about 650 bytes). It is used for FJPEG and FJPEG_DOWNLOAD; the cached thumbnail
uses the same tiny JPEG. The SVG is generated in assets.ts. `preview-svg-thumb.png` is a generated
320×240 PNG matching its blue background and white circle.

The checked-in `ui/public/preview-video.mp4` is generated from a two-second,
160x90 blue color source (H.264, no audio, no external content). The production
entry point does not import the preview modules; Vite copies the tiny public
fixture to build output.

Native WebKit video decoding, byte-range seeking over `flekfile`, OS deep-link
launching and sustained 60 fps require owner verification. Inline videos whose
codec is unsupported fall back to the file card; opening that card shows the
viewer with its localized failure and default-app action. To bound responder memory,
a request without a Range header for a video over 8 MiB also falls back to the
card; ranged responses are at most 8 MiB. Protocol containment, extension/MIME
checks, quarantine/download limits and active-document rejection remain in force.

## Downloads, object menus and row feedback (M3.5.10)

Use the same cached/uncached JPEG, both SVGs, inline MP4, document cards and thread
fixtures. Hover/focus a tile for its top-right Download button, use the viewer
header or Files Download, then choose the platform-specific reveal action on the
“Saved to Downloads” notice. No real file is written. `window.__downloadCalls`
records requested ids, `__savedDownload` records `{fileId, token}`, and
`__revealedDownload` records the id revealed by a valid receipt. The existing
`__openedFile` and `__openedUrl` still record OS/default-app/deep-link actions.
`__downloadFailure = "directory"` simulates a missing Downloads folder; `"save"`
simulates an unwritable/full folder. Set it to `undefined` to restore success.
FJPEG_FAIL exercises fetch failure, and hidden files have no download control.

Right-click file tiles, inline video, Files rows and the viewer stage; test
Shift+F10/ContextMenu on their focused controls/stage, arrow keys, Home/End,
Enter, Esc, outside click and window resize. Copy link only appears for stored
file permalinks (FJPEG_DOWNLOAD has none). Open in Slack needs message context.
Right-click message links and unfurl cards/images for Open link/Copy link.
Unselected plain areas must never show the browser Reload/Inspect menu. The
composer, other editable fields, and pointer hits on selected text keep native
editing/Copy. An unrelated selection elsewhere must not enable a native menu.
Message hover/focus-within should tint the row in both themes while active,
jump-flash and outbox states retain precedence. Resize the timeline to a narrow
pane with Ukrainian dates: both normal/sticky date pills remain centered and
ellipsize. `?platform=windows` / `?platform=linux` exercises OS wording only;
native Downloads resolution, disk errors/quarantine and reveal need native QA.

## Conversation header (M3.7.4)

- **general** is starred, has three cached members (two images and initials), and
  cannot be left. **course-project**, under collapsed **Muted**, is private and
  muted. **timeline-10000** has no cached membership, so its header omits the count
  and avatars. **Olena Koval** is a DM; **Olena, Taras** is a group DM.
- Click the star and bell choices to exercise the normal mocked directory-change
  flow. Notifications show **In Flek only** and the muted bell changes immediately
  after the store refresh. Sidebar right-click retains its notification submenu.
- Name and member buttons open cached channel details; DM names open a profile.
  The magnifier focuses global search with the channel/DM `in:` filter and a
  trailing space. Kebab opens a right-aligned menu; Copy opens its submenu.
- Check light/dark themes, English/Ukrainian, long names and narrow panes, keyboard
  arrows/Home/End/Escape, focus return, menu clamping and avatar loading without
  layout shift. Header/menu membership reads never request fresh data.
