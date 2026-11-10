import {
  createEffect,
  createResource,
  createSignal,
  For,
  onCleanup,
  onMount,
  Match,
  Show,
  Switch,
} from "solid-js";
import { Portal } from "solid-js/web";
import { t, errorMessage } from "./i18n";
import { api, type Status } from "./api";
import { Cross, Gear, ImageIcon, Palette, Person, Refresh, SidebarPanel } from "./icons";
import { changeSetting, setPreferencesOpen } from "./settings";
import type { Settings } from "./settings-model";
import AccountSection from "./preferences/AccountSection";
import type { Confirmable, PrefsContext } from "./preferences/context";
import {
  AppearanceSection,
  GeneralSection,
  MediaSection,
  SidebarSection,
} from "./preferences/sections";
import SyncSection from "./preferences/SyncSection";

const sections = [
  ["General", Gear],
  ["Appearance", Palette],
  ["Sidebar", SidebarPanel],
  ["Messages & media", ImageIcon],
  ["Sync & storage", Refresh],
  ["Account", Person],
] as const;
type Section = (typeof sections)[number][0];

export default function Preferences(props: { status: Status }) {
  const [section, setSection] = createSignal<Section>("General");
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [confirm, setConfirm] = createSignal<Confirmable | null>(null);
  const [storage, { refetch }] = createResource(api.storageInfo);
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
  const ctx: PrefsContext = {
    busy,
    update,
    run,
    ask: setConfirm,
    ready,
    storage,
  };
  const moveSection = (e: KeyboardEvent) => {
    const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const index = sections.findIndex(([name]) => name === section());
    const next = sections[(index + step + sections.length) % sections.length]![0];
    setSection(next);
    queueMicrotask(() => {
      panel.querySelector<HTMLElement>('nav button[aria-current="page"]')?.focus();
    });
  };
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
          <header class="preferences-header" inert={confirm() ? true : undefined}>
            <h2>{t("Preferences")}</h2>
            <button aria-label={t("Close")} disabled={busy()} onClick={close}>
              <Cross size={18} />
            </button>
          </header>
          <div class="preferences-layout" inert={confirm() ? true : undefined}>
            <nav aria-label={t("Preferences sections")} onKeyDown={moveSection}>
              <For each={sections}>
                {([name, SectionIcon]) => (
                  <button
                    aria-current={section() === name ? "page" : undefined}
                    onClick={() => setSection(name)}
                  >
                    <SectionIcon size={18} />
                    <span>{t(name)}</span>
                  </button>
                )}
              </For>
            </nav>
            <div class="preferences-pane">
              <h3>{t(section())}</h3>
              <div class="preferences-sections">
                <Switch>
                  <Match when={section() === "General"}>
                    <GeneralSection ctx={ctx} />
                  </Match>
                  <Match when={section() === "Appearance"}>
                    <AppearanceSection ctx={ctx} />
                  </Match>
                  <Match when={section() === "Sidebar"}>
                    <SidebarSection ctx={ctx} />
                  </Match>
                  <Match when={section() === "Messages & media"}>
                    <MediaSection ctx={ctx} />
                  </Match>
                  <Match when={section() === "Sync & storage"}>
                    <SyncSection ctx={ctx} />
                  </Match>
                  <Match when={section() === "Account"}>
                    <AccountSection ctx={ctx} />
                  </Match>
                </Switch>
              </div>
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
              <div class="preferences-confirm-actions">
                <button class="pref-button" onClick={() => setConfirm(null)}>
                  {t("Cancel")}
                </button>
                <button class="pref-button danger" onClick={confirmed}>
                  {t("Confirm")}
                </button>
              </div>
            </div>
          </Show>
        </div>
      </div>
    </Portal>
  );
}
