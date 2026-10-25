import type { ReactNode } from "react";

import mark from "@/assets/osdm-mark.png";

interface EmptyStateProps {
  title: string;
  body: string;
  action?: ReactNode;
}

/** An empty screen is an invitation to act, not a mood. */
export function EmptyState({ title, body, action }: EmptyStateProps) {
  return (
    <div className="max-w-[22rem] text-center">
      <div aria-hidden="true" className="mx-auto mb-4 w-14">
        <img
          src={mark}
          alt=""
          width={512}
          height={512}
          draggable={false}
          className="h-auto w-full select-none"
        />
      </div>
      <h2 className="font-display text-[18px] font-semibold tracking-[-0.025em] text-ink-strong">{title}</h2>
      <p className="mx-auto mt-2 max-w-[20rem] text-[12px] leading-relaxed text-muted-foreground">{body}</p>
      {action ? <div className="mt-5 flex justify-center">{action}</div> : null}
    </div>
  );
}
