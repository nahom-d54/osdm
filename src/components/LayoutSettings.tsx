import { Button } from "@/components/ui/button";
import { useSettings } from "@/store/settings";

export function LayoutSettings() {
  const { settings, update } = useSettings();
  return (
    <section className="flex flex-col gap-3">
      <h3 className="eyebrow">Layout</h3>
      <fieldset>
        <legend className="mb-2 text-xs">
          Visible columns (File is always shown)
        </legend>
        <div className="flex flex-wrap gap-3">
          {["state", "rate", "eta", "size"].map((column) => (
            <label className="flex items-center gap-1 text-xs" key={column}>
              <input
                type="checkbox"
                checked={settings.visible_columns.includes(column)}
                onChange={(event) =>
                  update({
                    visible_columns: event.target.checked
                      ? [...settings.visible_columns, column]
                      : settings.visible_columns.filter(
                          (item) => item !== column,
                        ),
                  })
                }
              />
              {column === "eta" ? "Time left" : column}
            </label>
          ))}
        </div>
      </fieldset>
      <fieldset>
        <legend className="mb-2 text-xs">
          Toolbar buttons (Add download is always shown)
        </legend>
        {[
          ...settings.toolbar_actions,
          ...["resume", "pause", "stop", "inspector"].filter(
            (action) => !settings.toolbar_actions.includes(action),
          ),
        ].map((action) => (
          <div key={action} className="mb-1 flex items-center gap-2 text-xs">
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={settings.toolbar_actions.includes(action)}
                onChange={(event) =>
                  update({
                    toolbar_actions: event.target.checked
                      ? [...settings.toolbar_actions, action]
                      : settings.toolbar_actions.filter(
                          (item) => item !== action,
                        ),
                  })
                }
              />
              {action}
            </label>
            <Button
              size="sm"
              variant="ghost"
              disabled={settings.toolbar_actions.indexOf(action) <= 0}
              onClick={() => {
                const actions = [...settings.toolbar_actions],
                  index = actions.indexOf(action);
                [actions[index - 1], actions[index]] = [
                  actions[index],
                  actions[index - 1],
                ];
                update({ toolbar_actions: actions });
              }}
            >
              Move up
            </Button>
          </div>
        ))}
      </fieldset>
    </section>
  );
}
