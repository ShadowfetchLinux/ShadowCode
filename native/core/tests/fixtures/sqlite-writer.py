# Independent live-WAL writer for native read-only interoperability checks.
# Uses the system Python sqlite3 module so the test does not require Node 22+
# (Ubuntu /usr/bin/node is often 18 and has no node:sqlite).
import sqlite3
import sys

path = sys.argv[1]
db = sqlite3.connect(path)
db.execute("PRAGMA journal_mode=WAL")
db.execute("PRAGMA wal_autocheckpoint=0")
db.execute("CREATE TABLE live(value INTEGER)")
db.execute("INSERT INTO live VALUES(1)")
db.commit()
print("ready", flush=True)
for line in sys.stdin:
    command = line.strip()
    if command == "next":
        db.execute("INSERT INTO live VALUES(2)")
        db.commit()
        print("updated", flush=True)
    elif command == "stop":
        break
db.close()
