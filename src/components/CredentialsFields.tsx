import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import type { BasicCredentials } from "@/lib/types";
import { useId } from "react";

export function CredentialsFields({
  value,
  onChange,
  disabled = false,
}: {
  value: BasicCredentials;
  onChange: (value: BasicCredentials) => void;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <fieldset className="flex flex-col gap-2 rounded-md border border-line-soft p-3">
      <legend className="px-1 text-xs">
        HTTP Basic authentication (optional)
      </legend>
      <p className="text-xs text-muted-foreground">
        HTTPS only. Used for this transfer and original origin only. Never saved
        to disk; enter again after restarting OSDM. Metadata inspection may fail
        until the authenticated transfer starts.
      </p>
      <Field label="Username" htmlFor={`${id}-username`}>
        <Input
          id={`${id}-username`}
          autoComplete="off"
          value={value.username}
          disabled={disabled}
          maxLength={256}
          onChange={(event) =>
            onChange({ ...value, username: event.target.value })
          }
        />
      </Field>
      <Field label="Password" htmlFor={`${id}-password`}>
        <Input
          id={`${id}-password`}
          type="password"
          autoComplete="new-password"
          value={value.password}
          disabled={disabled}
          maxLength={1024}
          onChange={(event) =>
            onChange({ ...value, password: event.target.value })
          }
        />
      </Field>
    </fieldset>
  );
}
