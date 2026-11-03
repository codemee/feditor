**English** | [繁體中文](customization.md)

[← Back to the README](../README.en.md)

# Customization and settings file

## Interface language

The **interface language** is a three-state button in the settings panel: "Follow system" (globe), "中文" (Chinese, a "中" icon), or "English" (an "En" icon); each click cycles to the next one and takes effect immediately without a restart. "Follow system" decides by the operating system's language: Chinese on a Chinese system, English otherwise. It is stored as `language` (`system` / `zh` / `en`) in the settings file. What is translated is the interface text, tooltips, and messages; documents such as the README, and the comment inside recorded macro files, are not affected.

## Color schemes

- **Appearance**: a single three-state button; its icon changes with the current state (◐ follow system, ☀ light, ☾ dark) and each click advances to the next one (follow system → light → dark → follow system).
- **Color scheme**: **light and dark each get their own scheme**, independent of each other (for example GitHub Light for light and Dracula for dark); the one for the current appearance is used, also when following the system. Your choice is remembered. Built-in schemes:
  - Dark: EVA (the default, the *Neon Genesis Evangelion* palette: purple-tinted near-black, Unit-01 purple, neon green, NERV orange), Dracula, Monokai, Solarized Dark, One Dark, Nord, Gruvbox Dark, Tokyo Night, Catppuccin Mocha / Macchiato / Frappé.
  - Light: Default Light (the original look), Solarized Light, GitHub Light, One Light, Gruvbox Light, VS Code Light+, Catppuccin Latte.
  - The built-in schemes are adapted from these well-known palettes, with the contrast between text and every background checked (at least 4.5:1 for body text), so they are not necessarily identical to the originals.

## Custom color schemes

- **Custom schemes**: "Edit current scheme…" in the settings panel opens an editor for the scheme selected for the **appearance currently in use** (dark or light; switch the appearance first to edit the other one). Every color (background, text, buttons and lines, accents and status, syntax coloring, the 16 terminal colors; 41 in all) has a swatch, changes preview immediately, and "Revert" discards them. Built-in schemes cannot be overwritten: use "Save as…" to give it a name and store it as a custom scheme (it is also selected for that appearance); a custom scheme can be saved in place.
  Custom schemes live in the `themes` folder inside the settings folder (Windows `%APPDATA%\feditor\themes\`; the "Scheme folder" button in the settings panel opens it), one `*.conf` file per scheme, and **they are re-read whenever the settings panel opens**, so you can also write one in a text editor or drop in one from someone else. The format is `key = value` (whole lines starting with `#` are comments, and a color may be followed by `  # note`):

  | Key | Meaning |
  |---|---|
  | `name` | Scheme name (required). A custom scheme overrides a built-in one with the same name |
  | `mode` | `dark` or `light`: which list the scheme appears in (default `dark`) |
  | `base` | Optional: colors you leave out are inherited from this existing scheme (built-in or another custom one); without `base` the default scheme of that mode fills the gaps |
  | `editor_bg`, `panel_bg`, `popup_bg`, `faint_bg`, `code_bg` | Backgrounds: editor, panels, popups, faint (striping), code |
  | `text`, `text_strong`, `text_weak` | Normal, strong, and secondary text |
  | `button`, `button_hover`, `button_active`, `selection`, `line`, `line_hover` | Buttons (normal, hovered, pressed), selection, lines (normal, hovered) |
  | `accent`, `cursor`, `link`, `warning`, `error` | Accent, caret, hyperlinks, warnings (also search-result highlights), errors |
  | `syn_comment`, `syn_string`, `syn_number`, `syn_keyword`, `syn_type`, `syn_function` | Syntax coloring: comments, strings, numbers, keywords, types, functions |
  | `term_black`, `term_red`, `term_green`, `term_yellow`, `term_blue`, `term_magenta`, `term_cyan`, `term_white`, and the `bright_` variants (e.g. `term_bright_red`) | The 16 colors of the terminal pane (ANSI 0–15); its default text uses `text` and its background `editor_bg` |

  Write colors as `#RRGGBB` (or `#RGB`). Broken files (missing `name`, unknown keys, bad colors, an unknown `base`) are skipped and the reason is shown in the output panel. See `examples/themes/midnight.conf`. If the scheme named in the settings no longer exists (for example the file was deleted), the default scheme of that mode is used.

## Fonts

- **Font**: lists **monospace fonts only**. A font counts as monospace when `i`, `m`, `W`, `.`, etc. all have the same advance width; the monospace flag inside font files is not used because it is often wrong. The default is the **operating system's default monospace font** (queried from the OS, not hard-coded; Consolas on Windows).
  Some old fonts (MingLiU, MS Gothic, Lucida Console, ...) have embedded *color bitmap* Latin glyphs, which egui does not tint with the text color and therefore draws invisibly, so such fonts are left out of the list; if the settings file names one, the editor falls back to the system default and the panel shows the font that is actually in effect.
  Building the list means opening every font file, so it is scanned in the background; until it finishes the panel shows "Finding monospace fonts on the system…". Chinese glyphs come from Microsoft JhengHei. Fonts differ in ascent/descent proportions, which makes egui draw Latin letters and Chinese characters on different baselines by default, so at startup and whenever the font changes the gap is measured with egui's real layout and compensated; tests verify that every font in the list puts both on the same baseline.
- **Font size**: the size field in the panel and the shortcuts share one setting, so they are always in sync. Shortcuts: **Ctrl +/−** to enlarge/shrink, **Ctrl+mouse wheel**, and **Ctrl+0** to reset (range 8–48 pt). This replaces egui's built-in "Ctrl +/− zooms the whole UI".

## Settings file

- **Settings file** location: Windows `%APPDATA%\feditor\settings.conf`, macOS `~/Library/Application Support/feditor/settings.conf`, Linux `$XDG_CONFIG_HOME/feditor/settings.conf` (`~/.config/feditor/settings.conf` when unset). It is plain `key = value`: `theme` (`system`/`light`/`dark`), `font_family` (no line means the system default), `font_size`, and `show_line_numbers` / `highlight_line` / `show_whitespace` / `syntax` / `word_wrap` (`true`/`false`). A missing or corrupt file, or an unreasonable value, falls back to the default and never prevents startup. While the font size is changing continuously the file is written only after 0.4 s of inactivity, and always on close.

## Custom syntax

- **Custom languages**: put `*.conf` files in the `syntax` folder under the config directory (Windows `%APPDATA%\feditor\syntax\`), one language per file; they take effect after a restart and take priority over the built-in languages (so you can override a built-in keyword list). Broken files are skipped and the reason is shown in the output panel. See `examples/syntax/php.conf`. The format is `key = value`; whole lines starting with `#` are comments:

  | Key | Meaning |
  |---|---|
  | `name` | Language name (required) |
  | `extensions` | File extensions separated by commas or spaces; a full file name such as `makefile` also works (required) |
  | `line_comment` | Line comment markers, possibly several, e.g. `//, #` |
  | `block_comment` | Start and end of a block comment, e.g. `/*, */` |
  | `quotes` | String quote characters, e.g. `", '` (default: `"` and `'`) |
  | `keywords` | Keywords separated by commas or spaces; may appear on several lines (merged) |
  | `capital_types` | `true` colors identifiers starting with a capital letter as types |
  | `case_insensitive` | `true` makes keywords case-insensitive (SQL) |
  | `triple_quotes` / `backtick_strings` | Triple-quoted strings (Python) and backtick strings (JavaScript); they can span lines |
  | `interpolation` | Start and end of an embedded expression in a string, e.g. `${, }` (JavaScript), `#{, }` (Ruby), `{, }` (Python f-string) |
  | `interpolation_in` | Which strings contain embedded expressions, comma-separated: `backtick` (backtick strings), `double` (double-quoted strings), `fstring` (strings with an `f` or `$` prefix; `{{` is an escape). Default: `backtick, double` |
  | `mode` | `code` (default), `markup` (HTML/XML), `markdown`, `config` (`[section]` and `key =` in ini/toml) |
