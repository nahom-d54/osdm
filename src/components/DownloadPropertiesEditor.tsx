import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Field, ErrorNote } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { getBridge } from "@/lib/bridge";
import type { Download } from "@/lib/types";
import { useEngine } from "@/store/engine";
import { useSettings } from "@/store/settings";
import { CredentialsFields } from "@/components/CredentialsFields";

export function DownloadPropertiesEditor({ download }: { download: Download }) {
  const categories = useSettings((state) => state.settings.categories);
  const [description, setDescription] = useState(
    download.properties?.description ?? "",
  );
  const [category, setCategory] = useState(
    download.properties?.category_id ?? "",
  );
  const [destination, setDestination] = useState(download.destination);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [credentials, setCredentials] = useState({
    username: "",
    password: "",
  });
  const editableDestination = ["Paused", "Failed", "Canceled"].includes(
    download.state,
  );
  return (
    <form
      className="flex flex-col gap-3 border-b border-line-soft px-4 py-4"
      aria-busy={busy}
      onSubmit={async (event) => {
        event.preventDefault();
        setBusy(true);
        setError(null);
        setSaved(false);
        try {
          const bridge = getBridge();
          // Separate commands intentionally report partial success through a refresh.
          if (destination !== download.destination)
            await bridge.updateDestination(download.id, destination);
          await bridge.updateProperties(download.id, {
            description,
            category_id: category || null,
          });
          setSaved(true);
        } catch (caught) {
          setError(String(caught));
        } finally {
          try {
            useEngine.setState({ downloads: await getBridge().snapshot() });
          } catch {
            /* engine error surface handles reconnect */
          }
          setBusy(false);
        }
      }}
    >
      <h3 className="eyebrow">Properties</h3>
      <Field label="Description" htmlFor="properties-description">
        <Input
          id="properties-description"
          value={description}
          maxLength={1024}
          disabled={busy}
          onChange={(event) => {
            setDescription(event.target.value);
            setSaved(false);
          }}
        />
      </Field>
      <Field
        label="Category"
        htmlFor="properties-category"
        hint="Changing category does not move an existing file."
      >
        <select
          id="properties-category"
          className="rounded-md border border-line-soft bg-background p-2 text-sm"
          value={category}
          disabled={busy}
          onChange={(event) => {
            setCategory(event.target.value);
            setSaved(false);
          }}
        >
          <option value="">General</option>
          {category && !categories.some((item) => item.id === category) && (
            <option value={category}>Deleted category</option>
          )}
          {categories.map((item) => (
            <option value={item.id} key={item.id}>
              {item.name}
            </option>
          ))}
        </select>
      </Field>
      <Field
        label="Destination"
        htmlFor="properties-destination"
        hint="Pause first. After partial data exists, only the filename within its folder can change."
      >
        <Input
          id="properties-destination"
          value={destination}
          disabled={busy || !editableDestination}
          onChange={(event) => {
            setDestination(event.target.value);
            setSaved(false);
          }}
          className="font-mono text-xs"
        />
      </Field>
      <Button size="sm" type="submit" disabled={busy}>
        {busy ? "Saving…" : "Save properties"}
      </Button>
      {["Paused", "Failed"].includes(download.state) && (
        <>
          <CredentialsFields
            value={credentials}
            onChange={setCredentials}
            disabled={busy}
          />
          <Button
            size="sm"
            variant="secondary"
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              setError(null);
              try {
                await getBridge().setCredentials(
                  download.id,
                  credentials.username || credentials.password
                    ? credentials
                    : null,
                );
                setCredentials({ username: "", password: "" });
                setSaved(true);
              } catch (caught) {
                setError(String(caught));
              } finally {
                setBusy(false);
              }
            }}
          >
            Apply credentials (blank clears)
          </Button>
        </>
      )}
      {error && <ErrorNote>{error}</ErrorNote>}
      {saved && (
        <p role="status" className="text-xs text-ok">
          Properties saved.
        </p>
      )}
    </form>
  );
}
