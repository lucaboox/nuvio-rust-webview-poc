/**
 * Naming for catalogs and content types.
 *
 * An addon usually publishes one catalog per type under the same name, so
 * "HBO Max" appears twice in any list that spans both and there is no way to
 * tell which is which. The type belongs in the label — except where the addon
 * already put it there, which many do.
 */

export function typeLabel(type: string): string {
  switch (type.toLowerCase()) {
    case "movie":
      return "Movies";
    case "series":
      return "Series";
    case "anime":
      return "Anime";
    case "channel":
      return "Channels";
    case "tv":
      return "TV";
    default:
      return type.charAt(0).toUpperCase() + type.slice(1);
  }
}

/**
 * Appends the content type to a catalog's name, unless the name already says
 * it. "HBO Max" becomes "HBO Max Series"; "HBO Max Series" is left alone, and
 * so is "Popular Movies".
 */
export function catalogLabel(title: string, contentType?: string): string {
  const name = title.trim();
  if (!contentType?.trim()) return name;
  const label = typeLabel(contentType);
  const haystack = name.toLowerCase();
  // Checked against both forms: a catalog named "Trending Movie" reads as
  // already typed even though the label is plural.
  const singular = contentType.trim().toLowerCase();
  if (haystack.endsWith(label.toLowerCase()) || haystack.endsWith(singular))
    return name;
  return `${name} ${label}`;
}
