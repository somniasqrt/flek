import { createSignal, Show } from "solid-js";
import { t, errorMessage, locale } from "../i18n/index";
import { tsDate, type Conversation } from "../api";

export function dayLabel(ts: string): string {
  const d = tsDate(ts);
  const today = new Date();
  const yesterday = new Date(today.getTime() - 86_400_000);
  if (d.toDateString() === today.toDateString()) return t("Today");
  if (d.toDateString() === yesterday.toDateString()) return t("Yesterday");
  return d.toLocaleDateString(locale(), {
    weekday: "long",
    day: "numeric",
    month: "long",
    year: d.getFullYear() === today.getFullYear() ? undefined : "numeric",
  });
}

export function JoinChannelBar(props: {
  conversation: Conversation;
  onJoin: (id: string) => Promise<void>;
}) {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const join = async () => {
    if (busy()) return;
    setBusy(true);
    setError("");
    try {
      await props.onJoin(props.conversation.id);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div class="channel-join-bar">
      <span>{t("You're viewing #{name}", { name: props.conversation.label })}</span>
      <button disabled={busy() || props.conversation.is_archived} onClick={join}>
        {busy() ? t("Joining…") : t("Join channel")}
      </button>
      <Show when={error()}>
        <p class="error" role="alert">{errorMessage(error())}</p>
      </Show>
    </div>
  );
}
