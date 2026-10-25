export interface Category {
  id: string;
  name: string;
  extensions: string[];
  domains: string[];
  directory: string;
}

export const DEFAULT_CATEGORIES: Category[] = [
  {
    id: "music",
    name: "Music",
    extensions: ["mp3", "flac", "wav", "ogg", "m4a"],
    domains: [],
    directory: "",
  },
  {
    id: "video",
    name: "Video",
    extensions: ["mp4", "mkv", "webm", "avi", "mov"],
    domains: [],
    directory: "",
  },
  {
    id: "programs",
    name: "Programs",
    extensions: ["exe", "msi", "msix", "deb", "rpm"],
    domains: [],
    directory: "",
  },
  {
    id: "documents",
    name: "Documents",
    extensions: ["pdf", "txt", "doc", "docx", "epub"],
    domains: [],
    directory: "",
  },
  {
    id: "compressed",
    name: "Compressed",
    extensions: ["zip", "7z", "rar", "tar", "gz", "bz2", "xz", "zst"],
    domains: [],
    directory: "",
  },
];

/** Exact source hostname rules precede extension rules; list order breaks ties.
 * Paths, query strings, credentials, and redirected hosts never become rules. */
export function routeCategory(
  categories: Category[],
  source: string,
  filename: string,
): Category | undefined {
  let host = "";
  try {
    host = new URL(source).hostname.toLowerCase().replace(/\.$/, "");
  } catch {
    /* no domain match */
  }
  const domain = categories.find(
    (category) =>
      host &&
      category.domains.some(
        (rule) => rule.toLowerCase().replace(/\.$/, "") === host,
      ),
  );
  if (domain) return domain;
  const name = filename.toLowerCase();
  return categories.find((category) =>
    category.extensions.some((rule) => {
      const extension = rule.toLowerCase().replace(/^\./, "");
      return (
        extension !== "" &&
        name.length > extension.length + 1 &&
        name.endsWith(`.${extension}`)
      );
    }),
  );
}
