import { createEffect, createSignal, For, on, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { t, errorMessage, locale, clock24 } from "./i18n";
import { setPreferencesOpen } from "./settings";
import { api } from "./api";
import Avatar from "./Avatar";
import CustomStatus from "./CustomStatus";
import T from "./i18n/T";
import Emoji from "./EmojiView";
import EmojiPicker from "./EmojiPicker";
import PresenceIndicator from "./PresenceIndicator";
import { people } from "./people";
import { openProfile } from "./profile";
import {
  indicator,
  dndUntil,
  statusExpiration,
  tomorrowMinutes,
  visibleStatus,
  type ClearAfter,
} from "./presence";
import {
  directoryChanged,
  presenceNow,
  presenceOf,
  presenceRows,
  refreshPresence,
  statuses,
} from "./presence-store";

export default function YouMenu(props: {
  pauseOnly?: boolean;
  userId: string;
  user: string;
  anchor: DOMRect;
  onClose: () => void;
}) {
  const [view, setView] = createSignal<"menu" | "status">("menu");
  const [pause, setPause] = createSignal(!!props.pauseOnly);
  const [picker, setPicker] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const current = () => visibleStatus(statuses()[props.userId], presenceNow());
  const [text, setText] = createSignal(current()?.status_text ?? "");
  const [emoji, setEmoji] = createSignal(current()?.status_emoji?.replace(/^:|:$/g, "") ?? "");
  const [after, setAfter] = createSignal<ClearAfter>("today");
  const name = () => people()[props.userId] ?? props.user;
  const row = () => presenceRows()[props.userId];
  const away = () =>
    indicator(row() ? { ...row()!, dnd_fresh: false } : undefined, presenceNow()) !== "active";
  const paused = () => dndUntil(row(), presenceNow());
  let panel!: HTMLDivElement;
  let emojiButton: HTMLButtonElement | undefined;
  const [position, setPosition] = createSignal({
    left: props.anchor.right + 8,
    top: props.anchor.bottom,
  });
  const fit = () => {
    if (view() === "status") return;
    setPosition({
      left: Math.max(
        8,
        Math.min(props.anchor.right + 8, window.innerWidth - panel.offsetWidth - 8),
      ),
      top: Math.max(
        8,
        Math.min(
          props.anchor.bottom - panel.offsetHeight,
          window.innerHeight - panel.offsetHeight - 8,
        ),
      ),
    });
  };
  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    const observer = new ResizeObserver(fit);
    observer.observe(panel);
    fit();
    window.addEventListener("resize", props.onClose);
    onCleanup(() => {
      window.removeEventListener("resize", props.onClose);
      observer.disconnect();
      previous?.focus({ preventScroll: true });
    });
  });
  createEffect(
    on(
      () => [view(), pause(), picker()] as const,
      ([view, pause, picker], previous) =>
        queueMicrotask(() => {
          fit();
          let control: Element | undefined | null;
          if (picker) {
            control = panel.querySelector(".status-emoji-picker input");
          } else if (view === "status" && previous?.[2]) {
            control = emojiButton;
          } else if (view === "status") {
            control = panel.querySelector(".status-input-row input");
          } else if (pause) {
            control = panel.querySelector(".you-pause button");
          } else if (previous?.[1]) {
            control = panel.querySelector('[aria-haspopup="menu"]');
          } else {
            control = panel.querySelector("button:not(:disabled)");
          }
          (control as HTMLElement | null)?.focus();
        }),
    ),
  );
  const run = async (action: () => Promise<void>, saved = false) => {
    if (busy()) return;
    setError("");
    setBusy(true);
    try {
      await action();
      await refreshPresence();
      directoryChanged();
      if (saved) {
        if (props.pauseOnly) props.onClose();
        setView("menu");
        setPause(false);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const keydown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      if (picker()) {
        setPicker(false);
        queueMicrotask(() => emojiButton?.focus());
      } else if (view() === "status") {
        setView("menu");
      } else if (pause() && !props.pauseOnly) {
        setPause(false);
      } else {
        props.onClose();
      }
      return;
    }
    if (
      view() === "menu" &&
      e.key === "ArrowRight" &&
      (e.target as HTMLElement).getAttribute("aria-haspopup") === "menu"
    ) {
      e.preventDefault();
      setPause(true);
      return;
    }
    if (view() === "menu" && pause() && !props.pauseOnly && e.key === "ArrowLeft") {
      e.preventDefault();
      setPause(false);
      return;
    }
    const controls = [
      ...panel.querySelectorAll<HTMLElement>("input, select, button:not(:disabled)"),
    ];
    const index = controls.indexOf(document.activeElement as HTMLElement);
    if (e.key === "Tab" && (e.shiftKey ? index <= 0 : index === controls.length - 1)) {
      e.preventDefault();
      (e.shiftKey ? controls.at(-1) : controls[0])?.focus();
    }
    if (view() === "menu" && ["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) {
      e.preventDefault();
      let next: number;
      if (e.key === "Home") {
        next = 0;
      } else if (e.key === "End") {
        next = controls.length - 1;
      } else {
        const direction = e.key === "ArrowUp" ? -1 : 1;
        next = (index + direction + controls.length) % controls.length;
      }
      controls[next]?.focus();
    }
  };
  return (
    <Portal>
      <div
        class="message-menu-backdrop you-backdrop"
        classList={{ "status-backdrop": view() === "status" }}
        onPointerDown={props.onClose}
      >
        <div
          ref={panel}
          class="message-menu you-menu"
          classList={{ "status-dialog": view() === "status" }}
          role={view() === "status" ? "dialog" : "menu"}
          aria-label={
            view() === "status"
              ? t("Set a status")
              : props.pauseOnly
                ? t("Pause notifications")
                : t("You")
          }
          aria-modal={view() === "status" ? true : undefined}
          aria-busy={busy()}
          style={
            view() === "menu" ? { left: `${position().left}px`, top: `${position().top}px` } : {}
          }
          onPointerDown={(e) => e.stopPropagation()}
          onKeyDown={keydown}
        >
          <Show
            when={view() === "status"}
            fallback={
              <>
                <Show when={!props.pauseOnly}>
                  <div class="you-header">
                    <Avatar
                      id={props.userId}
                      name={name()}
                      size={40}
                      presence={presenceOf(props.userId)}
                    />
                    <div>
                      <strong>{name()}</strong>
                      <div class="you-presence">
                        <PresenceIndicator presence={presenceOf(props.userId)} />{" "}
                        {away() ? t("Away") : t("Active")}
                      </div>
                    </div>
                  </div>
                  <div class="you-status-button">
                    <button
                      type="button"
                      role="menuitem"
                      disabled={busy()}
                      onClick={() => {
                        setText(current()?.status_text ?? "");
                        setEmoji(current()?.status_emoji?.replace(/^:|:$/g, "") ?? "");
                        setAfter("today");
                        setError("");
                        setView("status");
                      }}
                    >
                      <Show when={current()} fallback={<>{t("😊 Update your status")}</>}>
                        <CustomStatus status={current()} text />
                      </Show>
                    </button>
                    <Show when={current()}>
                      <button
                        type="button"
                        aria-label={t("Clear status")}
                        disabled={busy()}
                        onClick={() => run(api.clearStatus)}
                      >
                        ✕
                      </button>
                    </Show>
                  </div>
                  <button
                    class="message-menu-item"
                    role="menuitem"
                    disabled={busy()}
                    onClick={() => run(() => api.setPresence(!away()))}
                  >
                    {t(away() ? "Set yourself as active" : "Set yourself as away")}
                  </button>
                  <button
                    class="message-menu-item"
                    role="menuitem"
                    aria-haspopup="menu"
                    aria-expanded={pause()}
                    onClick={() => setPause(!pause())}
                  >
                    <T
                      k={
                        paused()
                          ? "Pause notifications <muted>(paused)</muted>"
                          : "Pause notifications"
                      }
                      tags={{ muted: (text) => <span class="muted">{text}</span> }}
                    />{" "}
                    <span aria-hidden="true">›</span>
                  </button>
                </Show>
                <Show when={props.pauseOnly}>
                  <h3>{t("Pause notifications")}</h3>
                </Show>
                <Show when={pause()}>
                  <div class="you-pause" role="menu" aria-label={t("Pause notifications")}>
                    <For
                      each={
                        [
                          [t("30 minutes"), 30],
                          [t("1 hour"), 60],
                          [t("2 hours"), 120],
                          [
                            t("Until tomorrow at {time}", {
                              time: new Date(2000, 0, 1, 9).toLocaleTimeString(locale(), {
                                hour: "numeric",
                                minute: "2-digit",
                                hour12: !clock24(),
                              }),
                            }),
                            0,
                          ],
                        ] as const
                      }
                    >
                      {([label, minutes]) => (
                        <button
                          class="message-menu-item"
                          role="menuitem"
                          disabled={busy()}
                          onClick={() => run(() => api.snooze(minutes || tomorrowMinutes()), true)}
                        >
                          {label}
                        </button>
                      )}
                    </For>
                    <button
                      class="message-menu-item"
                      role="menuitem"
                      disabled={
                        busy() ||
                        !row()?.snooze_enabled ||
                        (row()?.snooze_end ?? 0) <= presenceNow()
                      }
                      onClick={() => run(api.endSnooze, true)}
                    >
                      {t("Resume notifications")}
                    </button>
                  </div>
                </Show>
                <Show when={!props.pauseOnly}>
                  <div class="message-menu-rule" role="separator" />
                  <button
                    class="message-menu-item"
                    role="menuitem"
                    onClick={() => {
                      openProfile(props.userId);
                      props.onClose();
                    }}
                  >
                    {t("Profile")}
                  </button>
                  <button
                    class="message-menu-item"
                    role="menuitem"
                    onClick={() => {
                      props.onClose();
                      setPreferencesOpen(true);
                    }}
                  >
                    {t("Preferences…")}
                  </button>
                </Show>
              </>
            }
          >
            <h3>{t("Set a status")}</h3>
            <form
              onSubmit={(e) => {
                e.preventDefault();
                run(
                  () =>
                    api.setStatus({
                      status_text: text(),
                      status_emoji: emoji() ? `:${emoji()}:` : "",
                      status_expiration: statusExpiration(after()),
                    }),
                  true,
                );
              }}
            >
              <div class="status-input-row">
                <button
                  ref={emojiButton}
                  type="button"
                  class="status-emoji-button"
                  aria-label={t("Choose status emoji")}
                  aria-expanded={picker()}
                  disabled={busy()}
                  onClick={() => setPicker(!picker())}
                >
                  <Show when={emoji()} fallback="😊">
                    {(name) => <Emoji name={name()} inText />}
                  </Show>
                </button>
                <input
                  aria-label={t("Status text")}
                  placeholder={t("What's your status?")}
                  value={text()}
                  disabled={busy()}
                  onInput={(e) => {
                    const value = Array.from(e.currentTarget.value).slice(0, 100).join("");
                    setText(value);
                    e.currentTarget.value = value;
                  }}
                />
                <button
                  type="button"
                  aria-label={t("Clear status fields")}
                  disabled={busy()}
                  onClick={() => {
                    setText("");
                    setEmoji("");
                  }}
                >
                  ✕
                </button>
              </div>
              <Show when={picker()}>
                <div class="status-emoji-picker">
                  <EmojiPicker
                    disabled={busy()}
                    onPick={(name) => {
                      setEmoji(name);
                      setPicker(false);
                      queueMicrotask(() => emojiButton?.focus());
                    }}
                  />
                </div>
              </Show>
              <div class="status-expiration">
                <label for="status-clear-after">{t("Clear after")}</label>
                <select
                  id="status-clear-after"
                  value={after()}
                  disabled={busy()}
                  onChange={(e) => setAfter(e.currentTarget.value as ClearAfter)}
                >
                  <option value="never">{t("Don't clear")}</option>
                  <option value="30m">{t("30 minutes")}</option>
                  <option value="1h">{t("1 hour")}</option>
                  <option value="4h">{t("4 hours")}</option>
                  <option value="today">{t("Today")}</option>
                  <option value="week">{t("This week")}</option>
                </select>
              </div>
              <div class="status-actions">
                <button
                  type="button"
                  disabled={busy()}
                  onClick={() => {
                    setPicker(false);
                    setView("menu");
                  }}
                >
                  {t("Cancel")}
                </button>
                <button class="status-save" type="submit" disabled={busy()}>
                  {t("Save")}
                </button>
              </div>
            </form>
          </Show>
          <Show when={error()}>
            <p class="error you-error" role="alert">
              {errorMessage(error())}
            </p>
          </Show>
        </div>
      </div>
    </Portal>
  );
}
