import { createResource, For, Show } from "solid-js";
import { settings } from "./settings";
import { t } from "./i18n";
import T from "./i18n/T";
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
            <p>
              <T
                k="This is the start of your conversation with <b>{name}</b>."
                params={{ name: props.conversation.label }}
                tags={{ b: (text) => <strong>{text}</strong> }}
              />
            </p>
          </>
        }
      >
        {(id) => (
          <>
            <Avatar
              id={id()}
              name={props.conversation.label}
              size={140}
              presence={settings().presence_dots ? presenceOf(id()) : undefined}
              photo={photo()}
            />
            <h2>
              {self()
                ? t("{name} (you)", { name: props.conversation.label })
                : props.conversation.label}
            </h2>
            <Show when={profile()?.title}>
              <p class="muted">{profile()!.title}</p>
            </Show>
            <Show
              when={self()}
              fallback={
                <p>
                  <T
                    k="This conversation is just between you and <b>{name}</b>. Take a look at their profile to learn more about them."
                    params={{ name: props.conversation.label }}
                    tags={{ b: (text) => <strong>{text}</strong> }}
                  />
                </p>
              }
            >
              <p>
                <T
                  k="<b>This is your space.</b> Draft messages, make to-do lists or keep links and files to hand. You can also talk to yourself here, but please bear in mind you'll have to provide both sides of the conversation."
                  tags={{ b: (text) => <strong>{text}</strong> }}
                />
              </p>
            </Show>
            <button onClick={() => openProfile(id())}>{t("View profile")}</button>
          </>
        )}
      </Show>
    </div>
  );
}
