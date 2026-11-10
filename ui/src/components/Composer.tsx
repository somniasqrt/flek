import { For, Show } from "solid-js";
import { t } from "../i18n/index";
import { emoji } from "../models/emoji";
import EmojiPicker from "./EmojiPicker";
import ComposeIcon from "./ComposeIcon";
import { escapeText } from "../models/compose";
import { safeLink } from "../models/rich-compose";
import {
shortcutLabel
} from "../models/shortcuts";
import { keyCombo, modKeyLabel } from "../platform";
import { people } from "../stores/people";

import { createComposer, savePreference, type ComposerProps } from "./composer/createComposer";

export default function Composer(props: ComposerProps) {
  const {
    mac, key, hasText, toolbar, modEnter, setModEnter, popover, setPopover, broadcast,
    setBroadcast, linkUrl, setLinkUrl, linkLabel, setLinkLabel, personQuery, setPersonQuery,
    remember, refresh, selectionChanged, focusSelection, exec, insert, toggleToolbar, action,
    applyLink, submit, onKeyDown, title, formatButton, mention, setBox, setEditor,
  } = createComposer(props);
  return (
    <div class="composer">
      <div
        class="composer-box"
        ref={setBox}
        onKeyDown={(e) => {
          if (e.key === "Escape" && popover()) {
            e.stopPropagation();
            setPopover(null);
            focusSelection();
          }
        }}
      >
        <Show when={toolbar()}>
          <div class="composer-format" role="toolbar" aria-label={t("Text formatting")}>
            {formatButton("bold", t("Bold"))}
            {formatButton("italic", t("Italic"))}
            {formatButton("underline", t("Underline"))}
            {formatButton("strike", t("Strikethrough"), "strikeThrough")}
            <button
              type="button"
              class="compose-tool compose-paragraph"
              title={t("Paragraph style")}
              aria-label={t("Paragraph style")}
              aria-expanded={popover() === "paragraph"}
              onPointerDown={(e) => e.preventDefault()}
              onClick={() => setPopover(popover() === "paragraph" ? null : "paragraph")}
            >
              <ComposeIcon name="paragraph" />
              <ComposeIcon name="chevron" />
            </button>
            <span class="composer-format-divider" />
            {formatButton("link", t("Insert link"))}
            {formatButton("ordered", t("Numbered list"), "insertOrderedList")}
            {formatButton("bullet", t("Bulleted list"), "insertUnorderedList")}
            <span class="composer-format-divider" />
            {formatButton("quote", t("Quote"))}
            {formatButton("code", t("Inline code"))}
            {formatButton("pre", t("Code block"))}
            {formatButton("clear", t("Clear formatting"))}
          </div>
        </Show>
        <Show when={popover() === "paragraph"}>
          <div
            class="compose-popover compose-paragraph-menu"
            role="dialog"
            aria-label={t("Paragraph style")}
          >
            <button
              type="button"
              onClick={() => {
                exec("formatBlock", "div");
                setPopover(null);
              }}
            >
              {t("Normal text")}
            </button>
            <button
              type="button"
              onClick={() => {
                action("quote");
                setPopover(null);
              }}
            >
              {t("Quote")}
              <kbd>{shortcutLabel("quote", mac)}</kbd>
            </button>
            <button
              type="button"
              onClick={() => {
                action("pre");
                setPopover(null);
              }}
            >
              {t("Code block")}
              <kbd>{shortcutLabel("pre", mac)}</kbd>
            </button>
          </div>
        </Show>
        <div
          ref={setEditor}
          class="composer-editor"
          contentEditable
          role="textbox"
          aria-multiline="true"
          aria-label={props.placeholder}
          data-placeholder={props.placeholder}
          data-empty={!hasText()}
          spellcheck
          onInput={(e) => {
            remember((e as InputEvent).inputType === "insertText");
            refresh();
            selectionChanged();
          }}
          onKeyDown={onKeyDown}
          onPaste={(e) => {
            e.preventDefault();
            insert(e.clipboardData?.getData("text/plain") ?? "");
          }}
          onDrop={(e) => {
            e.preventDefault();
          }}
        />
        <div class="composer-bar" role="toolbar" aria-label={t("Message actions")}>
          <span title={t("File uploads are planned")}>
            <button
              type="button"
              class="compose-tool compose-add"
              aria-label={t("Attach file (coming soon)")}
              disabled
            >
              <ComposeIcon name="plus" />
            </button>
          </span>
          <button
            type="button"
            class="compose-tool"
            title={title("toolbar", t("Toggle formatting"))}
            aria-label={t("Toggle formatting")}
            aria-pressed={toolbar()}
            onPointerDown={(e) => e.preventDefault()}
            onClick={toggleToolbar}
          >
            <ComposeIcon name="format" />
          </button>
          <button
            type="button"
            class="compose-tool"
            title={title("emoji", t("Insert emoji"))}
            aria-label={t("Insert emoji")}
            aria-expanded={popover() === "emoji"}
            onPointerDown={(e) => e.preventDefault()}
            onClick={() => action("emoji")}
          >
            <ComposeIcon name="emoji" />
          </button>
          <button
            type="button"
            class="compose-tool"
            title={t("Mention someone")}
            aria-label={t("Mention someone")}
            aria-expanded={popover() === "mention"}
            onPointerDown={(e) => e.preventDefault()}
            onClick={() => {
              setPersonQuery("");
              setPopover(popover() === "mention" ? null : "mention");
            }}
          >
            <ComposeIcon name="mention" />
          </button>
          <span class="composer-format-divider" />
          <span title={t("Video clips are not available yet")}>
            <button
              type="button"
              class="compose-tool compose-future"
              aria-label={t("Record video (coming soon)")}
              disabled
            >
              <ComposeIcon name="video" />
            </button>
          </span>
          <span title={t("Audio clips are not available yet")}>
            <button
              type="button"
              class="compose-tool compose-future"
              aria-label={t("Record audio (coming soon)")}
              disabled
            >
              <ComposeIcon name="mic" />
            </button>
          </span>
          <span class="composer-format-divider compose-future" />
          <span title={t("Canvas is not supported")}>
            <button
              type="button"
              class="compose-tool compose-future"
              aria-label={t("Create canvas (unavailable)")}
              disabled
            >
              <ComposeIcon name="canvas" />
            </button>
          </span>
          <div class="composer-spacer" />
          <div class="composer-send-group" classList={{ ready: hasText() }}>
            <button
              type="button"
              class="composer-send"
              title={t("Send ({key})", {
                key: modEnter() ? keyCombo([modKeyLabel(), "Enter"]) : "Enter",
              })}
              aria-label={t("Send")}
              disabled={!hasText()}
              onClick={submit}
            >
              <ComposeIcon name="send" />
            </button>
            <button
              type="button"
              class="compose-send-options"
              title={t("Send button options")}
              aria-label={t("Send button options")}
              aria-expanded={popover() === "send"}
              onClick={() => setPopover(popover() === "send" ? null : "send")}
            >
              <ComposeIcon name="chevron" />
            </button>
          </div>
        </div>
        <Show when={props.broadcastLabel}>
          <label class="composer-broadcast">
            <input
              type="checkbox"
              checked={broadcast()}
              onChange={(e) => setBroadcast(e.currentTarget.checked)}
            />
            {props.broadcastLabel}
          </label>
        </Show>
        <Show when={popover() === "emoji"}>
          <div class="compose-popover compose-emoji-popover">
            <EmojiPicker
              onPick={(name) => {
                exec(
                  "insertHTML",
                  `<span contenteditable="false" data-emoji-name="${name.replace(/[^a-z0-9_+:.\-]/gi, "")}">${escapeText(emoji(name) ?? `:${name}:`)}</span> `,
                );
                setPopover(null);
              }}
            />
          </div>
        </Show>
        <Show when={popover() === "mention"}>
          <div
            class="compose-popover compose-mention-popover"
            role="dialog"
            aria-label={t("Mention someone")}
          >
            <input
              ref={(el) => queueMicrotask(() => el.focus())}
              type="search"
              aria-label={t("Find a person")}
              placeholder={t("Find a person…")}
              value={personQuery()}
              onInput={(e) => setPersonQuery(e.currentTarget.value)}
            />
            <div class="compose-people">
              <For
                each={Object.entries(people())
                  .filter(([, n]) => n.toLowerCase().includes(personQuery().toLowerCase()))
                  .slice(0, 30)}
              >
                {([id, name]) => (
                  <button type="button" onClick={() => mention(id, name)}>
                    ＠ {name}
                  </button>
                )}
              </For>
            </div>
          </div>
        </Show>
        <Show when={popover() === "link"}>
          <form
            class="compose-popover compose-link-popover"
            onSubmit={applyLink}
            aria-label={t("Insert link")}
          >
            <strong>{t("Insert link")}</strong>
            <label>
              {t("Text")}
              <input value={linkLabel()} onInput={(e) => setLinkLabel(e.currentTarget.value)} />
            </label>
            <label>
              URL
              <input
                ref={(el) => queueMicrotask(() => el.focus())}
                type="url"
                placeholder={t("https://example.com")}
                value={linkUrl()}
                onInput={(e) => setLinkUrl(e.currentTarget.value)}
                required
              />
            </label>
            <div>
              <button
                type="button"
                onClick={() => {
                  setPopover(null);
                  focusSelection();
                }}
              >
                {t("Cancel")}
              </button>
              <button type="submit" disabled={!safeLink(linkUrl().trim())}>
                {t("Apply")}
              </button>
            </div>
          </form>
        </Show>
        <Show when={popover() === "send"}>
          <div
            class="compose-popover compose-send-popover"
            role="dialog"
            aria-label={t("Send preferences")}
          >
            <strong>{t("Press Enter to…")}</strong>
            <For each={[false, true]}>
              {(value) => (
                <label>
                  <input
                    type="radio"
                    name={`send-mode-${key()}`}
                    checked={modEnter() === value}
                    onChange={() => {
                      setModEnter(value);
                      savePreference("flek.composer.modEnter", value);
                    }}
                  />
                  <span>
                    {value ? t("Start a new line") : t("Send the message")}
                    <small>
                      {value
                        ? t("Send with {key}Enter", { key: keyCombo([modKeyLabel(), ""]) })
                        : t("New line with Shift+Enter")}
                    </small>
                  </span>
                </label>
              )}
            </For>
          </div>
        </Show>
      </div>
    </div>
  );
}
