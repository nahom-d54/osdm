import { useEffect, useState } from "react";

import { Input } from "@/components/ui/input";

interface NumberInputProps {
  id?: string;
  value: number;
  onValueChange: (value: number) => void;
  className?: string;
}

/** A numeric field that can be emptied while typing and restores on blur. */
export function NumberInput({ id, value, onValueChange, className }: NumberInputProps) {
  const [draft, setDraft] = useState(String(value));

  useEffect(() => {
    if (Number(draft) !== value) setDraft(String(value));
    // Only external value changes should rewrite the draft.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value]);

  return (
    <Input
      id={id}
      inputMode="numeric"
      value={draft}
      onChange={(event) => {
        const next = event.target.value;
        if (next !== "" && !/^\d+$/.test(next)) return;
        setDraft(next);
        if (next !== "") onValueChange(Math.floor(Number(next)));
      }}
      onBlur={() => {
        if (draft.trim() === "") setDraft(String(value));
      }}
      className={className}
    />
  );
}
