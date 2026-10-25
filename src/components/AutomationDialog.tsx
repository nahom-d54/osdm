import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ErrorNote, Field } from "@/components/ui/field";
import { Modal } from "@/components/ui/modal";
import { getBridge } from "@/lib/bridge";
import { captureFilename, captureUrls } from "@/lib/capture";
import { joinPath } from "@/lib/format";
import type { Automation, AutomationCommand, BatchResult } from "@/lib/types";
import { useEngine } from "@/store/engine";
import { useSettings } from "@/store/settings";

const selectClass =
  "h-9 w-full rounded-md border border-line-soft bg-background px-2 text-xs focus-visible:outline-2 focus-visible:outline-brand";

/** A compact dispatch ledger: input, destination, then persistent queue windows. */
export function AutomationDialog({
  open,
  onClose,
  initialText = "",
}: {
  open: boolean;
  onClose(): void;
  initialText?: string;
}) {
  const [automation, setAutomation] = useState<Automation>({
    queues: [],
    schedules: [],
    membership: {},
  });
  const [text, setText] = useState("");
  const [directory, setDirectory] = useState("");
  const [queueId, setQueueId] = useState("");
  const [name, setName] = useState("");
  const [when, setWhen] = useState("");
  const [interval, setIntervalHours] = useState("0");
  const [windowMinutes, setWindowMinutes] = useState("0");
  const [catchUp, setCatchUp] = useState(false);
  const [results, setResults] = useState<BatchResult[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [captureEnabled, setCaptureEnabled] = useState(false);
  const [powerConsent, setPowerConsent] = useState(false);
  const [powerAction, setPowerAction] = useState("shutdown");
  const [connection, setConnection] = useState("");
  const [powerArmed, setPowerArmed] = useState(false);
  const [autostart, setAutostart] = useState(false);
  const checkedIds = useEngine((s) => s.checkedIds);
  const downloads = useEngine((s) => s.downloads);
  const settings = useSettings((s) => s.settings);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setText(initialText);
    setResults([]);
    setError(null);
    void getBridge()
      .captureStatus()
      .then((enabled) => {
        if (!cancelled) setCaptureEnabled(enabled);
      })
      .catch(() => {});
    void getBridge()
      .autostartStatus()
      .then((enabled) => {
        if (!cancelled) setAutostart(enabled);
      })
      .catch(() => {});
    void getBridge()
      .powerStatus()
      .then((plan) => {
        if (!cancelled) setPowerArmed(!!plan);
      })
      .catch(() => {});
    void getBridge()
      .automation()
      .then((state) => {
        if (!cancelled) setAutomation(state);
      })
      .catch(() => {
        if (!cancelled)
          setError("Could not load queues. Reopen this dialog to retry.");
      });
    void (
      settings.default_directory
        ? Promise.resolve(settings.default_directory)
        : getBridge().defaultDownloadDirectory()
    )
      .then((path) => {
        if (!cancelled) setDirectory(path);
      })
      .catch(() => {
        if (!cancelled)
          setError(
            "Choose an existing destination folder before importing links.",
          );
      });
    return () => {
      cancelled = true;
    };
  }, [open, initialText, settings.default_directory]);

  useEffect(() => {
    if (!open) return;
    const timer = window.setInterval(() => {
      void getBridge()
        .automation()
        .then(setAutomation)
        .catch(() => {});
      void getBridge()
        .powerStatus()
        .then((plan) => setPowerArmed(!!plan))
        .catch(() => {});
    }, 2000);
    return () => clearInterval(timer);
  }, [open]);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await action();
      useEngine.setState({
        downloads: await getBridge().snapshot(),
        automation: await getBridge().automation(),
      });
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : "Action failed. Safe progress is retained.",
      );
    } finally {
      setBusy(false);
    }
  }
  async function command(command: AutomationCommand) {
    const state = await getBridge().configureAutomation(command);
    setAutomation(state);
    const selectedQueue = useEngine.getState().queueFilter;
    useEngine.setState({
      automation: state,
      ...(selectedQueue && !state.queues.some((q) => q.id === selectedQueue)
        ? { queueFilter: null }
        : {}),
    });
  }
  async function submit(paused: boolean) {
    await run(async () => {
      const urls = captureUrls(text);
      if (!urls.length || !directory.trim())
        throw new Error(
          "Enter links and choose an existing destination folder.",
        );
      const items = urls.map((url) => ({
        request: {
          url,
          destination: joinPath(directory, captureFilename(url)),
        },
        queue_id: queueId || null,
      }));
      const result = await getBridge().enqueueBatch(items, paused);
      setResults(result);
      if (result.some((r) => r.error))
        setError(
          "Some links could not be admitted. Successful entries are saved; retrying reuses their IDs.",
        );
    });
  }

  return (
    <Modal
      open={open}
      onOpenChange={(value) => {
        if (!value && !busy) onClose();
      }}
      title="Capture & queues"
      description="Import links, save deferred work, and set unattended transfer windows."
      className="sm:max-w-[48rem]"
      footer={
        <Button onClick={onClose} disabled={busy}>
          Done
        </Button>
      }
    >
      <div className="space-y-6 text-xs">
        <section aria-label="Browser integration" className="space-y-2">
          <h2 className="eyebrow">Browser capture</h2>
          <p className="text-muted-foreground">
            Install the Chromium or Firefox extension and register
            osdm-native-host before enabling. Automatic capture sends no
            cookies; explicit cookie handoff is scoped to one requested
            transfer.
          </p>
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={captureEnabled}
              disabled={busy || getBridge().kind !== "tauri"}
              onChange={(e) => {
                const enabled = e.target.checked;
                void run(async () => {
                  await getBridge().enableCapture(enabled);
                  setCaptureEnabled(enabled);
                });
              }}
            />
            Enable local browser integration
          </label>
        </section>
        <section className="space-y-3" aria-label="Batch capture">
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={autostart}
              disabled={busy || getBridge().kind !== "tauri"}
              onChange={(e) => {
                const enabled = e.target.checked;
                void run(async () => {
                  await getBridge().setAutostart(enabled);
                  setAutostart(enabled);
                });
              }}
            />
            Start OSDM with Windows · needed for unattended schedules after
            sign-in
          </label>
          <h2 className="eyebrow">Batch capture</h2>
          <Field label="Links · one per line">
            <textarea
              aria-label="Batch links"
              className="min-h-28 w-full rounded-md border border-line-soft bg-background p-3 font-mono text-xs focus-visible:outline-2 focus-visible:outline-brand"
              value={text}
              onChange={(e) => {
                setText(e.target.value);
                setResults([]);
              }}
            />
          </Field>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="outline"
              disabled={busy}
              onClick={() =>
                void run(async () =>
                  setText(await navigator.clipboard.readText()),
                )
              }
            >
              Paste clipboard
            </Button>
            <label className="inline-flex cursor-pointer items-center rounded-md border border-line-soft px-3 py-2">
              Import link file
              <input
                aria-label="Import link file"
                type="file"
                accept=".txt,.urls,text/plain"
                className="ml-2 max-w-48"
                disabled={busy}
                onChange={(e) => {
                  const file = e.target.files?.[0];
                  if (file)
                    void run(async () => {
                      if (file.size > 2 * 1024 * 1024)
                        throw new Error("Link files must be at most 2 MiB.");
                      setText(await file.text());
                    });
                  e.target.value = "";
                }}
              />
            </label>
          </div>
          <Field label="Destination folder">
            <div className="flex gap-2">
              <Input
                aria-label="Batch destination folder"
                value={directory}
                onChange={(e) => setDirectory(e.target.value)}
              />
              <Button
                variant="outline"
                onClick={() =>
                  void run(async () => {
                    const path = await getBridge().pickDirectory(
                      "Batch destination",
                      directory,
                    );
                    if (path) setDirectory(path);
                  })
                }
              >
                Browse
              </Button>
            </div>
          </Field>
          <Field label="Queue">
            <select
              aria-label="Batch queue"
              className={selectClass}
              value={queueId}
              onChange={(e) => setQueueId(e.target.value)}
            >
              <option value="">General</option>
              {automation.queues.map((q) => (
                <option key={q.id} value={q.id}>
                  {q.name}
                </option>
              ))}
            </select>
          </Field>
          <div className="flex gap-2">
            <Button disabled={busy} onClick={() => void submit(false)}>
              Start now
            </Button>
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => void submit(true)}
            >
              Save for later
            </Button>
          </div>
          {results.length > 0 && (
            <div
              role="status"
              className="rounded-md border border-line-soft p-3 font-mono"
            >
              {results.filter((r) => !r.error).length} saved ·{" "}
              {results.filter((r) => r.duplicate).length} existing ·{" "}
              {results.filter((r) => r.error).length} failed
            </div>
          )}
        </section>
        <section
          aria-label="Named queues"
          className="space-y-3 border-t border-line-soft pt-4"
        >
          <h2 className="eyebrow">Named queues</h2>
          <div className="flex gap-2">
            <Input
              aria-label="Queue name"
              placeholder="Overnight, documents…"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
            <Button
              disabled={busy || !name.trim()}
              onClick={() =>
                void run(async () => {
                  const id = crypto.randomUUID();
                  await command({ action: "save_queue", id, name });
                  setQueueId(id);
                  setName("");
                })
              }
            >
              Create queue
            </Button>
          </div>
          {automation.queues.map((q) => (
            <div
              key={q.id}
              className="flex flex-wrap items-center gap-2 rounded-md border border-line-soft p-3"
            >
              <span className="min-w-24 flex-1 font-medium">
                {q.name}{" "}
                <span className="text-faint">
                  ·{" "}
                  {
                    Object.values(automation.membership).filter(
                      (id) => id === q.id,
                    ).length
                  }{" "}
                  files · {q.running ? "running" : "stopped"}
                </span>
              </span>
              <Button
                variant="outline"
                disabled={busy}
                onClick={() =>
                  void run(() => command({ action: "start", id: q.id }))
                }
              >
                Start queue
              </Button>
              <Button
                variant="outline"
                disabled={busy}
                onClick={() =>
                  void run(() => command({ action: "stop", id: q.id }))
                }
              >
                Stop queue
              </Button>
              <Button
                variant="outline"
                disabled={busy || !name.trim()}
                onClick={() =>
                  void run(() =>
                    command({ action: "save_queue", id: q.id, name }),
                  )
                }
              >
                Rename
              </Button>
              <Button
                variant="outline"
                disabled={busy || q.running}
                onClick={() =>
                  void run(async () => {
                    await command({ action: "delete_queue", id: q.id });
                    if (queueId === q.id) setQueueId("");
                  })
                }
              >
                Delete
              </Button>
            </div>
          ))}
          <Button
            variant="outline"
            disabled={
              busy ||
              !checkedIds.length ||
              checkedIds.some(
                (id) =>
                  !["Paused", "Completed", "Failed", "Canceled"].includes(
                    downloads.find((d) => d.id === id)?.state ?? "",
                  ),
              )
            }
            onClick={() =>
              void run(() =>
                command({
                  action: "assign",
                  ids: checkedIds,
                  queue_id: queueId || null,
                }),
              )
            }
          >
            Assign {checkedIds.length} selected to{" "}
            {queueId
              ? automation.queues.find((q) => q.id === queueId)?.name
              : "General"}
          </Button>
          {queueId && (
            <div className="space-y-2">
              <label className="flex items-center gap-2">
                <input
                  type="checkbox"
                  disabled={
                    busy ||
                    automation.queues.find((q) => q.id === queueId)?.running
                  }
                  checked={
                    automation.queues.find((q) => q.id === queueId)
                      ?.synchronize ?? false
                  }
                  onChange={(e) =>
                    void run(() =>
                      command({
                        action: "set_synchronization",
                        id: queueId,
                        enabled: e.target.checked,
                      }),
                    )
                  }
                />
                Check completed sources for changed revisions on each run
              </label>
              <p className="text-muted-foreground">
                Changed sources publish a new .revision file. Existing copies
                are never replaced or deleted. Without a usable validator and
                length, each run downloads a fresh revision. Failed revisions
                need an explicit retry. Keep source history assigned to this
                queue for future checks.
              </p>
              {automation.queues.find((q) => q.id === queueId)?.last_error && (
                <ErrorNote>
                  {automation.queues.find((q) => q.id === queueId)?.last_error}
                </ErrorNote>
              )}
            </div>
          )}
        </section>
        <section
          aria-label="Queue schedules"
          className="space-y-3 border-t border-line-soft pt-4"
        >
          <h2 className="eyebrow">Transfer windows</h2>
          <p className="text-muted-foreground">
            Select a named queue above. Start times are saved as UTC instants;
            recurring intervals use elapsed time, not a repeating local clock
            hour. Missed runs are skipped by default. OSDM must be running.
          </p>
          <div className="grid gap-3 sm:grid-cols-3">
            <Field label="First start · local time">
              <Input
                aria-label="Schedule first start"
                type="datetime-local"
                value={when}
                onChange={(e) => setWhen(e.target.value)}
              />
            </Field>
            <Field label="Repeat every · hours (0 = once)">
              <Input
                aria-label="Schedule repeat hours"
                type="number"
                min="0"
                max="8760"
                value={interval}
                onChange={(e) => setIntervalHours(e.target.value)}
              />
            </Field>
            <Field label="Stop after · minutes (0 = finish)">
              <Input
                aria-label="Schedule window minutes"
                type="number"
                min="0"
                max="525600"
                value={windowMinutes}
                onChange={(e) => setWindowMinutes(e.target.value)}
              />
            </Field>
          </div>
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={catchUp}
              onChange={(e) => setCatchUp(e.target.checked)}
            />
            Run once on startup if a slot was missed
          </label>
          <Button
            disabled={busy || !queueId || !when}
            onClick={() =>
              void run(async () => {
                const next = Math.floor(new Date(when).getTime() / 1000);
                const repeat = Number(interval) * 3600;
                const duration = Number(windowMinutes) * 60;
                if (
                  !Number.isSafeInteger(next) ||
                  next <= Date.now() / 1000 ||
                  !Number.isSafeInteger(repeat) ||
                  repeat < 0 ||
                  !Number.isSafeInteger(duration) ||
                  duration < 0
                )
                  throw new Error(
                    "Choose a future start and valid repeat/window values.",
                  );
                await command({
                  action: "save_schedule",
                  schedule: {
                    id: crypto.randomUUID(),
                    queue_id: queueId,
                    next_run: next,
                    interval_seconds: repeat || null,
                    stop_after_seconds: duration || null,
                    run_once_on_startup: catchUp,
                    enabled: true,
                  },
                });
              })
            }
          >
            Save schedule
          </Button>
          {automation.schedules.map((s) => (
            <div
              key={s.id}
              className="flex flex-wrap items-center gap-2 border-t border-line-soft py-2"
            >
              <span className="flex-1">
                {automation.queues.find((q) => q.id === s.queue_id)?.name} ·{" "}
                {new Date(s.next_run * 1000).toLocaleString()} ·{" "}
                {s.enabled ? "enabled" : "finished/disabled"}
              </span>
              <Button
                variant="outline"
                disabled={busy}
                onClick={() =>
                  void run(() =>
                    command({
                      action: "save_schedule",
                      schedule: { ...s, enabled: !s.enabled },
                    }),
                  )
                }
              >
                {s.enabled ? "Disable" : "Enable"}
              </Button>
              <Button
                variant="outline"
                disabled={busy}
                onClick={() =>
                  void run(() =>
                    command({ action: "delete_schedule", id: s.id }),
                  )
                }
              >
                Delete schedule
              </Button>
            </div>
          ))}
        </section>
        {error && <ErrorNote>{error}</ErrorNote>}
        <section
          aria-label="Completion action"
          className="space-y-3 border-t border-line-soft pt-4"
        >
          <h2 className="eyebrow">After this queue completes · Windows</h2>
          <p className="text-muted-foreground">
            Off by default. Applies once to the named queue selected above. All
            files must complete, unrelated active downloads block it, and a
            visible 60-second countdown can be canceled. Disconnect uses an
            existing Windows RAS/VPN connection, not Wi-Fi or dial-up redial.
          </p>
          <select
            aria-label="Completion action"
            value={powerAction}
            onChange={(e) => setPowerAction(e.target.value)}
            className={selectClass}
          >
            <option value="shutdown">Shut down Windows</option>
            <option value="disconnect">
              Disconnect a named RAS/VPN connection
            </option>
          </select>
          {powerAction === "disconnect" && (
            <Input
              aria-label="Windows connection name"
              placeholder="Existing Windows connection name"
              value={connection}
              onChange={(e) => setConnection(e.target.value)}
            />
          )}
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={powerConsent}
              onChange={(e) => setPowerConsent(e.target.checked)}
            />
            I explicitly permit this completion action for this session
          </label>
          <div className="flex gap-2">
            <Button
              disabled={
                busy ||
                !powerConsent ||
                !queueId ||
                getBridge().kind !== "tauri"
              }
              onClick={() =>
                void run(async () => {
                  await getBridge().armPower(
                    queueId,
                    powerAction === "shutdown"
                      ? { kind: "shutdown" }
                      : { kind: "disconnect", connection },
                    powerConsent,
                  );
                  setPowerArmed(true);
                })
              }
            >
              Arm completion action
            </Button>
            <Button
              variant="outline"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  await getBridge().cancelPower();
                  setPowerArmed(false);
                })
              }
            >
              Cancel completion action
            </Button>
          </div>
          {powerArmed && (
            <p role="status">
              Completion action armed. You can cancel it here or during the
              countdown.
            </p>
          )}
        </section>
      </div>
    </Modal>
  );
}
