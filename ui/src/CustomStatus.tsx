import { Show } from "solid-js";
import Emoji from "./EmojiView";
import { visibleStatus, type CustomStatus as Status } from "./presence";
import { presenceNow } from "./presence-store";

export default function CustomStatus(props: { status: Status | null | undefined; text?: boolean }) {
  const status = () => visibleStatus(props.status, presenceNow());
  return (
    <Show when={status()}>
      {(s) => (
        <span class="custom-status" title={s().status_text ?? undefined}>
          <Show when={s().status_emoji}>
            {(e) => <Emoji name={e().replace(/^:|:$/g, "")} inText />}
          </Show>
          <Show when={props.text}>{s().status_text}</Show>
        </span>
      )}
    </Show>
  );
}
