import { createEffect, createSignal, For, on, onCleanup, onMount, Show } from "solid-js";
import { t } from "./i18n";
import { emoji } from "./emoji";
import EmojiPicker from "./EmojiPicker";
import ComposeIcon from "./ComposeIcon";
import { draftKey, escapeText, readDraft, writeDraft } from "./compose";
import { blocksText, editorBlocks, restoreEditor, safeLink } from "./rich-compose";
import {
  bindings,
  editShortcut,
  usesMacShortcuts,
  primaryModifier,
  shortcutLabel,
  type EditAction,
} from "./shortcuts";
import { keyCombo, modKeyLabel } from "./platform";
import { people } from "./people";
import { send } from "./outbox";

// Rich message editor shared by conversations and threads.

const preference = (key: string, fallback: boolean) => {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : v === "true";
  } catch {
    return fallback;
  }
};
const savePreference = (key: string, value: boolean) => {
  try {
    localStorage.setItem(key, String(value));
  } catch {
    /* unavailable storage */
  }
};

export default function Composer(props: {
  conversationId: string;
  threadTs?: string;
  placeholder: string;
  broadcastLabel?: string;
  onSent?: () => void;
}) {
  const mac = usesMacShortcuts();
  const key = () => draftKey(props.conversationId, props.threadTs);
  const [hasText, setHasText] = createSignal(false);
  const [toolbar, setToolbar] = createSignal(preference("flek.composer.toolbar", true));
  const [modEnter, setModEnter] = createSignal(preference("flek.composer.modEnter", false));
  const [popover, setPopover] = createSignal<
    "emoji" | "link" | "paragraph" | "send" | "mention" | null
  >(null);
  const [broadcast, setBroadcast] = createSignal(false);
  const [active, setActive] = createSignal<Record<string, boolean>>({});
  const [linkUrl, setLinkUrl] = createSignal("");
  const [linkLabel, setLinkLabel] = createSignal("");
  const [personQuery, setPersonQuery] = createSignal("");
  let box!: HTMLDivElement, editor!: HTMLDivElement;
  let savedRange: Range | null = null;
  // One undo history per open draft; switching channels cannot undo into another draft.
  let history: string[] = [],
    historyIndex = -1;
  let lastEditAt = 0;
  const remember = (coalesce = false) => {
    const html = editor.innerHTML;
    if (history[historyIndex] === html) return;
    const now = Date.now();
    if (coalesce && now - lastEditAt < 600 && historyIndex > 0) history[historyIndex] = html;
    else {
      history = history.slice(0, historyIndex + 1);
      history.push(html);
      if (history.length > 100) history.shift();
      historyIndex = history.length - 1;
    }
    lastEditAt = coalesce ? now : 0;
  };
  const refresh = () => {
    const text = blocksText(editorBlocks(editor), people());
    setHasText(!!text);
    writeDraft(key(), text ? JSON.stringify({ version: 1, html: editor.innerHTML }) : "");
  };
  const selectionChanged = () => {
    const selection = window.getSelection();
    if (
      !selection?.rangeCount ||
      !editor.contains(selection.anchorNode) ||
      !editor.contains(selection.focusNode)
    )
      return;
    savedRange = selection.getRangeAt(0).cloneRange();
    const states: Record<string, boolean> = {};
    for (const command of [
      "bold",
      "italic",
      "underline",
      "strikeThrough",
      "insertOrderedList",
      "insertUnorderedList",
    ])
      states[command] = document.queryCommandState(command);
    const node = selection.anchorNode?.parentElement;
    states.code = !!node?.closest("code");
    states.pre = !!node?.closest("pre");
    states.quote = !!node?.closest("blockquote");
    setActive(states);
  };
  const focusSelection = () => {
    editor.focus({ preventScroll: true });
    const selection = window.getSelection();
    if (!selection) return;
    if (savedRange && editor.contains(savedRange.commonAncestorContainer)) {
      selection.removeAllRanges();
      selection.addRange(savedRange);
    } else {
      const range = document.createRange();
      range.selectNodeContents(editor);
      range.collapse(false);
      selection.removeAllRanges();
      selection.addRange(range);
    }
  };
  const exec = (command: string, value?: string) => {
    focusSelection();
    document.execCommand(command, false, value);
    remember();
    refresh();
    selectionChanged();
  };
  const insert = (text: string) => {
    exec("insertText", text);
    setPopover(null);
  };
  const toggleToolbar = () => {
    const value = !toolbar();
    setToolbar(value);
    savePreference("flek.composer.toolbar", value);
  };
  const action = (name: EditAction) => {
    if (name === "toolbar") {
      toggleToolbar();
      return;
    }
    if (name === "emoji") {
      setPopover(popover() === "emoji" ? null : "emoji");
      return;
    }
    if (name === "link") {
      focusSelection();
      setLinkLabel(window.getSelection()?.toString() ?? "");
      setLinkUrl("");
      setPopover("link");
      return;
    }
    const commands: Partial<Record<EditAction, string>> = {
      bold: "bold",
      italic: "italic",
      underline: "underline",
      strike: "strikeThrough",
      ordered: "insertOrderedList",
      bullet: "insertUnorderedList",
    };
    if (commands[name]) {
      exec(commands[name]!);
      return;
    }
    if (name === "quote" || name === "pre") {
      exec("formatBlock", active()[name] ? "div" : name === "quote" ? "blockquote" : "pre");
      return;
    }
    if (name === "clear") {
      exec("removeFormat");
      exec("unlink");
      exec("formatBlock", "div");
      return;
    }
    if (name === "code") {
      focusSelection();
      const range = window.getSelection()?.getRangeAt(0);
      const code = range?.startContainer.parentElement?.closest("code");
      if (code && editor.contains(code)) {
        code.replaceWith(...code.childNodes);
        remember();
        refresh();
        selectionChanged();
      } else {
        const selected = window.getSelection()?.toString() || "\u200b";
        exec("insertHTML", `<code data-compose-code="true">${escapeText(selected)}</code>`);
        const inserted = editor.querySelector("code[data-compose-code]");
        if (inserted) {
          inserted.removeAttribute("data-compose-code");
          const caret = document.createRange();
          caret.selectNodeContents(inserted);
          caret.collapse(false);
          const selection = window.getSelection();
          selection?.removeAllRanges();
          selection?.addRange(caret);
          savedRange = caret.cloneRange();
          history[historyIndex] = editor.innerHTML;
          refresh();
          selectionChanged();
        }
      }
    }
  };
  const applyLink = (e: SubmitEvent) => {
    e.preventDefault();
    if (!safeLink(linkUrl().trim())) return;
    const label = linkLabel() || linkUrl().trim();
    exec("insertText", label);
    const selection = window.getSelection();
    if (selection?.anchorNode?.nodeType === 3) {
      const range = document.createRange();
      const end = selection.anchorOffset;
      range.setStart(selection.anchorNode, Math.max(0, end - label.length));
      range.setEnd(selection.anchorNode, end);
      selection.removeAllRanges();
      selection.addRange(range);
      savedRange = range.cloneRange();
    }
    exec("createLink", linkUrl().trim());
    setPopover(null);
    editor.focus();
  };
  const submit = () => {
    const blocks = editorBlocks(editor),
      text = blocksText(blocks, people());
    if (!text) return;
    send(props.conversationId, text, props.threadTs ?? null, broadcast(), blocks);
    editor.replaceChildren();
    savedRange = null;
    history = [""];
    historyIndex = 0;
    setPopover(null);
    setBroadcast(false);
    refresh();
    editor.focus();
    props.onSent?.();
  };
  const undo = (redo: boolean) => {
    historyIndex = Math.max(0, Math.min(history.length - 1, historyIndex + (redo ? 1 : -1)));
    restoreEditor(editor, JSON.stringify({ version: 1, html: history[historyIndex] ?? "" }));
    savedRange = null;
    focusSelection();
    refresh();
    selectionChanged();
  };
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.isComposing || e.keyCode === 229) return;
    const name = editShortcut(e, mac);
    if (name) {
      e.preventDefault();
      e.stopPropagation();
      action(name);
      return;
    }
    if (
      primaryModifier(e, mac) &&
      !e.altKey &&
      (e.code === "KeyZ" || (!mac && e.code === "KeyY"))
    ) {
      e.preventDefault();
      undo(e.shiftKey || e.code === "KeyY");
      return;
    }
    if (e.key === "Escape" && popover()) {
      e.preventDefault();
      e.stopPropagation();
      setPopover(null);
      return;
    }
    if (e.key === "Enter" && !e.altKey) {
      e.preventDefault();
      if (!e.shiftKey && (!modEnter() || primaryModifier(e, mac))) submit();
      else {
        const selection = window.getSelection();
        const anchor = selection?.anchorNode;
        const element = anchor instanceof Element ? anchor : anchor?.parentElement;
        const item = element?.closest("li");
        // WebKit's paragraph command splits a list item and exits an empty one.
        // Keep the current keyboard selection before exec restores the saved range.
        if (selection?.rangeCount) savedRange = selection.getRangeAt(0).cloneRange();
        exec(item && editor.contains(item) ? "insertParagraph" : "insertLineBreak");
      }
    }
  };
  createEffect(
    on(key, (k) => {
      restoreEditor(editor, readDraft(k));
      setBroadcast(false);
      setPopover(null);
      savedRange = null;
      history = [editor.innerHTML];
      historyIndex = 0;
      lastEditAt = 0;
      setHasText(!!blocksText(editorBlocks(editor), people()));
      requestAnimationFrame(() => editor.focus({ preventScroll: true }));
    }),
  );
  onMount(() => {
    const dismiss = (e: PointerEvent) => {
      if (!box.contains(e.target as Node)) setPopover(null);
    };
    document.addEventListener("pointerdown", dismiss);
    document.addEventListener("selectionchange", selectionChanged);
    onCleanup(() => {
      document.removeEventListener("pointerdown", dismiss);
      document.removeEventListener("selectionchange", selectionChanged);
    });
  });
  const title = (name: EditAction, label: string) => `${label} (${shortcutLabel(name, mac)})`;
  const formatButton = (name: EditAction, label: string, command = name as string) => (
    <button
      type="button"
      class="compose-tool"
      title={title(name, label)}
      aria-label={label}
      aria-keyshortcuts={`${mac ? "Meta" : "Control"}+${bindings[name].shift ? "Shift+" : ""}${bindings[name].key}`}
      aria-pressed={!!active()[command]}
      onPointerDown={(e) => e.preventDefault()}
      onClick={() => action(name)}
    >
      <ComposeIcon
        name={
          name === "ordered" ||
          name === "bullet" ||
          name === "pre" ||
          name === "clear" ||
          name === "link" ||
          name === "quote" ||
          name === "code" ||
          name === "bold" ||
          name === "italic" ||
          name === "underline" ||
          name === "strike"
            ? name
            : "format"
        }
      />
    </button>
  );
  const mention = (id: string, name: string) => {
    exec(
      "insertHTML",
      `<span class="mention" contenteditable="false" data-user-id="${id.replace(/[^A-Z0-9]/g, "")}">@${escapeText(name)}</span> `,
    );
    setPopover(null);
  };
  return (
    <div class="composer">
      <div
        class="composer-box"
        ref={box}
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
          ref={editor}
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
