import { create } from "zustand";
import { persist } from "zustand/middleware";

export type ThemeChoice = "system" | "dark" | "light";

export interface AppSettings {
  theme: ThemeChoice;
  /** Zero means unlimited. Mirrors Config.bytes_per_second. */
  speed_limit_bytes_per_second: number;
  /** Concurrency values are fixed for the engine's lifetime (PLAN 3.3):
   *  changing them requires reopening the engine, so they apply on restart. */
  max_active_downloads: number;
  connections_per_download: number;
  max_connections: number;
  max_connections_per_host: number;
  split_threshold_mib: number;
  max_retries: number;
  /** Last-used download directory for the destination picker default. */
  default_directory: string;
  /** Check GitHub Releases after launch and every six hours. */
  auto_update_check: boolean;
  /** Include alpha/beta GitHub releases in update checks. */
  include_prereleases: boolean;
}

export const DEFAULT_SETTINGS: AppSettings = {
  theme: "dark",
  speed_limit_bytes_per_second: 0,
  max_active_downloads: 3,
  connections_per_download: 4,
  max_connections: 12,
  max_connections_per_host: 8,
  split_threshold_mib: 16,
  max_retries: 3,
  default_directory: "",
  auto_update_check: true,
  include_prereleases: true,
};

interface SettingsStore {
  settings: AppSettings;
  update(patch: Partial<AppSettings>): void;
}

export const useSettings = create<SettingsStore>()(
  persist(
    (set) => ({
      settings: DEFAULT_SETTINGS,
      update: (patch) => set((state) => ({ settings: { ...state.settings, ...patch } })),
    }),
    {
      name: "osdm.settings",
      // Older persisted settings predate update fields; merge so upgrades keep
      // their choices while receiving safe defaults for new ones.
      merge: (persisted, current) => {
        const saved = (persisted as { settings?: Partial<AppSettings> } | undefined)?.settings;
        return { ...current, settings: { ...current.settings, ...(saved ?? {}) } };
      },
    },
  ),
);
