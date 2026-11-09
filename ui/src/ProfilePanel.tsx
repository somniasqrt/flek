// Right-side profile panel: big photo, name, title, local time, contact
// details and a "Message" button, like Slack's. Shares the slot with the
// thread panel (Workspace keeps only one of them open).
import { createEffect, createSignal, on, onCleanup, Show } from "solid-js";
import { api, linkImageUrl, type Profile } from "./api";
import Avatar from "./Avatar";
import { emoji } from "./emoji";
import { avatars } from "./people";
import { localTime } from "./time";

const PHOTO = 240;

export default function ProfilePanel(props: {
  userId: string;
  onClose: () => void;
  /// Opens a conversation (the DM with this person).
  onOpen: (conversationId: string) => void;
}) {
  const [p, setP] = createSignal<Profile | null>(null);
  const [loaded, setLoaded] = createSignal(false);
  const [photo, setPhoto] = createSignal<string | null>(null);

  // Local time moves while the panel is open.
  const [now, setNow] = createSignal(new Date());
  const timer = window.setInterval(() => setNow(new Date()), 15_000);
  onCleanup(() => window.clearInterval(timer));

  createEffect(
    on(
      () => props.userId,
      async (id) => {
        setLoaded(false);
        setP(null);
        setPhoto(null);
        const profile = await api.profile(id).catch(() => null);
        if (id !== props.userId) return;
        setP(profile);
        setLoaded(true);
        if (profile?.photo) return setPhoto(profile.photo);
        if (!profile?.has_photo) return;
        // The small avatar stands in until the large photo arrives.
        const hash = await api.profilePhoto(id).catch(() => null);
        if (id === props.userId && hash) setPhoto(hash);
      },
    ),
  );

  const name = () => {
    const x = p();
    return x ? (x.real_name ?? x.display_name ?? x.name) : props.userId;
  };
  /// Slack's display name, when it says something the full name does not.
  const handle = () => {
    const x = p();
    return x?.display_name && x.display_name !== name() ? x.display_name : null;
  };
  const small = () => p()?.avatar ?? avatars()[props.userId] ?? null;
  const time = () => {
    const x = p();
    return x && localTime(now(), x.tz, x.tz_offset);
  };
  const status = () => {
    const x = p();
    if (!x?.status_text && !x?.status_emoji) return null;
    const e = x.status_emoji?.replace(/^:|:$/g, "");
    return { emoji: e ? (emoji(e) ?? "") : "", text: x.status_text ?? "" };
  };

  return (
    <aside class="thread-panel profile-panel" aria-label="Profile">
      <header class="thread-header">
        <h3>Profile</h3>
        <button class="thread-close" type="button" title="Close (Esc)" onClick={props.onClose}>
          <svg viewBox="0 0 20 20" width="18" height="18" aria-hidden="true">
            <path d="M5 5l10 10M15 5L5 15" />
          </svg>
        </button>
      </header>
      <div class="thread-scroller profile-body">
        <div class="profile-photo">
          <Show
            when={photo() ?? small()}
            fallback={<Avatar id={props.userId} name={name()} size={PHOTO} />}
          >
            {(hash) => <img src={linkImageUrl(hash())} alt="" width={PHOTO} height={PHOTO} />}
          </Show>
        </div>
        <Show when={loaded()}>
          <Show
            when={p()}
            fallback={<p class="muted profile-missing">Flek knows nothing about this person yet.</p>}
          >
            {(x) => (
              <>
                <div class="profile-names">
                  <h2 class="profile-name">
                    {name()}
                    <Show when={x().pronouns}>
                      {(pr) => <span class="profile-pronouns muted"> ({pr()})</span>}
                    </Show>
                  </h2>
                  <Show when={x().deleted}>
                    <span class="tag profile-tag">Deactivated</span>
                  </Show>
                  <Show when={x().is_bot}>
                    <span class="tag profile-tag">App</span>
                  </Show>
                </div>
                <Show when={handle()}>{(h) => <div class="muted profile-handle">{h()}</div>}</Show>
                <Show when={x().title}>{(t) => <div class="profile-title">{t()}</div>}</Show>
                <Show when={status()}>
                  {(s) => (
                    <div class="profile-status">
                      <Show when={s().emoji}>
                        <span class="emoji">{s().emoji}</span>
                      </Show>
                      {s().text}
                    </div>
                  )}
                </Show>
                <Show when={time()}>
                  {(t) => (
                    <div class="profile-time muted" title={x().tz_label ?? x().tz ?? undefined}>
                      <svg class="profile-icon" viewBox="0 0 20 20" width="16" height="16" aria-hidden="true">
                        <circle cx="10" cy="10" r="7.25" />
                        <path d="M10 5.5V10l3 2" />
                      </svg>
                      {t()} local time
                    </div>
                  )}
                </Show>
                <div class="profile-actions">
                  <Show
                    when={x().dm_conversation_id}
                    fallback={
                      <span
                        class="profile-button-wrap"
                        title={`No direct messages with ${name()} yet`}
                      >
                        <button class="profile-button" type="button" disabled>
                          <MessageIcon />
                          Message
                        </button>
                      </span>
                    }
                  >
                    {(dm) => (
                      <button
                        class="profile-button"
                        type="button"
                        onClick={() => props.onOpen(dm())}
                      >
                        <MessageIcon />
                        Message
                      </button>
                    )}
                  </Show>
                </div>
                <Show when={x().email || x().phone}>
                  <section class="profile-contact">
                    <h4>Contact information</h4>
                    <Show when={x().email}>
                      {(email) => (
                        <div class="profile-row">
                          <svg class="profile-icon" viewBox="0 0 20 20" width="18" height="18" aria-hidden="true">
                            <rect x="2.75" y="4.75" width="14.5" height="10.5" rx="2" />
                            <path d="M3.5 6l6.5 5 6.5-5" />
                          </svg>
                          <div>
                            <div class="profile-label muted">Email address</div>
                            <a
                              href={`mailto:${email()}`}
                              onClick={(e) => {
                                e.preventDefault();
                                api.openUrl(`mailto:${email()}`).catch(() => {});
                              }}
                            >
                              {email()}
                            </a>
                          </div>
                        </div>
                      )}
                    </Show>
                    <Show when={x().phone}>
                      {(phone) => (
                        <div class="profile-row">
                          <svg class="profile-icon" viewBox="0 0 20 20" width="18" height="18" aria-hidden="true">
                            <path d="M6.5 3h-2A1.5 1.5 0 0 0 3 4.6C3.3 11 9 16.7 15.4 17a1.5 1.5 0 0 0 1.6-1.5v-2l-3.4-1.4-1.6 1.6a9 9 0 0 1-4.7-4.7l1.6-1.6z" />
                          </svg>
                          <div>
                            <div class="profile-label muted">Phone</div>
                            <span class="profile-selectable">{phone()}</span>
                          </div>
                        </div>
                      )}
                    </Show>
                  </section>
                </Show>
              </>
            )}
          </Show>
        </Show>
      </div>
    </aside>
  );
}

const MessageIcon = () => (
  <svg class="profile-icon" viewBox="0 0 20 20" width="16" height="16" aria-hidden="true">
    <path d="M4.5 4h11A1.5 1.5 0 0 1 17 5.5v7a1.5 1.5 0 0 1-1.5 1.5H9l-4 3v-3h-.5A1.5 1.5 0 0 1 3 12.5v-7A1.5 1.5 0 0 1 4.5 4z" />
  </svg>
);
