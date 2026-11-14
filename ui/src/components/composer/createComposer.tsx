import {
  createEffect,
  createMemo,
  createResource,
  createSignal,
  createUniqueId,
  on,
  onCleanup,
  onMount,
} from "solid-js";
import { api, linkImageUrl } from "../../api";
import { customEmojiNames, resolveEmoji } from "../EmojiView";
import { standardEmojiNames } from "../../models/emoji";
import {
  readFrequency,
  readSkinTone,
  rememberEmoji,
  tonedName,
} from "../../models/emoji-picker-data";
import {
  channelCompletions,
  emojiCompletions,
  personCompletions,
  type Completion,
} from "../../models/composer-autocomplete";
import { completionRange } from "./completionRange";
import ComposeIcon from "../ComposeIcon";
import { draftKey, escapeText, readDraft, writeDraft } from "../../models/compose";
import { blocksText, editorBlocks, restoreEditor, safeLink } from "../../models/rich-compose";
import {
  ariaShortcut,
  editShortcut,
  usesMacShortcuts,
  primaryModifier,
  shortcutLabel,
  type EditAction,
} from "../../models/shortcuts";
import { people, channels } from "../../stores/people";
import { send } from "../../stores/outbox";

// Rich message editor shared by conversations and threads.

const preference = (key: string, fallback: boolean) => {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : v === "true";
  } catch {
    return fallback;
  }
};
export const savePreference = (key: string, value: boolean) => {
  try {
    localStorage.setItem(key, String(value));
  } catch {
    /* unavailable storage */
  }
};

export type ComposerProps = {
  conversationId: string;
  threadTs?: string;
  placeholder: string;
  broadcastLabel?: string;
  onSent?: () => void;
};

export function createComposer(props: ComposerProps) {
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
  let executing = false;
  const completionId = createUniqueId();
  const [completion, setCompletion] = createSignal<ReturnType<typeof completionRange>>(null);
  const [completionIndex, setCompletionIndex] = createSignal(0);
  let dismissed = "",
    composing = false;
  const [directory] = createResource(
    () => ({ conversation: props.conversationId, revision: people() }),
    ({ conversation }) => api.composerPeople(conversation),
  );
  const [frequency, setFrequency] = createSignal(readFrequency());
  const emojiNames = createMemo(() => [...standardEmojiNames, ...customEmojiNames()]);
  const completions = createMemo(() => {
    const token = completion()?.token;
    if (!token) return [];
    if (token.kind === "emoji") return emojiCompletions(token.query, emojiNames(), frequency());
    if (token.kind === "channel") return channelCompletions(token.query, channels());
    const data = directory.state === "ready" ? directory() : undefined;
    return data
      ? personCompletions(
          token.query,
          data,
          channels().some((c) => c.id === props.conversationId),
        )
      : [];
  });
  const completionOpen = () => !!completion() && completions().length > 0;
  const closeCompletion = () => {
    const current = completion();
    if (current) dismissed = `${current.token.start}:${current.token.kind}:${current.token.query}`;
    setCompletion(null);
  };
  const updateCompletion = (convert = false) => {
    if (composing || document.activeElement !== editor || popover()) {
      setCompletion(null);
      return;
    }
    const next = completionRange(editor);
    const signature = next ? `${next.token.start}:${next.token.kind}:${next.token.query}` : "";
    if (!next) dismissed = "";
    if (next && signature === dismissed) {
      setCompletion(null);
      return;
    }
    const old = completion();
    if (next?.token.kind === "emoji" && old?.token.kind !== "emoji") setFrequency(readFrequency());
    if (
      !old ||
      !next ||
      old.token.query !== next.token.query ||
      old.token.start !== next.token.start ||
      old.token.kind !== next.token.kind
    )
      setCompletionIndex(0);
    setCompletion(next);
    if (convert && next?.token.complete) {
      const name = emojiNames().find((n) => n.toLowerCase() === next.token.query.toLowerCase());
      if (name) chooseCompletion({ kind: "emoji", id: name, label: name });
      else setCompletion(null);
    } else if (next?.token.complete) setCompletion(null);
  };
  createEffect(() => {
    const count = completions().length;
    if (completionIndex() >= count) setCompletionIndex(0);
  });
  createEffect(() => {
    if (popover()) closeCompletion();
  });
  createEffect(() => {
    const index = completionIndex();
    if (!completionOpen()) return;
    queueMicrotask(() => {
      if (completionOpen() && completionIndex() === index)
        document.getElementById(`${completionId}-${index}`)?.scrollIntoView({ block: "nearest" });
    });
  });
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
    ) {
      setCompletion(null);
      return;
    }
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
    updateCompletion();
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
    // execCommand emits input too. Record it once, separately from typed text,
    // so completion Undo restores the literal trigger instead of coalescing it.
    remember();
    executing = true;
    try {
      document.execCommand(command, false, value);
    } finally {
      executing = false;
    }
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
    setCompletion(null);
    dismissed = "";
    setBroadcast(false);
    refresh();
    editor.focus();
    props.onSent?.();
  };
  const undo = (redo: boolean) => {
    historyIndex = Math.max(0, Math.min(history.length - 1, historyIndex + (redo ? 1 : -1)));
    restoreEditor(editor, JSON.stringify({ version: 1, html: history[historyIndex] ?? "" }));
    hydrateEmoji();
    savedRange = null;
    focusSelection();
    refresh();
    selectionChanged();
  };
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.isComposing || e.keyCode === 229) return;
    if (completionOpen() && ["ArrowUp", "ArrowDown", "Tab", "Enter", "Escape"].includes(e.key)) {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") closeCompletion();
      else if (e.key === "Tab" || e.key === "Enter")
        chooseCompletion(completions()[completionIndex()]);
      else {
        const count = completions().length;
        const index = (completionIndex() + (e.key === "ArrowUp" ? -1 : 1) + count) % count;
        setCompletionIndex(index);
      }
      return;
    }
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
      hydrateEmoji();
      setBroadcast(false);
      setCompletion(null);
      dismissed = "";
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
      if (!box.contains(e.target as Node)) {
        setPopover(null);
        closeCompletion();
      }
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
      aria-keyshortcuts={ariaShortcut(name, mac)}
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
  // Toolbar and completion share these exact atomic nodes and the same undo path.
  const personHtml = (id: string, name: string) =>
    `<span class="mention" contenteditable="false" data-user-id="${id.replace(/[^A-Z0-9]/g, "")}">@${escapeText(name)}</span> `;
  const emojiHtml = (name: string) => {
    const safeName = name.replace(/[^a-z0-9_+:.\-']/gi, "");
    const resolved = resolveEmoji(name);
    const body =
      resolved && "image" in resolved
        ? `<img class="emoji-img" src="${escapeText(linkImageUrl(resolved.image)).replace(/"/g, "&quot;")}" alt=":${escapeText(safeName)}:" draggable="false">`
        : escapeText(resolved && "char" in resolved ? resolved.char : `:${safeName}:`);
    return `<span contenteditable="false" data-emoji-name="${escapeText(safeName)}">${body}</span> `;
  };
  const hydrateEmoji = () => {
    for (const span of editor.querySelectorAll<HTMLElement>("[data-emoji-name]")) {
      const template = document.createElement("template");
      template.innerHTML = emojiHtml(span.dataset.emojiName!);
      span.replaceChildren(...template.content.firstElementChild!.childNodes);
    }
  };
  createEffect(() => {
    emojiNames();
    hydrateEmoji();
  });
  const mention = (id: string, name: string) => {
    exec("insertHTML", personHtml(id, name));
    setPopover(null);
  };
  const insertEmoji = (name: string) => {
    setFrequency(readFrequency());
    exec("insertHTML", emojiHtml(name));
    setPopover(null);
  };
  const chooseCompletion = (item: Completion | undefined) => {
    const current = completion();
    if (!current || !item || !editor.contains(current.range.startContainer)) return;
    const live = completionRange(editor);
    if (
      !live ||
      live.token.start !== current.token.start ||
      live.token.end !== current.token.end ||
      live.token.query !== current.token.query
    ) {
      setCompletion(null);
      return;
    }
    // Keep the literal query as a separate undo point before replacing it.
    remember();
    savedRange = current.range.cloneRange();
    setCompletion(null);
    dismissed = "";
    let html: string;
    if (item.kind === "person") html = personHtml(item.id, item.label);
    else if (item.kind === "channel")
      html = `<span class="mention" contenteditable="false" data-channel-id="${item.id.replace(/[^A-Z0-9]/g, "")}">#${escapeText(item.label)}</span> `;
    else if (item.kind === "broadcast")
      html = `<span class="mention" contenteditable="false" data-broadcast="${item.id}">@${item.label}</span> `;
    else {
      const name = tonedName(item.id, readSkinTone());
      rememberEmoji(name);
      setFrequency(readFrequency());
      html = emojiHtml(name);
    }
    exec("insertHTML", html);
  };
  const onInput = (e: InputEvent) => {
    if (executing) return;
    remember(e.inputType === "insertText");
    refresh();
    selectionChanged();
    if (!e.isComposing) updateCompletion(true);
  };
  const onCompositionStart = () => {
    composing = true;
    setCompletion(null);
  };
  const onCompositionEnd = () => {
    composing = false;
    selectionChanged();
    updateCompletion(true);
  };
  return {
    mac,
    key,
    hasText,
    toolbar,
    modEnter,
    setModEnter,
    popover,
    setPopover,
    broadcast,
    setBroadcast,
    active,
    linkUrl,
    setLinkUrl,
    linkLabel,
    setLinkLabel,
    personQuery,
    setPersonQuery,
    remember,
    refresh,
    selectionChanged,
    focusSelection,
    exec,
    insert,
    toggleToolbar,
    action,
    applyLink,
    submit,
    onKeyDown,
    title,
    formatButton,
    mention,
    insertEmoji,
    completionId,
    completionOpen,
    completions,
    completionIndex,
    setCompletionIndex,
    chooseCompletion,
    onInput,
    onCompositionStart,
    onCompositionEnd,
    closeCompletion,
    setBox: (el: HTMLDivElement) => {
      box = el;
    },
    setEditor: (el: HTMLDivElement) => {
      editor = el;
    },
  };
}
