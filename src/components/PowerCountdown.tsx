import { useEffect, useState } from "react";
import { getBridge } from "@/lib/bridge";
import type { PowerPlan } from "@/lib/types";
import { Button } from "@/components/ui/button";
import { Modal } from "@/components/ui/modal";
import { useEngine } from "@/store/engine";

export function PowerCountdown() {
  const [plan, setPlan] = useState<PowerPlan | null>(null);
  const [error, setError] = useState("");
  const ready = useEngine((s) => s.ready);
  useEffect(() => {
    if (!ready || getBridge().kind !== "tauri") return;
    let disposed = false;
    const timer = setInterval(() => {
      void getBridge()
        .powerStatus()
        .then((state) => {
          if (!disposed) setPlan(state);
        })
        .catch(() => {});
    }, 500);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, [ready]);
  const cancel = async () => {
    try {
      await getBridge().cancelPower();
      setPlan(null);
      setError("");
    } catch {
      setError("Could not cancel. Retry before the countdown ends.");
    }
  };
  useEffect(() => {
    const timer = document.querySelector("[data-power-countdown]");
    if (
      plan?.deadline != null &&
      document.visibilityState === "visible" &&
      timer &&
      timer.getClientRects().length > 0 &&
      !timer.closest('[aria-hidden="true"], [inert]')
    ) {
      void getBridge()
        .acknowledgePower()
        .catch(() => {});
    }
  }, [plan]);
  return (
    <Modal
      open={plan?.deadline != null}
      onOpenChange={(open) => {
        if (!open) void cancel();
      }}
      title={
        plan?.action.kind === "shutdown"
          ? "Windows shutdown is armed"
          : "Connection disconnect is armed"
      }
      description="The selected queue finished. Cancel to keep this computer and connection as they are."
      footer={
        <Button onClick={() => void cancel()}>Cancel completion action</Button>
      }
    >
      {plan?.action.kind === "disconnect" && (
        <p className="mb-3 text-sm">Connection: {plan.action.connection}</p>
      )}
      <p role="timer" data-power-countdown className="font-mono text-3xl">
        {plan?.deadline == null
          ? ""
          : Math.max(0, plan.deadline - Math.floor(Date.now() / 1000))}{" "}
        seconds
      </p>
      <p className="mt-3 text-xs text-muted-foreground">
        Starting new download work cancels this countdown. Power actions are not
        remembered across restarts.
      </p>
      {error && <p role="alert">{error}</p>}
    </Modal>
  );
}
