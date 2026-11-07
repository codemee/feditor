**English** | [繁體中文](project-commands.md)

# Project commands (design draft)

> Status: stages 1 and 2 of section 10 are implemented (Cargo, uv, poetry, `.venv` scripts; the code is in `ui/src/project_cmd.rs`); stage 3 (PEP 723, pytest, npm, Makefile, `go`) is not done. Where the implementation differs from the draft is listed in "Implementation notes" at the end. The goal is to let "Run in terminal" offer, besides "which interpreter runs this file", the commands that a project's configuration file provides (`cargo run`, `uv run`, ...), and to offer something clickable only when the tool it needs is actually installed.

## 1. Goals and non-goals

**Goals**

- When a `.py` file is open, besides the interpreters found today, also look for a project configuration file from the file's folder upwards and list the commands it can run as options.
- Same for `.rs` files: finding `Cargo.toml` offers `cargo run`, `cargo test` and so on.
- **When the tool is not on the system, do not show an item that cannot run when clicked.** Show a single line instead (for example "Found a project configuration that uses uv, but uv is not installed").
- The discovery phase only reads files and checks whether tools exist and which version they are; **it never runs a command from the project** (the same rule `discover` follows today).

**Non-goals (at least for the first version)**

- No plugin-based providers (see section 8).
- No project command runs automatically; the user always picks one and presses Run.
- No installing tools for the user; only say what is missing and hint how to install it.
- No change to the existing interpreter search (`Rule`, `discover`, `Resolver` stay as they are).

## 2. Terms

| Term | Meaning |
|---|---|
| provider | Code that understands one kind of project configuration file, for example Cargo or Python (`pyproject.toml`). Input is a file path, output is findings. |
| finding | What a provider found in one configuration file: a set of commands plus the tool those commands need. |
| project command (`ProjectCommand`) | One runnable option: display name, program, arguments, working folder, source configuration file. |
| tool status (`ToolStatus`) | Whether the tool a command needs is on the system: available (with path and version) or missing. |

## 3. Data structures

```rust
/// One project command (it only becomes a clickable item after probing confirmed it can run).
pub struct ProjectCommand {
    /// A stable id used to remember the manual choice in settings, e.g. `cargo:run`, `cargo:example:demo`, `uv:script:serve`.
    pub id: String,
    /// The name shown in the menu, e.g. `cargo run --example demo`.
    pub label: String,
    pub program: PathBuf,        // already resolved to the real path
    pub args: Vec<String>,       // fixed arguments
    pub cwd: PathBuf,            // the folder of the configuration file
    pub source: PathBuf,         // the configuration file, for display (Cargo.toml)
    /// Whether the current file is appended (`uv run file.py` does, `cargo run` does not).
    pub takes_file: bool,
}

pub enum ToolStatus {
    Available { path: PathBuf, version: String },
    /// Not found, or found but cannot run (for example rustup's `cargo` with no toolchain installed).
    Missing { tool: &'static str, reason: MissingReason },
}

pub enum MissingReason { NotFound, Broken(String) }

/// What one provider found in one configuration file.
pub struct Finding {
    pub provider: &'static str,  // "Cargo", "uv", "poetry"
    pub source: PathBuf,
    pub tool: &'static str,      // the tool that is needed
    pub commands: Vec<CommandSpec>, // commands not yet bound to a tool path (name and arguments only)
}
```

A provider only reads the configuration file and produces `CommandSpec`s; **it does not detect tools**. One shared function does that. So providers can be tested with a fake file system and tool detection with a fake `PATH` (the same way `discover` takes injected `env` and `path_var` today).

## 4. Discovery flow

```
a file is opened / the tab changes
   └─ Resolver (background thread, existing cache and re-search)
        ├─ 1. interpreters: the existing discover (unchanged)
        └─ 2. project commands (new)
             a. pick providers by language (Python -> pyproject; Rust -> Cargo)
             b. look for the configuration file from the file's folder upwards
                (stop conditions as in project_root: a .git folder, the home folder, the disk root)
             c. the provider reads it -> Finding (unreadable or unparsable files are skipped, no error)
             d. for each Finding's tool run tool detection -> ToolStatus
             e. build the result: runnable ProjectCommands + "tool missing" notices
```

- The result sits next to the existing `Resolved` (`Resolved.items` stays the interpreters, plus a new `project: ProjectResolved`), sharing the same background thread and the 30 s / 5 s staleness. After the user installs a tool, the existing "search again" updates it.
- Configuration files get a size limit (for example 1 MB); bigger ones are skipped so a huge file cannot stall the search.
- Going upwards may find several files (nested workspaces). **The nearest one wins**; a Cargo workspace member uses its own `Cargo.toml`, and running `cargo run` inside the member folder is cargo's standard behaviour.

### Tool detection (the heart of this design)

One function `detect_tool(spec) -> ToolStatus`:

1. Look on `PATH` (reusing `program_path_in` and `skip_paths`; on Windows the fake `WindowsApps` shortcuts are excluded as well; `.exe` and `.cmd` are tried too).
2. If it is not on `PATH`, try fixed locations: `cargo` tries `~/.cargo/bin` (rustup's default; `PATH` often lacks it when started from a GUI); `uv` tries `~/.local/bin` and `~/.cargo/bin`.
3. When found, confirm in the background with `--version` that it really runs (4 s timeout, reusing `probe_version`). One that does not run is `Broken`:
   - rustup's `cargo` without a default toolchain -> `Broken("no default toolchain")`, hint `rustup default stable`.
   - the `python3` stub on macOS without developer tools -> likewise.
4. A tool is detected once per search round (the five `cargo` commands share one result).

**Rule: if the tool is not `Available`, none of its commands enters the selectable list.** Only one "tool missing" notice is produced.

### Which commands need which tool

The point: **the tool that is needed depends on how the command runs, not on which configuration file was found.**

| Situation | How it runs | Needs |
|---|---|---|
| `Cargo.toml` | `cargo run`, `cargo test`, ... | `cargo` |
| `pyproject.toml` has `[project.scripts]` and the project already has a `.venv` | run `.venv/bin/name` directly (Windows `.venv\Scripts\name.exe`) | no `uv` |
| same, but no `.venv`, and the project uses uv (`uv.lock` or `[tool.uv]`) | `uv run name` | `uv` |
| the project uses poetry (`poetry.lock` or `[tool.poetry]`) | `poetry run name` | `poetry` |

So "`.venv` exists, `uv` is not installed" still lists the script commands (through `.venv`); a missing `uv` does not remove options that can still run.

## 5. How a missing tool is shown

- **No clickable item.** The menu does not contain a greyed-out `uv run ...`, because a greyed-out item looks broken and is easily mistaken for a vanished feature.
- Instead there is **one non-clickable line of text** (a `label`, pale colour) at the end of the project-commands section:
  - `Found a project configuration that uses uv (pyproject.toml, uv.lock), but uv is not installed`
  - `Found Cargo.toml, but cargo is not installed`
  - `Found Cargo.toml, but cargo cannot run (no default toolchain); try rustup default stable`
- The notice's tooltip carries an install hint: on Windows `scoop install uv` first, on macOS and Linux `brew install uv` first, other ways as a link to the official documentation. **It only hints; it never runs anything for the user.**
- **A notice only appears when the evidence is strong**, to avoid false alarms:
  - uv: needs `uv.lock` or `[tool.uv]` in `pyproject.toml`. A bare `pyproject.toml` does not mean the user wants uv; nothing is shown.
  - poetry: needs `poetry.lock` or `[tool.poetry]`.
  - cargo: `Cargo.toml` is enough.
- One missing tool gets one line, however many commands it affects.
- The interpreter button in the status bar is unaffected. These notices only appear in the dropdown.

## 6. What the providers cover

### 6.1 Cargo (`.rs` files)

Find the nearest `Cargo.toml` upwards and offer commands by file location:

| File location | Commands |
|---|---|
| a crate with `src/main.rs` or `[[bin]]` | `cargo run` (with several `[[bin]]`: `--bin name`) |
| `src/bin/name.rs` (or `src/bin/name/main.rs`) | `cargo run --bin name` |
| `examples/name.rs` (or `examples/name/main.rs`) | `cargo run --example name` |
| `tests/name.rs` | `cargo test --test name` |
| any file of a crate | `cargo test`, `cargo build` |
| `src/lib.rs`, a crate with no bin | only `cargo test`, `cargo build` |

- Commands that match the current file come first (in `examples/demo.rs` the `--example demo` command is first).
- The working folder is the folder of `Cargo.toml`; the user's "extra arguments" go after `--` (for the program itself).
- Reading `Cargo.toml` needs a TOML parser: the project already has one (used by the preview), so the first version reuses it and adds no dependency (to be confirmed during implementation).

### 6.2 Python (`.py` files)

Find the nearest `pyproject.toml` upwards:

| Finding | Commands |
|---|---|
| the project uses uv | `uv run file`, `uv run script-name` (one per `[project.scripts]` entry) |
| the project uses poetry | `poetry run python file`, `poetry run script-name` |
| the project already has a `.venv` | `script-name` inside `.venv` (no uv or poetry needed, see the previous section) |
| the file is `test_*.py` or `*_test.py` and pytest is in the `.venv` or the dependencies | `pytest file` (second version) |

- The value of `uv run file`: when the project has no `.venv` yet, uv creates it and installs the dependencies; the interpreter search cannot do that.
- PEP 723 inline script metadata (`# /// script`): when present, offer `uv run file` (needs uv); third stage.

### 6.3 Possible later

npm (`scripts` in `package.json`), `Makefile` / `justfile` (target lists), `go.mod` (`go run`, `go test`). These lists can be long, so the count limit and search (section 9) must come first.

## 7. User interface

The dropdown has two sections, each with a small heading:

```
Interpreters
  ● Python 3.12.1 · .venv
    Python 3.11.9 · PATH
Project commands (Cargo.toml)
    cargo run --example demo
    cargo test
    cargo build
    Found a project configuration that uses uv, but uv is not installed   <- pale, not clickable
```

- **The default stays the interpreter** (today's behaviour). Only after the user picks a project command is it used, and it is remembered in the settings (key: `project#cmd:id`, in the style of the existing `choice_key`, so old settings are unaffected). After an upgrade nobody finds that what "Run" does has changed.
- Once a project command is chosen, the status bar button shows `cargo run --example demo`; if the command is gone (the configuration changed, the tool was removed) it falls back to the first usable interpreter.
- Running opens (or reuses) a terminal whose working folder is the command's `cwd`, not the file's folder; the file path is appended only when `takes_file` is true.
- The "extra arguments" field keeps its meaning and goes at the end of the command (after `--` for Cargo).

## 8. Why built-in providers and not plugins

- The existing plugins (macros, preview) run in a sandbox that cannot read files or reach the network. Letting a plugin look for project configuration files means designing a new set of host APIs first (walk up folders, read files, query tools), which is redoing a security boundary.
- Only a few configuration files need support, their formats are fixed, and code in the program is easy to test and behaves the same on all three platforms.
- The provider interface (input a file path, output `Finding`s) is deliberately narrow: if someone really wants to add a language later, this interface can be opened up as the plugin API without redesign. The decision whether to do that waits until then.

## 9. Risks and things to settle

- **Security**: the `scripts` in `package.json`, Makefiles and the scripts in `pyproject.toml` are arbitrary commands. Discovery never runs them; the menu always shows the full command line; nothing runs automatically; changing the project folder does not carry the previous project's choice over (choices are remembered per project already).
- **Count**: a Makefile can have dozens of targets. A limit is needed (for example 12, the rest under "More..."), or a searchable list. The first version has only Cargo and Python and will not hit it, but the data structures must leave room.
- **Cross-platform**: Windows `cargo.exe`, `uv.exe`, `.cmd` wrappers; `PATH` is often incomplete when started from a GUI (hence the fixed-location fallbacks); executables inside `.venv` are `Scripts\name.exe` on Windows.
- **Terminal environment**: the existing `terminal_env` adds `VIRTUAL_ENV` and `PATH` to the terminal when the user chose a `.venv` interpreter. Whether project commands get the same must be decided (suggestion: `uv` and `poetry` manage their own environment, so no; `.venv` scripts yes).
- **Unparsable configuration**: skipped without an error dialog. Showing "`Cargo.toml` could not be read" in the notice line is better than silently vanishing; decide in the second version.
- **Cost of version probing**: each tool's `--version` starts a process (`uv` and `cargo` are fast; `cargo` under rustup is occasionally slow). Results are shared within one search round and use the 30 s cache.

## 10. Implementation stages

1. **Skeleton**: add `ui/src/project_cmd.rs` (data structures, tool detection, `Finding` and the provider interface), add the project-command field to `Resolved`, show the dropdown in sections, make `run_in_terminal` accept "program + arguments + cwd". Cargo only at first. Tests use a fake file system and a fake `PATH` and cover the "tool present", "tool missing" and "tool broken" outcomes, confirming the latter two produce only a notice and no clickable item.
2. **Python**: uv / poetry / `.venv` scripts, plus the missing-tool notices.
3. **Extensions**: PEP 723, pytest, npm, Makefile, `go`; count limit and search.
4. Later, evaluate whether to open the provider interface as a plugin API.

## 11. What the tests must cover

- Providers: given a file path and fake file contents, the resulting `Finding`s and command lists (including workspaces and the `examples/` and `src/bin/` mappings).
- Tool detection: on `PATH`, absent, present but not running, in a fixed location, excluded by `skip_paths`.
- Combinations: `.venv` exists but no uv -> `.venv` scripts are still listed; no `.venv` and no uv -> only the notice; only a `pyproject.toml` without evidence of uv -> nothing is shown.
- Interface: the missing-tool notice is not a button (it cannot be clicked or reached by the keyboard).
- Remembered choice: old settings files are still read; a choice that points to a vanished command falls back to the interpreter.

## 12. Implementation notes

- The data structures differ slightly from the draft: a provider produces `Finding`s (with `CommandSpec`s not yet bound to a tool path), and `resolve` turns them into `ProjectCommand`s after detecting the tools; for a `.venv` script `CommandSpec.program` is the script's path and `Finding.tool` is `None`.
- Remembered choice: the key is `project#cmd:language` (for example `/code/app#cmd:Python`), the value is the command's `id`, stored in the existing `runtime_choices`; picking an interpreter or "Auto" also clears the project-command choice of the same project.
- The notices are unified as "Found a project configuration (evidence), but tool is not installed" and "... but tool cannot run: reason", where the evidence is like `Cargo.toml` or `pyproject.toml, uv.lock`.
- The section in the menu is titled "Project commands (Cargo.toml)"; a menu item's tooltip shows the working folder and the full command line.
- A language without an interpreter (Rust) uses the first project command by default; so does Python when none of its interpreters can run.
- Search results are cached per (language, file path), since they depend on the file's location, and expire after 30 s (5 s when empty); "Search again" refreshes them as well.
