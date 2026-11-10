import { createSignal, For, Show } from "solid-js";
import { Dynamic } from "solid-js/web";
import { t } from "./i18n";
import { linkImageUrl } from "./api";
import Avatar from "./Avatar";
import { Activity, Document, Home, Moon, People, Plus } from "./icons";
import { dndUntil } from "./presence";
import { presenceNow, presenceOf, presenceRows } from "./presence-store";
import { type RailView } from "./rail-model";
import YouMenu from "./YouMenu";

const items = () =>
  [
    { id: "home", label: t("Home"), icon: Home },
    { id: "dms", label: t("DMs"), icon: People },
    { id: "activity", label: t("Activity"), icon: Activity },
    { id: "files", label: t("Files"), icon: Document },
  ] as const;

export default function Rail(props: {
  team: string;
  icon: string | null;
  userId: string;
  user: string;
  view: RailView;
  dms: number;
  activity: number;
  onView: (view: RailView) => void;
  onCompose: () => void;
}) {
  const [menu, setMenu] = createSignal<{
    anchor: DOMRect;
    pause: boolean;
  } | null>(null);
  const initials = () =>
    props.team
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((w) => w[0]!.toUpperCase())
      .join("");
  const snoozed = () => !!dndUntil(presenceRows()[props.userId], presenceNow());
  const badge = (id: string) => {
    if (id === "dms") return props.dms;
    if (id === "activity") return props.activity;
    return 0;
  };
  return (
    <nav class="rail" aria-label={t("Workspace")}>
      <div class="rail-team" title={props.team}>
        <Show when={props.icon} fallback={initials()}>
          {(hash) => <img src={linkImageUrl(hash())} alt={props.team} />}
        </Show>
      </div>
      <For each={items()}>
        {(item) => (
          <button
            class="rail-item"
            classList={{ active: props.view === item.id }}
            aria-current={props.view === item.id ? "page" : undefined}
            onClick={() => props.onView(item.id)}
          >
            <span class="rail-tile">
              <Dynamic component={item.icon} size={20} />
              <Show when={badge(item.id) > 0}>
                <span class="rail-badge">{badge(item.id)}</span>
              </Show>
            </span>
            <span>{item.label}</span>
          </button>
        )}
      </For>
      <div class="rail-bottom">
        <button class="rail-action" aria-label={t("New message")} onClick={props.onCompose}>
          <Plus size={22} />
        </button>
        <button
          class="rail-action"
          classList={{ snoozed: snoozed() }}
          aria-label={t("Pause notifications")}
          aria-pressed={snoozed()}
          aria-haspopup="menu"
          aria-expanded={!!menu()?.pause}
          onClick={(e) =>
            setMenu({
              anchor: e.currentTarget.getBoundingClientRect(),
              pause: true,
            })
          }
        >
          <Moon size={22} />
        </button>
        <button
          class="rail-me"
          aria-label={t("You")}
          aria-haspopup="menu"
          aria-expanded={!!menu() && !menu()?.pause}
          title={props.user}
          onClick={(e) =>
            setMenu(
              menu()
                ? null
                : {
                    anchor: e.currentTarget.getBoundingClientRect(),
                    pause: false,
                  },
            )
          }
        >
          <Avatar
            id={props.userId}
            name={props.user}
            size={36}
            presence={presenceOf(props.userId)}
          />
        </button>
      </div>
      <Show when={menu()}>
        {(m) => (
          <YouMenu
            userId={props.userId}
            user={props.user}
            anchor={m().anchor}
            pauseOnly={m().pause}
            onClose={() => setMenu(null)}
          />
        )}
      </Show>
    </nav>
  );
}
