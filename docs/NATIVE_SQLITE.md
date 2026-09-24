# Native SQLite inspection

> **Advanced.** This is reached through model tools, the CLI and MCP. For the everyday workflow see the [user guide](USER_GUIDE.md), [subscriptions](SUBSCRIPTIONS.md) and [local models](LOCAL_MODELS.md).

The native engine lists tables and queries existing SQLite files inside the
selected project. It replaces the Python built-in reader with bundled SQLite;
no Python, shell, database CLI or external MCP registration is required. The
same reader serves model tools, the CLI, desktop IPC and the MCP server.

## Use it

```sh
shadowcode --workspace /path/to/project sqlite app.db
shadowcode --workspace /path/to/project --json sqlite app.db \
  'SELECT id, name FROM users WHERE id > ? ORDER BY id' \
  --params '[100]' --limit 50
```

With no SQL, the response includes `tables` and their CREATE TABLE definitions
in `rows`. Queries return `columns`, `rows`,
`truncated`, `limit`, and `read_only`. Check `truncated`; narrow the predicate
or use an ordered keyset query to retrieve further rows. Duplicate column names
are rejected: use `AS` aliases rather than losing a value in a JSON object.
NULL, integers, finite numbers and UTF-8 text retain their types. BLOBs return
`{ "type": "blob", "hex": "00ff", "bytes": 2 }`. Invalid UTF-8 and non-finite
numbers fail explicitly; `hex(column)` can produce a readable representation.

Models use the legacy names `mcp_sqlite_tables(path)` and
`mcp_sqlite_query(path, sql, params?, limit?)`. They are native tools available
in Plan/Review and Build, with durable task results and bounded parallel reads.
External MCP activation is unrelated to these built-ins.

External clients use `shadow_sqlite` with the same arguments, optionally adding
`timeout_ms`. Omit `sql` to list tables. The default read-only server supports it;
the selected project remains fixed. Application clients use `POST /api/sqlite`
with that JSON body through the native service.

## Data safety and live databases

The main file opens read-only and must already exist. SQLite's authorizer
denies data/schema writes, ATTACH/DETACH, configuration PRAGMAs, transactions,
extension loading and file-access functions. A query must be a single statement returning columns;
SELECT, WITH and subqueries work. Read-only metadata PRAGMAs are allowed:
`table_info`, `table_xinfo`, `index_list`, `index_info`, `index_xinfo`,
`foreign_key_list`, and `table_list`, including their table-valued forms.
Parameters are bound separately from SQL.
A bounded preparation pass rejects multiple statements without recursively
walking an arbitrary chain of SQL statements.

Live WAL reads retain SQLite's normal locking and transaction semantics and
see committed WAL data; the connection never asserts `immutable=1`. SQLite may
create or update its `-wal`/`-shm` coordination files, including after a writer
closes or leaves WAL data without shared memory. This bookkeeping does not
grant SQL write access or change database rows/schema. Tests check database and
existing WAL bytes remain unchanged while reading a separate process's commits.
An exclusive lock, inaccessible coordination file, corruption, or recovery that
requires a database write produces an error rather than bypassing SQLite locks.

The Linux reader holds the workspace and every real parent directory open. A
private VFS preserves the directory capability's `/proc/self/fd` path while
delegating locking and I/O to SQLite's bundled Unix VFS. Parent/final symlinks,
special files and existing sidecars with additional hard links are rejected.
This shares the application's same-account trust boundary; it is not an OS
sandbox against another process controlling the user's files.

## Limits and cancellation

- 200 rows by default, maximum 1,000; serialized output is capped at 1 MB.
- Five seconds by default; CLI `--timeout-ms` and API `timeout_ms` accept
  1–10,000 ms. Model tools also respect their configured tool timeout.
- Eight simultaneous readers per process; excess requests fail with a retry hint.
- SQL and parameter data each have a 64 KB limit, with at most 128 parameters
  and 128 result columns. SQLite values/rows are limited to 1 MB.
- Ten million VM instructions, reduced parser/VM limits, an approximately 2 MB
  configured page cache, and no auxiliary SQLite threads. Queries requiring
  temporary disk spill fail instead of keeping an unbounded in-memory working
  set or creating temporary files. Use indexed or narrower queries.

Cancellation/deadlines use SQLite's interrupt handle and progress callback.
Abandoning a request cancels its worker, which retains its reader permit until
the connection closes. SQLite interruption is cooperative; a stalled kernel
filesystem operation may delay return. Tests check that cancellation releases
locks for a subsequent writer.

The locked native library bundles SQLite 3.53.2, including the upstream
[WAL-reset fix](https://www.sqlite.org/wal.html#walreset). This also updates
ShadowCode's history store; schema 24 and existing migration backups remain.
The implementation follows SQLite's [WAL documentation](https://www.sqlite.org/wal.html),
[VFS contract](https://www.sqlite.org/c3ref/vfs.html), and
[untrusted-SQL guidance](https://www.sqlite.org/security.html).

The [verification record](archive/NATIVE_VERIFICATION.md#real-local-models) includes real
gpt-oss and Qwen runs that discover a disposable database's schema, execute the
correct aggregate query and report its result without changing database bytes.
Successful SQLite reads also count toward required task inspection.
