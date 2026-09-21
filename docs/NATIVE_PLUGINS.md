# Native project plugins — 0.21

A native plugin installs a reviewed collection of skills, slash commands,
lifecycle hooks and MCP definitions into one project. The desktop and CLI use
the same Rust implementation. Installation runs no package scripts and starts
no process. The application itself still needs no Python runtime; a Python
project's own tools are separate dependencies.

## Install and use

Open **Settings → Plugins**, review a built-in bundle or import a local JSON
bundle, expand its files, then install it in the displayed project. The project
must be trusted and writable, with no running or queued task or manual mutation
holding its workspace reservation. Project and content hashes guard stale actions.

Installed skills appear in **Terminal → Skills** and the slash menu. For example,
`/skill python-expert--python-build inspect the packaging` starts a normal task
with the installed instructions and existing permissions/model routing. Commands
use the same namespace, such as `/linux-expert--apt-info bash`.

Use **Review plugin hooks** or **Review plugin MCP servers** to inspect and
activate executable integrations separately. The install operation clears stale
grants for the paths it creates, so reinstalling identical content does not
silently reactivate it. Each MCP tool call retains its normal approval. Hooks
run only after content-specific activation and obey ordinary hook lifecycle rules.

Built-ins:

- **python-expert 2.0.0**: Python build guidance, Ruff formatting and test-watcher
  workflows, plus optional project-wide Ruff formatting after Python edits and
  lint checks before commits. Ruff/Python must come from the project's environment.
- **linux-expert 2.0.0**: AppImage, Debian packaging and service-review skills,
  plus package inspection and build commands. Tools and build commands are
  selected from the actual project; installation does not install system packages.
- **ios 1.0.0**: Xcode inspection/build guidance. Actual Xcode builds require
  an appropriate macOS host and its installed tooling.

Legacy plugin directories are displayed and preserved at
`$XDG_CONFIG_HOME/shadow-agent/shadowcode/plugins/`. Python callbacks are not
loaded. Install a native replacement or convert custom contents to the schema
below. The old PostHog/Docker entries were registry stubs; they are not represented
as working native connectors. Configure a reviewed MCP definition for a real
installed service instead. Legacy agent-name declarations were not executable
agents; delegated native agents remain a separate migration requirement.

## CLI

Use the same explicit `--profile` and `--workspace` on each command when testing:

```sh
shadowcode plugin
shadowcode plugin inspect python-expert --json
# Use the bundle hash returned by inspect:
shadowcode plugin install python-expert --hash BUNDLE_HASH
shadowcode skill --list
shadowcode skill python-expert--python-build "inspect the packaging"
shadowcode hooks --json

shadowcode plugin inspect --file examples/plugins/review-kit.json --json
shadowcode plugin install --file examples/plugins/review-kit.json --hash BUNDLE_HASH
shadowcode plugin --json
# Removal uses the current installation hash, not the original bundle hash:
shadowcode plugin remove review-kit --hash INSTALLATION_HASH
```

Inspect is read-only and prints every generated file. Import accepts a bounded
regular JSON file, including a user-selected file outside the project. It does
not fetch URLs or install runtime dependencies. The CLI can share an open desktop
engine without changing its selected project.

## Bundle format

See the usable [review-kit example](../examples/plugins/review-kit.json).
`format`, `name`, `version` and `description` are required. Optional collections:

- `skills`: map of component names to `{description, content, mode?, files?}`.
  `files` maps relative supporting-file paths to UTF-8 text.
- `commands`: the same workflow fields, without supporting files.
- `hooks`: [native hook definitions](NATIVE_HOOKS.md), with component names.
- `mcp_servers`: [native MCP definitions](NATIVE_MCP.md), with component names.
  Use `env_refs` or `api_key_env`; literal `env` values are rejected in bundles.

Names use 1–32 lowercase letters, numbers or hyphens. Generated names are
`plugin--component`; skills and commands must have distinct component names.
Workflow modes are `code`, `plan`, `review`, `test`, or omitted. Content is the
body, without front matter. Metadata is generated with quoted fields and validated
by the normal workflow parser. Unknown fields, including automatic install scripts,
permission overrides and agent declarations, are rejected.

Generated project files:

```text
.shadowcode/skills/plugin--skill/SKILL.md
.shadowcode/skills/plugin--skill/supporting-file.md
.shadow/commands/plugin--command.md
.shadowcode/hooks/plugin--hook.json
.shadowcode/mcp/plugin--server.json
```

Supporting files cannot escape their skill directory, use dot paths, or replace
`SKILL.md`. They are plain text files, without an executable permission grant.
A selected model may read them through the normal confined file tools. Shell
snippets are instructions for ordinary tool execution, never install-time code.

Limits: 256 KB per bundle and its expanded contents, 64 files per plugin,
32 plugin records per project. Existing workflow/hook/MCP discovery limits also
apply; conflicting workflow names and occupied files are refused before writing.
No overwrite or automatic upgrade is performed. To upgrade, remove the old
installation, preserve or relocate edited files reported by removal, then review
and install the new bundle.

## Removal and interruption

Removal revokes this bundle's hook/MCP activations for the selected project,
deletes only files whose bytes still match the installation, and lists edited
or unreadable files it preserved. Empty skill component directories are pruned;
user files and shared configuration directories remain. Changes made by running
hooks, MCP tools or workflow tasks are outside the install inventory and are not
undone by uninstalling.

A private journal under the profile state directory records the bundle and
project before the first file is created. Installation is not an atomic transaction
across multiple project files: a disk failure or killed process can leave a partial
install. Settings identifies its `prepared`/`removing` state; refresh and remove it
to clean up unchanged owned files before retrying. Other files are preserved.
Corrupt or inaccessible inventory is reported rather than interpreted as an empty
successful installation. User project files never determine uninstall ownership.

API routes shared by desktop and CLI are `GET /api/plugins` and
`POST /api/plugins/preview`, `/install`, `/remove`. Preview takes a built-in `name`
or a `bundle`. Install additionally requires the displayed `workspace` and bundle
`hash`. Remove requires `workspace`, `name` and the current installation `hash`.
Mutations return `{result, catalog}`; removal's `result.retained` explains every
preserved owned path. Audit events record installation/removal outcomes without
embedding bundle content or secret values.
