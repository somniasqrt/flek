import { clock24, t } from "../i18n";
import { pick } from "../platform";
import { settings } from "../settings";
import type { Settings } from "../settings-model";
import { Card, Row, SelectBox, Segmented, Switch, ThemeCards } from "./controls";
import type { PrefsContext } from "./context";

type Props = { ctx: PrefsContext };

const toggle = (ctx: PrefsContext, key: keyof Settings) => ({
  checked: !!settings()[key],
  disabled: ctx.busy(),
  onChange: (value: boolean) => ctx.update(key, value as never),
});

const closeKeeps = () =>
  pick({
    mac: t("Closing the window keeps Flek in the menu bar"),
    windows: t("Closing the window keeps Flek in the system tray"),
    linux: t("Closing the window keeps Flek in the system tray"),
  });

export function GeneralSection(props: Props) {
  const ctx = props.ctx;
  return (
    <Card>
      <Row label={t("Language")} description={t("The language of Flek's menus and labels.")}>
        <Segmented
          label={t("Language")}
          value={settings().language}
          disabled={ctx.busy()}
          options={[
            ["system", t("System")],
            ["en", t("English")],
            ["uk", t("Українська")],
          ]}
          onChange={(v) => ctx.update("language", v)}
        />
      </Row>
      <Row
        label={t("Launch at login")}
        description={pick({
          mac: t("Start Flek automatically when you log in to your Mac."),
          windows: t("Start Flek automatically when you log in to Windows."),
          linux: t("Start Flek automatically when you log in to your computer."),
        })}
      >
        <Switch label={t("Launch at login")} {...toggle(ctx, "launch_at_login")} />
      </Row>
      <Row
        label={closeKeeps()}
        description={t("Flek keeps syncing in the background, so new messages arrive meanwhile.")}
      >
        <Switch
          label={closeKeeps()}
          {...toggle(ctx, "keep_in_menu_bar")}
        />
      </Row>
    </Card>
  );
}

export function AppearanceSection(props: Props) {
  const ctx = props.ctx;
  return (
    <>
      <Card>
        <Row
          stacked
          label={t("Theme")}
          description={pick({
            mac: t("Choose how Flek looks. System follows your Mac."),
            windows: t("Choose how Flek looks. System follows Windows."),
            linux: t("Choose how Flek looks. System follows your system."),
          })}
        >
          <ThemeCards
            label={t("Theme")}
            value={settings().theme}
            disabled={ctx.busy()}
            options={[
              ["system", t("System")],
              ["light", t("Light")],
              ["dark", t("Dark")],
            ]}
            onChange={(v) => ctx.update("theme", v)}
          />
        </Row>
      </Card>
      <Card>
        <Row
          label={t("Message text size")}
          description={t("Changes only the text in messages and the composer.")}
        >
          <Segmented
            label={t("Message text size")}
            value={settings().text_size}
            disabled={ctx.busy()}
            options={[
              ["small", t("Small")],
              ["default", t("Default")],
              ["large", t("Large")],
            ]}
            onChange={(v) => ctx.update("text_size", v)}
          />
        </Row>
        <Row
          label={t("Clock")}
          description={clock24() ? t("Using a 24-hour clock") : t("Using a 12-hour clock")}
        >
          <Segmented
            label={t("Clock")}
            value={settings().clock_24 === null ? "locale" : settings().clock_24 ? "true" : "false"}
            disabled={ctx.busy()}
            options={[
              ["locale", t("Language default")],
              ["false", t("12-hour")],
              ["true", t("24-hour")],
            ]}
            onChange={(v) => ctx.update("clock_24", v === "locale" ? null : v === "true")}
          />
        </Row>
      </Card>
    </>
  );
}

export function SidebarSection(props: Props) {
  const ctx = props.ctx;
  return (
    <Card>
      <Row
        label={t("Sort direct messages")}
        description={t("How the list of direct messages is ordered.")}
      >
        <Segmented
          label={t("Sort direct messages")}
          value={settings().dm_sort}
          disabled={ctx.busy()}
          options={[
            ["recent", t("Recent activity")],
            ["name", t("Name")],
          ]}
          onChange={(v) => ctx.update("dm_sort", v)}
        />
      </Row>
      <Row
        label={t("Show muted conversations in a collapsed group")}
        description={t("Muted conversations stay out of the way instead of mixing with the rest.")}
      >
        <Switch
          label={t("Show muted conversations in a collapsed group")}
          {...toggle(ctx, "show_muted")}
        />
      </Row>
      <Row
        label={t("Show presence dots in DMs")}
        description={t("A small dot next to a name shows whether the person is active.")}
      >
        <Switch label={t("Show presence dots in DMs")} {...toggle(ctx, "presence_dots")} />
      </Row>
    </Card>
  );
}

export function MediaSection(props: Props) {
  const ctx = props.ctx;
  return (
    <Card>
      <Row
        label={t("Show link previews")}
        description={t("Show a card with a title and picture under shared links.")}
      >
        <Switch label={t("Show link previews")} {...toggle(ctx, "link_previews")} />
      </Row>
      <Row
        label={t("Show image previews inline")}
        description={t("Show pictures inside messages instead of only a file name.")}
      >
        <Switch label={t("Show image previews inline")} {...toggle(ctx, "image_previews")} />
      </Row>
      <Row
        label={t("Auto-download files up to")}
        description={t("Download changes apply to the next sync pass.")}
      >
        <SelectBox
          label={t("Auto-download files up to")}
          value={String(settings().max_file_bytes)}
          disabled={ctx.busy()}
          options={[
            ["0", t("Off")],
            [String(10 << 20), t("10 MB")],
            [String(25 << 20), t("25 MB")],
            [String(100 << 20), t("100 MB")],
            ["null", t("No limit")],
          ]}
          onChange={(v) => ctx.update("max_file_bytes", v === "null" ? null : Number(v))}
        />
      </Row>
    </Card>
  );
}
