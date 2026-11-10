import { createEffect, createSignal, For, Show } from "solid-js";
import { t, errorMessage, locale, plural } from "../i18n/index";
import { api, type ChannelInfo } from "../api";
import { people } from "../stores/people";

export default function ChannelPanel(props: { id: string; onClose: () => void }) {
  const [info, setInfo] = createSignal<ChannelInfo | null>(null);
  const [error, setError] = createSignal("");
  const [loading, setLoading] = createSignal(false);
  let generation = 0;
  const load = async (fresh: boolean) => {
    const gen = ++generation;
    setLoading(true);
    const id = props.id;
    try {
      const result = await api.channelInfo(id, fresh);
      if (gen === generation) {
        setInfo(result);
        setError("");
      }
    } catch (e) {
      if (gen === generation) setError(String(e));
    } finally {
      if (gen === generation) setLoading(false);
    }
  };
  createEffect(() => {
    props.id;
    setInfo(null);
    void load(false);
  });
  return (
    <aside class="channel-panel">
      <header class="conv-header">
        <h2>{t("Channel details")}</h2>
        <button class="plain-icon" aria-label={t("Close channel details")} onClick={props.onClose}>
          ×
        </button>
      </header>
      <div class="channel-panel-body">
        <Show when={error()}>
          <p class="error" role="alert">
            {errorMessage(error())}
          </p>
        </Show>
        <Show when={info()}>
          {(data) => (
            <>
              <h2>#{data().conversation.name ?? data().conversation.label}</h2>
              <h3>{t("Topic")}</h3>
              <p>{data().conversation.topic ?? t("No topic")}</p>
              <h3>{t("Purpose")}</h3>
              <p>{data().conversation.purpose ?? t("No purpose")}</p>
              <h3>{t("Created")}</h3>
              <p>
                {data().created
                  ? new Date(data().created! * 1000).toLocaleDateString(locale())
                  : t("Unknown")}
              </p>
              <p>
                {t("By {name}", {
                  name: people()[data().creator ?? ""] ?? data().creator ?? t("Unknown"),
                })}
              </p>
              <h3>{plural("members", data().num_members ?? data().members?.length ?? 0)}</h3>
              <Show when={data().members} fallback={<p class="muted">{t("Members not cached")}</p>}>
                <For each={data().members ?? []}>
                  {(user) => <p>{user.display_name || user.name}</p>}
                </For>
              </Show>
              <h3>{t("Pinned messages")}</h3>
              <Show when={data().pins} fallback={<p class="muted">{t("Pins not cached")}</p>}>
                <For each={data().pins ?? []}>{(pin) => <p>{pin.text}</p>}</For>
              </Show>
            </>
          )}
        </Show>
        <button disabled={loading()} onClick={() => load(true)}>
          {loading() ? t("Loading…") : t("Refresh members and pins")}
        </button>
      </div>
    </aside>
  );
}
