**English** | [繁體中文](preview-plugins.md)

[← Back to the README](../README.en.md)

# Preview plugins

The preview pane supports Markdown, CSV / TSV, JSON / TOML / YAML and SVG out of the box. Other file types can be previewed with JavaScript plugins.

- **Preview plugins**: to preview other file types, write a JavaScript plugin and put it in the `previews` folder under the config directory (Windows `%APPDATA%\feditor\previews\`; the "Preview folder" button in the "Preview" group of the settings panel opens it), one `*.js` file per plugin; they are **re-read whenever the settings panel opens**. See `examples/previews/` (a log-file table, an INI tree, a to-do list). Plugins run in the same sandbox as macros (time and memory limits, no file or network access, and no `editor`), so installing someone else's plugin is safe.
  - The header is declared in `//` comments: `@name` (optional, defaults to the file name), `@extensions` (the extensions it handles, comma-separated, case-insensitive, required), and `@output` (how to show the result, required).
  - A plugin defines a global function `preview(text, info)`: `text` is the whole file, and `info` is `{ path, name, ext }` (empty for unsaved documents). What it returns depends on `@output`:

    | `@output` | Return | Shown as |
    |---|---|---|
    | `markdown` | a Markdown string | like the built-in Markdown preview |
    | `table` | `{ header: [...], rows: [[...], ...] }` | a table |
    | `tree` | any JSON value (object, array, ...) | a collapsible tree |
    | `svg` | an SVG string | a drawing |
    | `text` | a string | monospaced text |

  - When a plugin handles an extension, it takes priority over the built-in preview (so you can even replace the built-in Markdown preview). Plugin errors (no `preview` function, an exception, a timeout, a wrong return shape, ...) are shown in the preview pane; a plugin with a bad header is skipped and the reason is shown in the output panel.
  - For now a plugin can only choose among these output forms, and calling external programs is not supported.
