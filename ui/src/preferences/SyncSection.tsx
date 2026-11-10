import { Show } from "solid-js";
import { api } from "../api";
import { t } from "../i18n";
import { settings } from "../settings";
import { Card, Row, SelectBox, Stepper } from "./controls";
import type { PrefsContext } from "./context";

const megabytes = (bytes: number) => t("{size} MB", { size: (bytes / 1024 / 1024).toFixed(1) });

export default function SyncSection(props: { ctx: PrefsContext }) {
  const ctx = props.ctx;
  return (
    <>
      <Card title={t("Edit checks")}>
        <Row
          label={t("Re-check edits for the last N days when reopening")}
          description={t("Edit checks apply when reopening. Zero days disables them.")}
        >
          <Stepper
            label={t("Re-check edits for the last N days when reopening")}
            value={settings().refresh_days}
            min={0}
            max={30}
            disabled={ctx.busy()}
            onChange={(n) => ctx.update("refresh_days", n)}
          />
        </Row>
        <Row
          label={t("Re-check no more often than")}
          description={t("Keeps the number of requests to the server low.")}
        >
          <SelectBox
            label={t("Re-check no more often than")}
            value={String(settings().refresh_hours)}
            disabled={ctx.busy()}
            options={[
              ["1", t("1 hour")],
              ["3", t("3 hours")],
              ["12", t("12 hours")],
              ["24", t("24 hours")],
            ]}
            onChange={(v) => ctx.update("refresh_hours", Number(v))}
          />
        </Row>
      </Card>
      <Card title={t("Storage")}>
        <Show when={ctx.storage()} fallback={<p class="pref-note">{t("Loading storage…")}</p>}>
          {(s) => (
            <>
              <div class="pref-stats">
                <div class="pref-stat">
                  <span class="pref-stat-label">{t("Database")}</span>
                  <span class="pref-stat-value">{megabytes(s().db_bytes)}</span>
                </div>
                <div class="pref-stat">
                  <span class="pref-stat-label">{t("File cache")}</span>
                  <span class="pref-stat-value">{megabytes(s().cache_bytes)}</span>
                </div>
              </div>
              <Row stacked label={t("Location")}>
                <div class="pref-path">
                  <code>{s().db_path}</code>
                  <button
                    type="button"
                    class="pref-button"
                    onClick={() => ctx.run(api.showStorage)}
                  >
                    {t("Show in Finder")}
                  </button>
                </div>
              </Row>
            </>
          )}
        </Show>
        <Show when={ctx.storage.error}>
          <p class="error pref-note">{t("Could not read storage information")}</p>
        </Show>
      </Card>
      <Card title={t("Maintenance")}>
        <Row
          label={t("Clear file cache")}
          description={t(
            "Messages, avatars and emoji are kept. Files may download on the next sync pass.",
          )}
        >
          <button
            type="button"
            class="pref-button"
            disabled={ctx.busy()}
            onClick={() => ctx.ask("cache")}
          >
            {t("Clear")}
          </button>
        </Row>
        <Row
          label={t("Run a full re-sync")}
          description={t("Flek will re-read available history in the background.")}
        >
          <button
            type="button"
            class="pref-button"
            disabled={ctx.busy() || !ctx.ready()}
            onClick={() => ctx.ask("sync")}
          >
            {t("Re-sync")}
          </button>
        </Row>
      </Card>
    </>
  );
}
