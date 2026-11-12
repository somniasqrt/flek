import { api } from "../api";
import { slackDeepLink } from "../models/slack-link";
import { t } from "../i18n/index";

export async function storedSlackLink(conversation: string, ts?: string) {
  const status = await api.status();
  if (status.phase !== "ready") throw new Error(t("Workspace is not ready"));
  return slackDeepLink(status.team_id, conversation, ts);
}
export async function openInSlack(conversation: string, ts?: string) {
  await api.openUrl(await storedSlackLink(conversation, ts));
}
