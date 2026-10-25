import { useState } from "react";
import { Modal } from "@/components/ui/modal";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { getBridge } from "@/lib/bridge";
import { useEngine } from "@/store/engine";
import { useSettings } from "@/store/settings";

export function OnboardingDialog() {
  const ready = useEngine((state) => state.ready);
  const { settings, update } = useSettings();
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState("");
  return (
    <Modal
      open={ready && !settings.onboarding_complete}
      onOpenChange={(open) => {
        if (!open && !busy) update({ onboarding_complete: true });
      }}
      title="Welcome to OSDM"
      description="Downloads and learning stay on this machine. Existing destination files are never overwritten."
      footer={
        <Button
          disabled={busy}
          onClick={() => update({ onboarding_complete: true })}
        >
          {result ? "Done" : "Skip connection check"}
        </Button>
      }
    >
      <div className="flex flex-col gap-3">
        <p className="text-sm">
          Optional: check a direct download link before your first transfer. No
          download is enqueued; the server receives a metadata request only.
        </p>
        <Field label="Test URL" htmlFor="onboarding-url">
          <Input
            id="onboarding-url"
            value={url}
            disabled={busy}
            onChange={(event) => setUrl(event.target.value)}
            placeholder="https://…"
          />
        </Field>
        <Button
          disabled={busy || !/^https?:\/\//i.test(url)}
          variant="secondary"
          onClick={async () => {
            setBusy(true);
            setResult("");
            try {
              await getBridge().probe(url.trim());
              setResult(
                "Connection succeeded. You can start adding downloads.",
              );
            } catch {
              setResult(
                "Connection failed. Check your network and URL. Authenticated sources may require credentials when adding the download.",
              );
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? "Checking…" : "Check connection"}
        </Button>
        {result && (
          <p className="text-xs" role="status">
            {result}
          </p>
        )}
        <p className="text-xs text-muted-foreground">
          Choose an output folder and configure categories, completion behavior,
          and concurrency in Settings. You can repeat this check there.
        </p>
      </div>
    </Modal>
  );
}
