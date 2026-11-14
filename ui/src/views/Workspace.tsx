import {
Show,
Switch,
Match
} from "solid-js";
import { t, errorMessage } from "../i18n/index";
import { pick } from "../platform";
import {
type Status
} from "../api";
import { closeProfile, profileUser } from "../stores/profile";
import ConversationMenu, {
LeaveConfirmation
} from "../components/ConversationMenu";
import ChannelPanel from "./ChannelPanel";
import ChannelBrowser from "./ChannelBrowser";
import CreateChannel from "./CreateChannel";
import ChannelsMenu from "../components/ChannelsMenu";
import { hasUnread } from "../models/conversation-model";
import ProfilePanel from "./ProfilePanel";
import Rail from "./Rail";
import ActivityView from "./ActivityView";
import DirectMessages from "./DirectMessages";
import FileBrowser, { FileNav } from "./FileBrowser";
import QuickSwitcher from "../components/QuickSwitcher";
import NewMessage from "./NewMessage";
import { SearchBar, SearchResults, scopeFilter } from "./Search";
import Viewer from "../components/Viewer";
import Sidebar from "./Sidebar";
import SyncBar from "../components/SyncBar";
import Thread from "./Thread";
import Timeline from "./Timeline";
import ResizeHandle from "../components/ResizeHandle";
import {
DEFAULT_SIDEBAR,
DEFAULT_DETAIL,
MIN_SIDEBAR,
MIN_DETAIL, savePanelSize
} from "../stores/panel-sizes";

import { useWorkspace } from "./workspace-state";

type Ready = Extract<Status, { phase: "ready" }>;

export default function Workspace(props: { status: Ready }) {
  const {
    conversations, view, icon, dms, activityBadge, revision, fileCategory, setFileCategory,
    switcher, setSwitcher, compose, setCompose, readError, actionError, setActionError, channelNotice,
    setChannelNotice, leaving, setLeaving, menu, setMenu, channelDetails, setChannelDetails,
    menuConversation, details, chooseView, browsing, setBrowsing, creatingChannel,
    setCreatingChannel, channelsMenu, setChannelsMenu, loaded, selected, progress, connection,
    change, searching, setSearching, searchRequest, setSearchRequest, jump, select, openHit, openJump, composeIn, openChannel,
    actionCompleted, joinChannel, browseChannels, current, thread, setThread, sidebarWidth,
    setSidebarWidth, detailWidth, setDetailWidth, frameWidth, sizes, detailOpen, SIDEBAR_KEY,
    DETAIL_KEY, setFrame,
  } = useWorkspace(props);
  return (
    <div class="shell">
      {/* The window has no title bar: this strip drags it, under the traffic lights. */}
      <header class="topbar" data-tauri-drag-region>
        <div class="topbar-side" data-tauri-drag-region />
        <SearchBar
          team={props.status.team}
          conversations={conversations}
          request={searchRequest()}
          onSubmit={setSearching}
          onOpen={openHit}
        />
        <div class="topbar-side end" data-tauri-drag-region>
          <SyncBar progress={progress()} connection={connection()} />
        </div>
      </header>
      <div class="shell-body">
        <Rail
          team={props.status.team}
          icon={icon()}
          userId={props.status.user_id}
          user={props.status.user}
          view={view()}
          dms={
            dms().filter((dm) =>
              hasUnread({
                ...dm,
                notification: conversations.find((c) => c.id === dm.id)?.notification,
              }),
            ).length
          }
          activity={activityBadge()}
          onView={chooseView}
          onCompose={() => setCompose(true)}
        />
        <div
          class="frame"
          classList={{
            "rail-view-frame": view() !== "home",
            "detail-overlay": sizes().overlay,
          }}
          ref={setFrame}
          style={{
            "--sidebar-width": `${sizes().sidebar}px`,
            "--detail-width": `${sizes().detail}px`,
          }}
        >
          <div class="sidebar-pane" id="conversation-sidebar">
            <Switch>
              <Match when={view() === "dms"}>
                <DirectMessages
                  rows={dms()}
                  mutedIds={conversations
                    .filter((c) => c.notification === "muted")
                    .map((c) => c.id)}
                  me={props.status.user_id}
                  selected={selected()}
                  onContext={setMenu}
                  onSelect={select}
                  onCompose={() => setCompose(true)}
                />
              </Match>
              <Match when={view() === "activity"}>
                <ActivityView
                  revision={revision()}
                  conversations={conversations}
                  me={props.status.user_id}
                  onOpen={openJump}
                />
              </Match>
              <Match when={view() === "files"}>
                <FileNav category={fileCategory()} onCategory={setFileCategory} />
              </Match>
              <Match when={view() === "home"}>
                <Sidebar
                  team={props.status.team}
                  conversations={conversations}
                  selected={selected()}
                  onContext={setMenu}
                  onChannelsMenu={setChannelsMenu}
                  onSelect={select}
                />
              </Match>
            </Switch>
            <ResizeHandle
              side="left"
              label={t("Resize sidebar")}
              controls="conversation-sidebar"
              value={sizes().sidebar}
              min={MIN_SIDEBAR}
              max={sizes().sidebarMax}
              initial={DEFAULT_SIDEBAR * frameWidth()}
              onResize={(px) => setSidebarWidth(px / frameWidth())}
              onCommit={() => savePanelSize(SIDEBAR_KEY, sidebarWidth())}
            />
          </div>
          <section class="main">
            <Show when={readError()}>
              <p class="error view-empty" role="alert">
                {errorMessage(readError())}
              </p>
            </Show>
            <Show when={!browsing()} fallback={
              <ChannelBrowser
                onOpen={openChannel}
                onJoin={joinChannel}
                onCreate={() => setCreatingChannel(true)}
                onClose={() => setBrowsing(false)}
              />
            }>
            <Show
              when={view() === "files" && !searching()}
              fallback={
                <Show
                  when={searching()}
                  fallback={
                    <Show
                      when={current()}
                      fallback={
                        <div class="empty">
                          <Show when={loaded()} fallback={<div class="spinner" />}>
                            <h2>{t("Syncing messages")}</h2>
                            <p class="muted">
                              {pick({
                                mac: t(
                                  "Channels appear here as soon as they are fetched. Flek keeps syncing in the menu bar even when this window is closed.",
                                ),
                                windows: t(
                                  "Channels appear here as soon as they are fetched. Flek keeps syncing in the system tray even when this window is closed.",
                                ),
                                linux: t(
                                  "Channels appear here as soon as they are fetched. Flek keeps syncing in the system tray even when this window is closed.",
                                ),
                              })}
                            </p>
                          </Show>
                        </div>
                      }
                    >
                      {(c) => (
                        <Timeline
                          conversation={c()}
                          onDetails={() => details(c())}
                          onSearch={() => setSearchRequest({ prefill: scopeFilter(c()) })}
                          onLeave={setLeaving}
                          onError={setActionError}
                          revision={revision()}
                          onJoin={joinChannel}
                          me={props.status.user_id}
                          members={dms().find((dm) => dm.id === c().id)?.members ?? []}
                          change={change()}
                          thread={thread()}
                          onThread={setThread}
                          jump={jump()}
                        />
                      )}
                    </Show>
                  }
                >
                  {(q) => (
                    <SearchResults
                      query={q()}
                      conversations={conversations}
                      onOpen={openHit}
                      onClose={() => setSearching(null)}
                    />
                  )}
                </Show>
              }
            >
              <FileBrowser
                category={fileCategory()}
                revision={revision()}
                conversations={conversations}
                onOpen={(target) => {
                  chooseView("home");
                  openJump(target);
                }}
              />
            </Show>
            </Show>
          </section>
          <Show when={view() !== "files" && detailOpen()}>
            <div class="detail-pane" id="conversation-detail">
              <ResizeHandle
                side="right"
                label={t("Resize details panel")}
                controls="conversation-detail"
                value={sizes().detail}
                min={MIN_DETAIL}
                max={sizes().detailMax}
                initial={DEFAULT_DETAIL * frameWidth()}
                onResize={(px) => setDetailWidth(px / frameWidth())}
                onCommit={() => savePanelSize(DETAIL_KEY, detailWidth())}
              />
              <Show when={channelDetails()}>
                {(id) => <ChannelPanel id={id()} onClose={() => setChannelDetails(null)} />}
              </Show>
              <Show when={current() && thread()}>
                {(ts) => (
                  <Thread
                    conversation={current()!}
                    ts={ts()}
                    jump={jump()}
                    change={change()}
                    onClose={() => setThread(null)}
                  />
                )}
              </Show>
              <Show when={profileUser()}>
                {(id) => <ProfilePanel userId={id()} onClose={closeProfile} onOpen={select} />}
              </Show>
            </div>
          </Show>
        </div>
      </div>
      <Show when={channelsMenu()}>
        {(position) => (
          <ChannelsMenu
            x={position().x}
            y={position().y}
            onClose={() => setChannelsMenu(null)}
            onBrowse={browseChannels}
            onCreate={() => setCreatingChannel(true)}
          />
        )}
      </Show>
      <Show when={creatingChannel()}>
        <CreateChannel
          onClose={() => setCreatingChannel(false)}
          onCreated={actionCompleted}
        />
      </Show>
      <Show when={switcher()}>
        <QuickSwitcher conversations={conversations} dms={dms()} onClose={() => setSwitcher(false)}
          onSelect={(id) => { chooseView("home"); select(id); }} />
      </Show>
      <Show when={compose()}>
        <NewMessage
          conversations={conversations}
          onSelect={composeIn}
          onClose={() => setCompose(false)}
        />
      </Show>
      <Show when={menu() && menuConversation()}>
        {(conversation) => (
          <ConversationMenu
            conversation={conversation()}
            x={menu()!.x}
            y={menu()!.y}
            onClose={() => setMenu(null)}
            onDetails={() => details(conversation())}
            onError={setActionError}
            onLeave={setLeaving}
          />
        )}
      </Show>
      <Show when={leaving()}>
        {(conversation) => (
          <LeaveConfirmation conversation={conversation()} onClose={() => setLeaving(null)} />
        )}
      </Show>
      <Show when={channelNotice()}>
        <div class="conversation-toast" role="status">
          {channelNotice() === "purpose_failed"
            ? t("Channel created, but its description could not be saved.")
            : t("Channel joined. Recent messages will sync when the connection recovers.")}
          <button aria-label={t("Dismiss error")} onClick={() => setChannelNotice(null)}>×</button>
        </div>
      </Show>
      <Show when={actionError()}>
        <div class="conversation-toast" role="alert">
          {errorMessage(actionError())}
          <button aria-label={t("Dismiss error")} onClick={() => setActionError("")}>
            ×
          </button>
        </div>
      </Show>
      <Viewer />
    </div>
  );
}
