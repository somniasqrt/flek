import { Show, type Accessor, type Setter } from "solid-js";
import { t } from "../i18n/index";
import { api } from "../api";
import { statusExpiration, type ClearAfter } from "../models/presence";
import Emoji from "./EmojiView";
import EmojiPicker from "./EmojiPicker";

// Rendered in YouMenu's existing status branch; draft and focus ownership stay there.
export function statusForm(ctx: {
  text: Accessor<string>;
  setText: Setter<string>;
  emoji: Accessor<string>;
  setEmoji: Setter<string>;
  after: Accessor<ClearAfter>;
  setAfter: Setter<ClearAfter>;
  picker: Accessor<boolean>;
  setPicker: Setter<boolean>;
  busy: Accessor<boolean>;
  run: (action: () => Promise<void>, saved?: boolean) => Promise<void>;
  setView: Setter<"menu" | "status">;
  setEmojiButton: (el: HTMLButtonElement) => void;
  focusEmojiButton: () => void;
}) {
  const { text, setText, emoji, setEmoji, after, setAfter, picker, setPicker, busy, run, setView } = ctx;
  return (<>
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
                  ref={ctx.setEmojiButton}
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
                      queueMicrotask(() => ctx.focusEmojiButton());
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
  </>);
}
