import { createResource, Show } from "solid-js";
import { api, linkImageUrl } from "../../api";
import Avatar from "../../components/Avatar";
import { t } from "../../i18n/index";
import { people } from "../../stores/people";
import { pick } from "../../platform";
import { Card, Row } from "./controls";
import type { PrefsContext } from "./context";

export default function AccountSection(props: { ctx: PrefsContext }) {
  const ctx = props.ctx;
  const [origin] = createResource(api.accountOrigin);
  const [icon] = createResource(api.teamIcon);
  return (
    <Show when={ctx.ready()} fallback={<p class="pref-note">{t("Not signed in")}</p>}>
      {(s) => {
        const name = () => people()[s().user_id] ?? s().user;
        return (
          <>
            <Card>
              <div class="pref-identity">
                <Show
                  when={icon()}
                  fallback={<span class="pref-team-fallback">{s().team.slice(0, 1)}</span>}
                >
                  {(hash) => <img class="preferences-team" src={linkImageUrl(hash())} alt="" />}
                </Show>
                <div class="pref-identity-text">
                  <div class="pref-identity-name">{s().team}</div>
                  <div class="pref-row-desc">{t("Workspace")}</div>
                </div>
              </div>
              <div class="pref-identity">
                <Avatar id={s().user_id} name={name()} size={40} />
                <div class="pref-identity-text">
                  <div class="pref-identity-name">{name()}</div>
                  <div class="pref-row-desc">{t("You")}</div>
                </div>
              </div>
              <Row label={t("Session source")}>
                <span class="pref-pill">
                  {origin() === "desktop"
                    ? t("Slack desktop import")
                    : origin() === "webview"
                      ? t("Sign-in")
                      : t("Session source unknown")}
                </span>
              </Row>
            </Card>
            <Card danger title={t("Danger zone")}>
              <Row
                label={t("Sign out")}
                description={pick({
                  mac: t("The session will be removed from Keychain. The local archive is kept."),
                  windows: t(
                    "The session will be removed from Credential Manager. The local archive is kept.",
                  ),
                  linux: t(
                    "The session will be removed from the system keyring. The local archive is kept.",
                  ),
                })}
              >
                <button
                  type="button"
                  class="pref-button danger"
                  disabled={ctx.busy()}
                  onClick={() => ctx.ask("signout")}
                >
                  {t("Sign out of {name}", { name: s().team })}
                </button>
              </Row>
            </Card>
          </>
        );
      }}
    </Show>
  );
}
