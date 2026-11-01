import { useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { ErrorNote, Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { DEFAULT_USER_AGENT, useSettings } from "@/store/settings";

const INVALID = /[\u0000-\u001f\u007f]/;

/** Draft-then-save so an in-progress header value is never persisted. */
export function BrowserIdentitySettings() {
  const { settings, update } = useSettings();
  const [draft, setDraft] = useState(settings.user_agent);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { setDraft(settings.user_agent); }, [settings.user_agent]);
  const invalid = draft.trim() === "" || draft.length > 512 || INVALID.test(draft);
  const dirty = draft !== settings.user_agent;

  const save = () => {
    if (invalid) {
      setError("Use a single-line user agent of 1–512 visible characters.");
      return;
    }
    setError(null);
    update({ user_agent: draft });
  };

  return (
    <section>
      <div className="flex items-baseline justify-between gap-2">
        <h3 className="eyebrow">Browser identity</h3>
        <span className="font-mono text-[8.5px] uppercase tracking-[0.1em] text-muted-foreground">Downloads</span>
      </div>
      <Field
        label="User agent"
        htmlFor="settings-user-agent"
        hint="Used for ordinary downloads and media requests. Browser-captured downloads keep the browser's own user agent and replayable headers for the original site."
      >
        <Input
          id="settings-user-agent"
          value={draft}
          spellCheck={false}
          aria-invalid={invalid}
          className="font-mono text-xs"
          onChange={(event) => { setDraft(event.target.value); setError(null); }}
          onKeyDown={(event) => { if (event.key === "Enter") save(); }}
        />
      </Field>
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <Button size="sm" disabled={!dirty || invalid} onClick={save}>Save user agent</Button>
        <Button
          size="sm"
          variant="ghost"
          disabled={draft === DEFAULT_USER_AGENT}
          onClick={() => { setDraft(DEFAULT_USER_AGENT); setError(null); update({ user_agent: DEFAULT_USER_AGENT }); }}
        >
          Use default browser user agent
        </Button>
      </div>
      {error && <div className="mt-2"><ErrorNote>{error}</ErrorNote></div>}
    </section>
  );
}
