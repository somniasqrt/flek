import {
  createEffect,
  createResource,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
  type JSX,
} from "solid-js";
import { Portal } from "solid-js/web";
import { t, errorMessage } from "./i18n";
import { api, linkImageUrl, type Status } from "./api";
import Avatar from "./Avatar";
import { people } from "./people";
import { clock24 } from "./i18n";
import { changeSetting, settings, setPreferencesOpen } from "./settings";
import type { Settings } from "./settings-model";

const sections = [
  "General",
  "Appearance",
  "Sidebar",
  "Messages & media",
  "Sync & storage",
  "Account",
] as const;
type Section = (typeof sections)[number];

export default function Preferences(props: { status: Status }) {
  const [section, setSection] = createSignal<Section>("General");
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [confirm, setConfirm] = createSignal<"cache" | "sync" | "signout" | null>(null);
  const [storage, { refetch }] = createResource(api.storageInfo);
  const [origin] = createResource(api.accountOrigin);
  const [icon] = createResource(api.teamIcon);
  const ready = () => (props.status.phase === "ready" ? props.status : null);
  let panel!: HTMLDivElement;
  const close = () => setPreferencesOpen(false);
  let confirmTrigger: HTMLElement | null = null;
  createEffect(() => {
    const action = confirm();
    if (action) confirmTrigger = document.activeElement as HTMLElement;
    queueMicrotask(() => {
      if (action) panel.querySelector<HTMLElement>(".preferences-confirm button")?.focus();
      else confirmTrigger?.focus();
    });
  });
  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    panel.querySelector<HTMLElement>("button")?.focus();
    onCleanup(() => previous?.focus({ preventScroll: true }));
  });
  const run = async (action: () => Promise<unknown>) => {
    if (busy()) return;
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const update = <K extends keyof Settings>(key: K, value: Settings[K]) =>
    run(() => changeSetting(key, value));
  const Toggle = (p: { setting: keyof Settings; label: string }) => (
    <label class="preference-toggle">
      <input
        type="checkbox"
        checked={!!settings()[p.setting]}
        disabled={busy()}
        onChange={(e) => update(p.setting, e.currentTarget.checked)}
      />
      {p.label}
    </label>
  );
  const Select = (p: {
    label: string;
    value: string;
    options: readonly (readonly [string, string])[];
    change: (value: string) => void;
  }) => (
    <label class="preference-field">
      <span>{p.label}</span>
      <select value={p.value} disabled={busy()} onChange={(e) => p.change(e.currentTarget.value)}>
        <For each={p.options}>{([value, label]) => <option value={value}>{label}</option>}</For>
      </select>
    </label>
  );
  const Pane = (p: { name: Section; children: JSX.Element }) => (
    <Show when={section() === p.name}>{p.children}</Show>
  );
  const confirmed = async () => {
    const action = confirm();
    setConfirm(null);
    await run(async () => {
      if (action === "cache") {
        await api.clearFileCache();
        await refetch();
      } else if (action === "sync") {
        await api.fullResync();
      } else if (action === "signout") {
        await api.signOut();
        close();
      }
    });
  };
  return (
    <Portal>
      <div
        class="preferences-backdrop"
        onPointerDown={(e) => {
          if (e.target === e.currentTarget && !busy()) close();
        }}
      >
        <div
          ref={panel}
          class="preferences"
          role="dialog"
          aria-modal="true"
          aria-label={t("Preferences")}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.stopPropagation();
              e.preventDefault();
              if (confirm()) setConfirm(null);
              else if (!busy()) close();
            }
            if (e.key === "Tab") {
              const scope = confirm() ? panel.querySelector(".preferences-confirm")! : panel;
              const controls = [
                ...scope.querySelectorAll<HTMLElement>(
                  "button:not(:disabled), input:not(:disabled), select:not(:disabled)",
                ),
              ].filter((el) => el.getClientRects().length);
              const first = controls[0];
              const last = controls.at(-1);
              if (document.activeElement === (e.shiftKey ? first : last)) {
                e.preventDefault();
                (e.shiftKey ? last : first)?.focus();
              }
            }
          }}
        >
          <header inert={confirm() ? true : undefined}>
            <h2>{t("Preferences")}</h2>
            <button aria-label={t("Close")} disabled={busy()} onClick={close}>
              ×
            </button>
          </header>
          <div class="preferences-layout" inert={confirm() ? true : undefined}>
            <nav aria-label={t("Preferences sections")}>
              <For each={sections}>
                {(name) => (
                  <button
                    aria-current={section() === name ? "page" : undefined}
                    onClick={() => setSection(name)}
                  >
                    {t(name)}
                  </button>
                )}
              </For>
            </nav>
            <div class="preferences-pane">
              <h3>{t(section())}</h3>
              <Pane name="General">
                <Select
                  label={t("Language")}
                  value={settings().language}
                  options={[
                    ["system", t("System")],
                    ["en", t("English")],
                    ["uk", t("Українська")],
                  ]}
                  change={(v) => update("language", v as Settings["language"])}
                />
                <Toggle setting="launch_at_login" label={t("Launch at login")} />
                <Toggle
                  setting="keep_in_menu_bar"
                  label={t("Closing the window keeps Flek in the menu bar")}
                />
              </Pane>
              <Pane name="Appearance">
                <Select
                  label={t("Theme")}
                  value={settings().theme}
                  options={[
                    ["system", t("System")],
                    ["light", t("Light")],
                    ["dark", t("Dark")],
                  ]}
                  change={(v) => update("theme", v as Settings["theme"])}
                />
                <Select
                  label={t("Message text size")}
                  value={settings().text_size}
                  options={[
                    ["small", t("Small")],
                    ["default", t("Default")],
                    ["large", t("Large")],
                  ]}
                  change={(v) => update("text_size", v as Settings["text_size"])}
                />
                <Select
                  label={t("Clock")}
                  value={settings().clock_24 === null ? "locale" : String(settings().clock_24)}
                  options={[
                    ["locale", t("Language default")],
                    ["false", t("12-hour")],
                    ["true", t("24-hour")],
                  ]}
                  change={(v) => update("clock_24", v === "locale" ? null : v === "true")}
                />
                <p class="muted">
                  {clock24() ? t("Using a 24-hour clock") : t("Using a 12-hour clock")}
                </p>
              </Pane>
              <Pane name="Sidebar">
                <Select
                  label={t("Sort direct messages")}
                  value={settings().dm_sort}
                  options={[
                    ["recent", t("Recent activity")],
                    ["name", t("Name")],
                  ]}
                  change={(v) => update("dm_sort", v as Settings["dm_sort"])}
                />
                <Toggle
                  setting="show_muted"
                  label={t("Show muted conversations in a collapsed group")}
                />
                <Toggle setting="presence_dots" label={t("Show presence dots in DMs")} />
              </Pane>
              <Pane name="Messages & media">
                <Toggle setting="link_previews" label={t("Show link previews")} />
                <Toggle setting="image_previews" label={t("Show image previews inline")} />
                <Select
                  label={t("Auto-download files up to")}
                  value={String(settings().max_file_bytes)}
                  options={[
                    ["0", t("Off")],
                    [String(10 << 20), t("10 MB")],
                    [String(25 << 20), t("25 MB")],
                    [String(100 << 20), t("100 MB")],
                    ["null", t("No limit")],
                  ]}
                  change={(v) => update("max_file_bytes", v === "null" ? null : Number(v))}
                />
                <p class="muted">{t("Download changes apply to the next sync pass.")}</p>
              </Pane>
              <Pane name="Sync & storage">
                <label class="preference-field">
                  <span>{t("Re-check edits for the last N days when reopening")}</span>
                  <input
                    type="number"
                    min="0"
                    max="30"
                    step="1"
                    value={settings().refresh_days}
                    disabled={busy()}
                    onChange={(e) => {
                      const n = e.currentTarget.valueAsNumber;
                      if (Number.isInteger(n) && n >= 0 && n <= 30) update("refresh_days", n);
                      else e.currentTarget.value = String(settings().refresh_days);
                    }}
                  />
                </label>
                <Select
                  label={t("Re-check no more often than")}
                  value={String(settings().refresh_hours)}
                  options={[
                    ["1", t("1 hour")],
                    ["3", t("3 hours")],
                    ["12", t("12 hours")],
                    ["24", t("24 hours")],
                  ]}
                  change={(v) => update("refresh_hours", Number(v))}
                />
                <p class="muted">
                  {t("Edit checks apply when reopening. Zero days disables them.")}
                </p>
                <Show when={storage()} fallback={<p>{t("Loading storage…")}</p>}>
                  {(s) => (
                    <div class="storage-info">
                      <h4>{t("Database")}</h4>
                      <code>{s().db_path}</code>
                      <p>{t("{size} MB", { size: (s().db_bytes / 1024 / 1024).toFixed(1) })}</p>
                      <button onClick={() => run(api.showStorage)}>{t("Show in Finder")}</button>
                      <h4>{t("File cache")}</h4>
                      <p>{t("{size} MB", { size: (s().cache_bytes / 1024 / 1024).toFixed(1) })}</p>
                      <button disabled={busy()} onClick={() => setConfirm("cache")}>
                        {t("Clear file cache")}
                      </button>
                      <p class="muted">
                        {t(
                          "Messages, avatars and emoji are kept. Files may download on the next sync pass.",
                        )}
                      </p>
                    </div>
                  )}
                </Show>
                <Show when={storage.error}>
                  <p class="error">{t("Could not read storage information")}</p>
                </Show>
                <button disabled={busy() || !ready()} onClick={() => setConfirm("sync")}>
                  {t("Run a full re-sync")}
                </button>
              </Pane>
              <Pane name="Account">
                <Show when={ready()} fallback={<p>{t("Not signed in")}</p>}>
                  {(s) => (
                    <>
                      <Show when={icon()}>
                        {(hash) => (
                          <img class="preferences-team" src={linkImageUrl(hash())} alt="" />
                        )}
                      </Show>
                      <h4>{s().team}</h4>
                      <Avatar id={s().user_id} name={people()[s().user_id] ?? s().user} />
                      <p>{people()[s().user_id] ?? s().user}</p>
                      <p>
                        {origin() === "desktop"
                          ? t("Slack desktop import")
                          : origin() === "webview"
                            ? t("Sign-in")
                            : t("Session source unknown")}
                      </p>
                      <button
                        class="danger"
                        disabled={busy()}
                        onClick={() => setConfirm("signout")}
                      >
                        {t("Sign out of {name}", { name: s().team })}
                      </button>
                    </>
                  )}
                </Show>
              </Pane>
            </div>
          </div>
          <Show when={error()}>
            <p class="error preferences-error" role="alert">
              {errorMessage(error())}
            </p>
          </Show>
          <Show when={confirm()}>
            <div
              class="preferences-confirm"
              role="alertdialog"
              aria-modal="true"
              aria-label={t("Confirm action")}
            >
              <h3>
                {confirm() === "cache"
                  ? t("Clear file cache?")
                  : confirm() === "sync"
                    ? t("Run a full re-sync?")
                    : t("Sign out?")}
              </h3>
              <p>
                {confirm() === "cache"
                  ? t("Downloaded files will be removed. Messages are kept.")
                  : confirm() === "sync"
                    ? t("Flek will re-read available history in the background.")
                    : t("The session will be removed from Keychain. The local archive is kept.")}
              </p>
              <button onClick={() => setConfirm(null)}>{t("Cancel")}</button>
              <button class="danger" onClick={confirmed}>
                {t("Confirm")}
              </button>
            </div>
          </Show>
        </div>
      </div>
    </Portal>
  );
}
