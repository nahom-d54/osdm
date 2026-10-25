import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Field, ErrorNote } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { getBridge } from "@/lib/bridge";
import type { Category } from "@/lib/categories";
import { useSettings } from "@/store/settings";

export function CategorySettings() {
  const { settings, update } = useSettings();
  const [error, setError] = useState<string | null>(null);
  const change = (id: string, patch: Partial<Category>) =>
    update({
      categories: useSettings
        .getState()
        .settings.categories.map((category) =>
          category.id === id ? { ...category, ...patch } : category,
        ),
    });
  const rules = (value: string) =>
    value
      .toLowerCase()
      .split(/[,\s]+/)
      .filter(Boolean)
      .slice(0, 200);
  return (
    <section className="flex flex-col gap-3">
      <h3 className="eyebrow">Categories</h3>
      <p className="text-xs text-muted-foreground">
        Exact source domains take precedence over extensions. First category
        wins ties. Unmatched files use General and the default folder. Changes
        affect new downloads only.
      </p>
      {settings.categories.map((category, index) => (
        <fieldset
          key={category.id}
          className="grid grid-cols-2 gap-3 rounded-md border border-line-soft p-3"
        >
          <legend className="px-1 text-xs">
            {category.name || "Unnamed category"}
          </legend>
          <Field label="Name" htmlFor={`category-name-${category.id}`}>
            <Input
              id={`category-name-${category.id}`}
              value={category.name}
              maxLength={128}
              onChange={(event) =>
                change(category.id, { name: event.target.value })
              }
            />
          </Field>
          <Field
            label="Extensions"
            htmlFor={`category-ext-${category.id}`}
            hint="Comma-separated, e.g. zip, tar.gz"
          >
            <Input
              id={`category-ext-${category.id}`}
              value={category.extensions.join(", ")}
              onChange={(event) =>
                change(category.id, {
                  extensions: event.target.value
                    .split(",")
                    .map((item) => item.trim().toLowerCase())
                    .slice(0, 200),
                })
              }
            />
          </Field>
          <Field
            label="Exact domains"
            htmlFor={`category-domain-${category.id}`}
            hint="Comma-separated hostnames; no URLs or wildcards."
          >
            <Input
              id={`category-domain-${category.id}`}
              value={category.domains.join(",")}
              onChange={(event) =>
                change(category.id, {
                  domains: event.target.value
                    .split(",")
                    .map((item) => item.trim().toLowerCase())
                    .slice(0, 200),
                })
              }
              onBlur={() =>
                change(category.id, {
                  domains: rules(category.domains.join(",")),
                })
              }
            />
          </Field>
          <Field
            label="Folder"
            htmlFor={`category-dir-${category.id}`}
            hint="Blank uses the default folder."
          >
            <Input
              id={`category-dir-${category.id}`}
              value={category.directory}
              maxLength={32768}
              onChange={(event) =>
                change(category.id, { directory: event.target.value })
              }
            />
          </Field>
          <div className="col-span-2 flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="secondary"
              onClick={async () => {
                try {
                  const directory = await getBridge().pickDirectory(
                    `Folder for ${category.name}`,
                    category.directory || undefined,
                  );
                  if (directory) change(category.id, { directory });
                  setError(null);
                } catch (caught) {
                  setError(String(caught));
                }
              }}
            >
              Choose folder
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={index === 0}
              onClick={() => {
                const categories = [...settings.categories];
                [categories[index - 1], categories[index]] = [
                  categories[index],
                  categories[index - 1],
                ];
                update({ categories });
              }}
            >
              Move up
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() =>
                update({
                  categories: settings.categories.filter(
                    (item) => item.id !== category.id,
                  ),
                })
              }
            >
              Delete category
            </Button>
          </div>
        </fieldset>
      ))}
      <Button
        size="sm"
        variant="secondary"
        disabled={settings.categories.length >= 100}
        onClick={() =>
          update({
            categories: [
              ...settings.categories,
              {
                id: crypto.randomUUID(),
                name: "New category",
                extensions: [],
                domains: [],
                directory: "",
              },
            ],
          })
        }
      >
        Add category
      </Button>
      {error && <ErrorNote>{error}</ErrorNote>}
    </section>
  );
}
