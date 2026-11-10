import { createResource, For, Show } from "solid-js";
import { api, type Conversation } from "./api";
import Avatar from "./Avatar";
import { people } from "./people";
import { presenceOf } from "./presence-store";
import { openProfile } from "./profile";

export default function DmIntro(props: {
  conversation: Conversation;
  me: string;
  members: string[];
}) {
  const partner = () => (props.conversation.kind === "im" ? props.conversation.dm_user_id : null);
  const [profile] = createResource(partner, (id) => api.profile(id).catch(() => null));
  const [photo] = createResource(partner, (id) => api.profilePhoto(id).catch(() => null));
  const self = () => partner() === props.me;
  return (
    <div class="dm-intro">
      <Show
        when={partner()}
        fallback={
          <>
            <div class="dm-intro-group">
              <For each={props.members}>
                {(id) => <Avatar id={id} name={people()[id] ?? id} size={72} />}
              </For>
            </div>
            <h2>{props.conversation.label}</h2>
            <p>This is the start of your conversation with {props.conversation.label}.</p>
          </>
        }
      >
        {(id) => (
          <>
            <Avatar
              id={id()}
              name={props.conversation.label}
              size={140}
              presence={presenceOf(id())}
              photo={photo()}
            />
            <h2>
              {props.conversation.label}
              {self() ? " (you)" : ""}
            </h2>
            <Show when={profile()?.title}>
              <p class="muted">{profile()!.title}</p>
            </Show>
            <Show
              when={self()}
              fallback={
                <p>
                  This conversation is just between you and{" "}
                  <strong>{props.conversation.label}</strong>. Take a look at their profile to learn
                  more about them.
                </p>
              }
            >
              <p>
                <strong>This is your space.</strong> Draft messages, make to-do lists or keep links
                and files to hand. You can also talk to yourself here, but please bear in mind
                you'll have to provide both sides of the conversation.
              </p>
            </Show>
            <button onClick={() => openProfile(id())}>View profile</button>
          </>
        )}
      </Show>
    </div>
  );
}
