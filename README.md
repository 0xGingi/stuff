# stuff

`stuff` is a tiny document database that puts the whole database in one JSON file.

Why have many files when one file do trick?

```text
stuff.json
|-- collections
|-- documents
|-- indexes
|-- metadata
|-- transactions
|-- questionable-decisions
`-- stuff-we-should-name-later
```

Folder structure?
That is just stuff hiding from Ctrl+F.

Schema?
Documents are JSON objects. Each one gets an `_id`. That is the opinion.

Validation?
If `stuff` rejects it, it was bad stuff.

Performance?
JSON parse once. Vibes until the file gets too large.

Enterprise version?
`StuffCloud(TM)`, legally imaginary.

## what it actually does

- Stores everything in one readable JSON file.
- Keeps documents in named collections.
- Adds automatic `_id` values, unless you bring your own.
- Does CRUD: insert, get, replace, patch, upsert, delete.
- Queries with `eq`, `ne`, `gt`, `gte`, `lt`, `lte`, `contains`, and `exists`.
- Reads dot paths like `profile.email` and array indexes like `items.0.name`.
- Sorts, limits, and offsets.
- Saves index definitions and rebuilds in-memory indexes on open.
- Runs transactions by cloning the database, doing the stuff, then saving once.
- Makes backups.
- Reports stats.
- Has a Rust API.
- Has a CLI.
- Has a JavaScript/TypeScript client, because JavaScript saw Rust having a file and wanted to touch it.

## install

For Rust:

```sh
cargo install --path .
```

Or run it in place:

```sh
cargo run -- --help
```

For JavaScript and TypeScript:

```sh
npm install
npm run build
```

The Node client uses the Rust binary. It looks for `target/release/stuff`, then `target/debug/stuff`, then `stuff` on your `PATH`.

If the binary lives elsewhere:

```sh
STUFF_BIN=/path/to/stuff node app.js
```

## cli speedrun

Question: where does this data go?

Answer:
Is it stuff?

Yes -> `stuff.json`
No -> inspect your assumptions, then `stuff.json`

```sh
stuff --file pantry.json init
stuff --file pantry.json collections create snacks
stuff --file pantry.json insert snacks '{"name":"emergency pretzels","rating":9,"tags":["salty","load-bearing"]}'
stuff --file pantry.json indexes create snacks rating
stuff --file pantry.json query snacks --filter 'rating:gte:8' --sort rating --desc
stuff --file pantry.json stats
stuff --file pantry.json backup pantry.backup.json
```

Need a sample document because thinking is closed for maintenance:

```sh
stuff sample
```

## rust usage

```rust
use serde_json::json;
use stuff::{Query, StuffDb};

fn main() -> stuff::Result<()> {
    let mut db = StuffDb::open("stuff.json")?;

    db.create_collection("relics")?;
    db.insert_with_id(
        "relics",
        "sock",
        json!({
            "name": "Sock",
            "kind": "desk relic",
            "snacks": 14
        }),
    )?;

    db.merge_patch("relics", "sock", json!({"snacks": 15}))?;

    let well_fed = db.find(
        "relics",
        Query::new()
            .gte("snacks", 10)
            .sort_desc("snacks")
            .limit(5),
    )?;

    println!("{well_fed:#?}");
    Ok(())
}
```

## javascript and typescript usage

TypeScript loader philosophy, but with fewer lies to the compiler:

```ts
import { StuffDb } from "stuff-json-db";

const db = new StuffDb({ file: "stuff.json" });

await db.init();
await db.createCollection({ name: "snacks" });
await db.createIndex({ collection: "snacks", field: "rating" });

await db.insertWithId({
  collection: "snacks",
  id: "pretzel",
  document: {
    name: "emergency pretzels",
    rating: 9,
    tags: ["salty", "load-bearing"],
  },
});

const snacks = await db.find({
  collection: "snacks",
  query: {
    filters: [{ field: "rating", op: "gte", value: 8 }],
    sort: { field: "rating", desc: true },
    limit: 5,
  },
});

console.log(snacks);
```

CommonJS also gets stuff:

```js
const { StuffDb } = require("stuff-json-db");
```

## transactions

But early on?

```text
stuff.json was not bad architecture.
stuff.json was honest architecture.
```

Rust transactions:

```rust
# use serde_json::json;
# use stuff::StuffDb;
# fn run() -> stuff::Result<()> {
# let mut db = StuffDb::open("stuff.json")?;
db.transaction(|tx| {
    tx.insert_with_id("snacks", "pickle-chip", json!({"crunch": 10}))?;
    tx.insert_with_id("snacks", "suspicious-raisin", json!({"crunch": -4}))?;
    Ok(())
})?;
# Ok(())
# }
```

JS transactions collect operations in memory and send them to Rust as one batch:

```ts
await db.transaction((tx) => {
  tx.insertWithId({
    collection: "snacks",
    id: "pickle-chip",
    document: { crunch: 10 },
  });
  tx.patch({
    collection: "snacks",
    id: "pretzel",
    patch: { rating: 10 },
  });
});
```

If the transaction returns an error, nothing gets saved.

The database looks at the failed batch and says:

```text
That never happened.
```

## file format

Final form:

```text
stuff.json
```

Inside the final form:

```json
{
  "meta": {
    "version": 1,
    "created_at": "2026-06-22T20:00:00Z",
    "updated_at": "2026-06-22T20:00:01Z",
    "tx": 3
  },
  "collections": {
    "snacks": {
      "documents": {
        "pickle-chip": {
          "_id": "pickle-chip",
          "crunch": 10
        }
      },
      "indexed_fields": ["crunch"]
    }
  }
}
```

You can read it. You can diff it. You can copy it. You can open it at midnight and ask why the app is like this.

The answer is in the filename.

## development

```sh
cargo test
cargo fmt
cargo clippy --all-targets --all-features
npm test
```