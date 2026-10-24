import type { ThemeChoice } from "@/store/settings";
import { DEFAULT_SETTINGS, useSettings } from "@/store/settings";
import { useEngine } from "@/store/engine";
import { Dialog } from "@/components/ui/Dialog";
import { Button } from "@/components/ui/Button";
import { Field, TextField } from "@/components/ui/TextField";

export function SettingsDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { settings, update } = useSettings();
  const { applySpeedLimit } = useEngine();

  const setNumber = (field: keyof typeof settings, value: string) => {
    const n = Math.floor(Number(value));
    if (!Number.isFinite(n) || n < 0) return;
    update({ [field]: n });
  };

  return (
    <Dialog open={open} title="Settings" onClose={onClose} width="30rem">
      <div className="flex flex-col gap-5 px-5 py-4">
        <section className="grid grid-cols-2 gap-3">
          <Field label="Theme">
            <ThemeSelect value={settings.theme} onChange={(theme) => update({ theme })} />
          </Field>
          <Field label="Default folder" hint="Used as the destination picker starting point.">
            <TextField
              value={settings.default_directory}
              placeholder="Not set"
              onChange={(event) => update({ default_directory: event.target.value })}
            />
          </Field>
        </section>

        <section>
          <h3 className="mb-2 text-[10px] font-semibold tracking-[0.08em] text-faint uppercase">Limit</h3>
          <div className="grid grid-cols-2 gap-3">
            <Field
              label="Speed limit (bytes/s)"
              hint="0 disables the limit. Applied immediately, mid-transfer."
            >
              <TextField
                inputMode="numeric"
                value={String(settings.speed_limit_bytes_per_second)}
                onChange={(event) => setNumber("speed_limit_bytes_per_second", event.target.value)}
              />
            </Field>
            <Field label="Retries" hint="Per transfer step, with backoff.">
              <TextField
                inputMode="numeric"
                value={String(settings.max_retries)}
                onChange={(event) => setNumber("max_retries", event.target.value)}
              />
            </Field>
          </div>
        </section>

        <section>
          <h3 className="mb-2 text-[10px] font-semibold tracking-[0.08em] text-faint uppercase">
            Connections
          </h3>
          <p className="mb-3 rounded-[var(--radius-control)] bg-warn-soft px-2.5 py-2 text-[11.5px] leading-relaxed text-warn">
            These are fixed for the engine's lifetime. Changes apply after the app reopens.
          </p>
          <div className="grid grid-cols-3 gap-3">
            <Field label="Active downloads">
              <TextField
                inputMode="numeric"
                value={String(settings.max_active_downloads)}
                onChange={(event) => setNumber("max_active_downloads", event.target.value)}
              />
            </Field>
            <Field label="Per download">
              <TextField
                inputMode="numeric"
                value={String(settings.connections_per_download)}
                onChange={(event) => setNumber("connections_per_download", event.target.value)}
              />
            </Field>
            <Field label="Per host">
              <TextField
                inputMode="numeric"
                value={String(settings.max_connections_per_host)}
                onChange={(event) => setNumber("max_connections_per_host", event.target.value)}
              />
            </Field>
          </div>
        </section>

        <div className="flex items-center justify-between border-t border-line-soft pt-3">
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              update(DEFAULT_SETTINGS);
              void applySpeedLimit(DEFAULT_SETTINGS.speed_limit_bytes_per_second);
            }}
          >
            Reset to defaults
          </Button>
          <Button variant="primary" onClick={onClose}>
            Close
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

function ThemeSelect({ value, onChange }: { value: ThemeChoice; onChange: (theme: ThemeChoice) => void }) {
  return (
    <select
      value={value}
      onChange={(event) => onChange(event.target.value as ThemeChoice)}
      className="h-9 w-full rounded-[var(--radius-control)] border border-line bg-raise px-2 text-[13px] text-ink focus:outline-none focus-visible:border-accent"
    >
      <option value="system">Match system</option>
      <option value="dark">Dark</option>
      <option value="light">Light</option>
    </select>
  );
}
