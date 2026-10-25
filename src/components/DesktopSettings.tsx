import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Field, ErrorNote } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { getBridge } from "@/lib/bridge";
import { useSettings, type DesktopPreferences } from "@/store/settings";

export function DesktopSettings() {
  const { settings, update } = useSettings();
  const [error, setError] = useState<string | null>(null);
  const [argumentsText, setArgumentsText] = useState(
    JSON.stringify(settings.desktop.scanner_arguments),
  );
  const [soundPath, setSoundPath] = useState(settings.desktop.sound_file);
  const [scannerPath, setScannerPath] = useState(
    settings.desktop.scanner_executable,
  );
  const [checking, setChecking] = useState(false);
  const [connection, setConnection] = useState("");
  const [checkUrl, setCheckUrl] = useState("");
  const change = (patch: Partial<DesktopPreferences>) =>
    update({
      desktop: { ...useSettings.getState().settings.desktop, ...patch },
    });
  const switches = [
    ["silent", "Silent mode (no sound, notification, or completion dialog)"],
    ["completion_dialog", "Show completion dialog"],
    ["progress_dialog", "Show progress dialog after adding a download"],
    ["minimized_progress", "Use minimized progress panel"],
    ["confirm_downloads", "Confirm filename and destination before starting"],
  ] as const;
  return (
    <section className="flex flex-col gap-3">
      <h3 className="eyebrow">Desktop behavior</h3>
      <Field
        label="Temporary storage folder"
        htmlFor="desktop-temporary"
        hint="Blank keeps partials next to their destination. A custom folder must be on the destination volume and support hard links. Applies to new downloads only."
      >
        <Input
          id="desktop-temporary"
          readOnly
          value={settings.temporary_directory}
          placeholder="Same as destination"
        />
      </Field>
      <div className="flex gap-2">
        <Button
          size="sm"
          variant="secondary"
          onClick={async () => {
            try {
              const directory = await getBridge().pickDirectory(
                "Choose temporary storage",
                settings.temporary_directory || undefined,
              );
              if (directory) update({ temporary_directory: directory });
              setError(null);
            } catch (caught) {
              setError(String(caught));
            }
          }}
        >
          Choose temporary folder
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => update({ temporary_directory: "" })}
        >
          Use destination folder
        </Button>
      </div>
      {switches.map(([key, label]) => (
        <label key={key} className="flex items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={settings.desktop[key]}
            onChange={(event) => change({ [key]: event.target.checked })}
          />
          {label}
        </label>
      ))}
      <p className="text-xs text-muted-foreground">
        Resume uses a matching strong ETag in preference to modification time.
        Date-only or incompatible sources restart safely; there is no unsafe
        validator bypass.
      </p>
      <Field
        label="Completion sound (Windows WAV, up to 10 MiB)"
        htmlFor="desktop-sound"
      >
        <Input
          id="desktop-sound"
          value={soundPath}
          placeholder="Absolute WAV path; blank disables sound"
          onChange={(event) => setSoundPath(event.target.value)}
          onBlur={() => {
            if (soundPath && !/^(?:[a-z]:[\\/]|\\\\|\/)/i.test(soundPath)) {
              setError("Choose an absolute sound path.");
              return;
            }
            change({ sound_file: soundPath });
            setError(null);
          }}
        />
      </Field>
      <Button
        size="sm"
        variant="secondary"
        disabled={!settings.desktop.sound_file}
        onClick={async () => {
          try {
            await getBridge().testSound(settings.desktop.sound_file);
            setError(null);
          } catch (caught) {
            setError(String(caught));
          }
        }}
      >
        Test sound
      </Button>
      <Field
        label="Antivirus scanner executable"
        htmlFor="desktop-scanner"
        hint="Optional external program. OSDM does not certify files malware-free."
      >
        <Input
          id="desktop-scanner"
          disabled={settings.desktop.scanner_enabled}
          value={scannerPath}
          placeholder="Absolute executable path"
          onChange={(event) => setScannerPath(event.target.value)}
          onBlur={() => {
            if (scannerPath && !/^(?:[a-z]:[\\/]|\\\\|\/)/i.test(scannerPath)) {
              setError("Choose an absolute scanner executable path.");
              return;
            }
            change({ scanner_executable: scannerPath });
            setError(null);
          }}
        />
      </Field>
      <Field
        label="Scanner arguments (JSON string array)"
        htmlFor="desktop-scanner-args"
        hint={
          'Use an exact "{file}" argument for the completed filename. No shell is invoked. Disable scanning before editing.'
        }
      >
        <Input
          id="desktop-scanner-args"
          disabled={settings.desktop.scanner_enabled}
          value={argumentsText}
          onChange={(event) => setArgumentsText(event.target.value)}
          onBlur={() => {
            try {
              const args: unknown = JSON.parse(argumentsText);
              if (
                !Array.isArray(args) ||
                !args.every((arg) => typeof arg === "string") ||
                !args.includes("{file}")
              )
                throw new Error(
                  'Arguments must be a JSON string array containing "{file}".',
                );
              change({ scanner_arguments: args });
              setError(null);
            } catch (caught) {
              setError(String(caught));
            }
          }}
        />
      </Field>
      <label className="flex items-center gap-2 text-xs">
        <input
          type="checkbox"
          checked={settings.desktop.scanner_enabled}
          disabled={
            !settings.desktop.scanner_enabled &&
            (!settings.desktop.scanner_executable || !!error)
          }
          onChange={(event) =>
            change({ scanner_enabled: event.target.checked })
          }
        />
        Run this scanner on newly completed downloads (five-minute timeout)
      </label>
      <h3 className="eyebrow mt-2">Connection check</h3>
      <Field
        label="Test a download URL"
        htmlFor="connection-url"
        hint="OSDM sends a bounded metadata request to the URL you supply; nothing is enqueued."
      >
        <Input
          id="connection-url"
          value={checkUrl}
          onChange={(event) => setCheckUrl(event.target.value)}
          placeholder="https://…"
        />
      </Field>
      <Button
        size="sm"
        variant="secondary"
        disabled={checking || !/^https?:\/\//i.test(checkUrl)}
        onClick={async () => {
          setChecking(true);
          setConnection("");
          try {
            const probe = await getBridge().probe(checkUrl);
            setConnection(
              `Connected. ${probe.ranged ? "Byte ranges supported." : "Single-stream source."}`,
            );
          } catch {
            setConnection(
              "Connection failed. Check the URL, network, and server access requirements.",
            );
          } finally {
            setChecking(false);
          }
        }}
      >
        {checking ? "Checking…" : "Check connection"}
      </Button>
      {connection && (
        <p role="status" className="text-xs">
          {connection}
        </p>
      )}
      {error && <ErrorNote>{error}</ErrorNote>}
    </section>
  );
}
