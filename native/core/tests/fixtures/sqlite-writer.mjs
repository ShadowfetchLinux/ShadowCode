// Independent live-WAL writer for native read-only interoperability checks.
import { DatabaseSync } from "node:sqlite";
import { createInterface } from "node:readline";
const db = new DatabaseSync(process.argv[2]);
db.exec("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE live(value INTEGER); INSERT INTO live VALUES(1)");
console.log("ready");
for await (const line of createInterface({ input: process.stdin })) {
  if (line === "next") { db.exec("INSERT INTO live VALUES(2)"); console.log("updated"); }
  else if (line === "stop") break;
}
db.close();
