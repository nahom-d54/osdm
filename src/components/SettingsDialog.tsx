import { FolderOpen, Monitor, Moon, RefreshCw, Sun } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { IconButton } from "@/components/ui/icon-button";
import { Input } from "@/components/ui/input";
import { Modal } from "@/components/ui/modal";
import { NumberInput } from "@/components/ui/number-input";
import { Separator } from "@/components/ui/separator";
import { Toggle } from "@/components/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { getBridge } from "@/lib/bridge";
import type { ThemeChoice } from "@/store/settings";
import { DEFAULT_SETTINGS, useSettings } from "@/store/settings";
import { useEngine } from "@/store/engine";

export function SettingsDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { settings, update } = useSettings();
  const { applySpeedLimit } = useEngine();

  return (
    <Modal
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title="Settings"
      description="Stored on this machine. Speed changes apply to running transfers; concurrency changes apply after OSDM reopens."
      className="sm:max-w-[37rem]"
      footer={
        <div className="flex w-full items-center justify-between">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              update(DEFAULT_SETTINGS);
              void applySpeedLimit(DEFAULT_SETTINGS.speed_limit_bytes_per_second);
            }}
          >
            Reset to defaults
          </Button>
          <Button size="sm" onClick={onClose}>
            Done
          </Button>
        </div>
      }
    >
      <div className="flex flex-col gap-5">
        <section>
          <h3 className="eyebrow">Appearance</h3>
          <div className="mt-3 grid grid-cols-[auto_minmax(0,1fr)] items-start gap-4 max-[520px]:grid-cols-1">
            <Field label="Theme">
              <ToggleGroup
                aria-label="Theme"
                size="sm"
                value={[settings.theme]}
                onValueChange={(value) => {
                  if (value[0]) update({ theme: value[0] as ThemeChoice });
                }}
              >
                <ToggleGroupItem value="system" className="gap-1.5 px-2.5">
                  <Monitor strokeWidth={1.75} />
                  System
                </ToggleGroupItem>
                <ToggleGroupItem value="dark" className="gap-1.5 px-2.5">
                  <Moon strokeWidth={1.75} />
                  Dark
                </ToggleGroupItem>
                <ToggleGroupItem value="light" className="gap-1.5 px-2.5">
                  <Sun strokeWidth={1.75} />
                  Light
                </ToggleGroupItem>
              </ToggleGroup>
            </Field>

            <div className="min-w-0">
              <Field label="Default folder" htmlFor="settings-directory" hint="The starting point for new downloads.">
                <div className="relative">
                  <Input
                    id="settings-directory"
                    value={settings.default_directory}
                    placeholder="Not set"
                    onChange={(event) => update({ default_directory: event.target.value })}
                    className="pr-9 font-mono text-[11px]"
                  />
                  <IconButton
                    label="Choose default folder"
                    size="icon-xs"
                    className="absolute top-1/2 right-1 -translate-y-1/2 text-faint hover:text-foreground"
                    onClick={async () => {
                      const picked = await getBridge().pickDirectory(
                        "Choose default download folder",
                        settings.default_directory || undefined,
                      );
                      if (picked) update({ default_directory: picked });
                    }}
                  >
                    <FolderOpen strokeWidth={1.75} />
                  </IconButton>
                </div>
              </Field>
            </div>
          </div>
        </section>

        <Separator />

        <section>
          <div className="flex items-baseline justify-between gap-2">
            <h3 className="eyebrow">Transfer</h3>
          </div>
          <div className="mt-3 grid grid-cols-2 gap-4">
            <Field
              label="Speed limit"
              htmlFor="settings-speed"
              hint="Bytes per second. 0 means unlimited. Applies immediately."
            >
              <NumberInput
                id="settings-speed"
                value={settings.speed_limit_bytes_per_second}
                onValueChange={(value) => {
                  update({ speed_limit_bytes_per_second: value });
                  void applySpeedLimit(value);
                }}
                className="font-mono tnum"
              />
            </Field>
            <Field label="Retries" htmlFor="settings-retries" hint="Per transfer step; HTTP 429/503 keeps retrying automatically. Applies after a restart.">
              <NumberInput
                id="settings-retries"
                value={settings.max_retries}
                onValueChange={(value) => update({ max_retries: value })}
                className="font-mono tnum"
              />
            </Field>
          </div>
        </section>

        <Separator />

        <section>
          <div className="flex items-baseline justify-between gap-2">
            <h3 className="eyebrow">Connections</h3>
            <span className="font-mono text-[8.5px] uppercase tracking-[0.1em] text-warn">Restart</span>
          </div>
          <div className="mt-3 grid grid-cols-2 gap-4 sm:grid-cols-4">
            <Field label="Active" htmlFor="settings-active" hint="Downloads at once.">
              <NumberInput
                id="settings-active"
                value={settings.max_active_downloads}
                onValueChange={(value) => update({ max_active_downloads: value })}
                className="font-mono tnum"
              />
            </Field>
            <Field label="Per file" htmlFor="settings-per-download" hint="Maximum connections; adapts automatically.">
              <NumberInput
                id="settings-per-download"
                value={settings.connections_per_download}
                onValueChange={(value) => update({ connections_per_download: value })}
                className="font-mono tnum"
              />
            </Field>
            <Field label="Per host" htmlFor="settings-per-host" hint="Connections allowed.">
              <NumberInput
                id="settings-per-host"
                value={settings.max_connections_per_host}
                onValueChange={(value) => update({ max_connections_per_host: value })}
                className="font-mono tnum"
              />
            </Field>
            <Field label="Split above" htmlFor="settings-split" hint="MiB.">
              <NumberInput
                id="settings-split"
                value={settings.split_threshold_mib}
                onValueChange={(value) => update({ split_threshold_mib: value })}
                className="font-mono tnum"
              />
            </Field>
          </div>
          <p className="mt-3 rounded-md border border-warn/15 bg-warn-soft/35 px-3 py-2 text-[10.5px] leading-relaxed text-warn">
            Concurrency is fixed for the engine's lifetime. Changes apply after OSDM reopens.
          </p>
        </section>

        <Separator />

        <section>
          <div className="flex items-baseline justify-between gap-2">
            <h3 className="eyebrow">Updates</h3>
            <Button variant="ghost" size="sm" onClick={() => void getBridge().checkForUpdates()}>
              <RefreshCw strokeWidth={1.75} />
              Check now
            </Button>
          </div>
          <div className="mt-3 grid grid-cols-2 gap-4">
            <Field
              label="Automatic checks"
              hint="Check GitHub Releases after launch and every six hours."
            >
              <Toggle
                size="sm"
                pressed={settings.auto_update_check}
                onPressedChange={(pressed) => update({ auto_update_check: pressed })}
                aria-label="Automatic update checks"
              >
                {settings.auto_update_check ? "On" : "Off"}
              </Toggle>
            </Field>
            <Field label="Prereleases" hint="Include alpha and beta versions.">
              <Toggle
                size="sm"
                pressed={settings.include_prereleases}
                onPressedChange={(pressed) => update({ include_prereleases: pressed })}
                aria-label="Include prerelease updates"
              >
                {settings.include_prereleases ? "On" : "Off"}
              </Toggle>
            </Field>
          </div>
          <p className="mt-3 rounded-md border border-line-soft px-3 py-2 text-[10.5px] leading-relaxed text-muted-foreground">
            Updates are downloaded only after you confirm. Packages are verified against the
            release’s SHA256SUMS.txt. Windows updates install in place and restart OSDM.
          </p>
        </section>

        <div className="flex items-center justify-between pt-1 font-mono text-[8.5px] uppercase tracking-[0.1em] text-faint">
          <span>OSDM alpha</span>
          <span>Local engine · no telemetry</span>
        </div>
      </div>
    </Modal>
  );
}
