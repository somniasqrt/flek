import { createEffect, createSignal, For, Show } from "solid-js";
import { api, type ChannelInfo } from "./api";
import { people } from "./people";

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
        <h2>Channel details</h2>
        <button
          class="plain-icon"
          aria-label="Close channel details"
          onClick={props.onClose}
        >
          ×
        </button>
      </header>
      <div class="channel-panel-body">
        <Show when={error()}>
          <p class="error" role="alert">{error()}</p>
        </Show>
        <Show when={info()}>
          {(data) => (
            <>
              <h2>#{data().conversation.name ?? data().conversation.label}</h2>
              <h3>Topic</h3>
              <p>{data().conversation.topic ?? "No topic"}</p>
              <h3>Purpose</h3>
              <p>{data().conversation.purpose ?? "No purpose"}</p>
              <h3>Created</h3>
              <p>
                {data().created
                  ? new Date(data().created! * 1000).toLocaleDateString()
                  : "Unknown"}
              </p>
              <p>By {people()[data().creator ?? ""] ?? data().creator ?? "Unknown"}</p>
              <h3>Members ({data().num_members ?? data().members?.length ?? "unknown"})</h3>
              <Show when={data().members} fallback={<p class="muted">Members not cached</p>}>
                <For each={data().members ?? []}>
                  {(user) => <p>{user.display_name || user.name}</p>}
                </For>
              </Show>
              <h3>Pinned messages</h3>
              <Show when={data().pins} fallback={<p class="muted">Pins not cached</p>}>
                <For each={data().pins ?? []}>
                  {(pin) => <p>{pin.text}</p>}
                </For>
              </Show>
            </>
          )}
        </Show>
        <button disabled={loading()} onClick={() => load(true)}>
          {loading() ? "Loading…" : "Refresh members and pins"}
        </button>
      </div>
    </aside>
  );
}
