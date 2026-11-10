import type { Resource } from "solid-js";
import type { Status, StorageInfo } from "../../api";
import type { Settings } from "../../models/settings-model";

// What the dialog shares with its sections.
export type Confirmable = "cache" | "sync" | "signout";
export type PrefsContext = {
  busy: () => boolean;
  update: <K extends keyof Settings>(key: K, value: Settings[K]) => Promise<void>;
  run: (action: () => Promise<unknown>) => Promise<void>;
  ask: (action: Confirmable) => void;
  ready: () => Extract<Status, { phase: "ready" }> | null;
  storage: Resource<StorageInfo>;
};
