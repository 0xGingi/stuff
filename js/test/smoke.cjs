const assert = require("node:assert/strict");
const { mkdtemp, rm } = require("node:fs/promises");
const { tmpdir } = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { StuffDb } = require("../dist/index.js");

test("uses stuff from JavaScript", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "stuff-js-"));

  try {
    const db = new StuffDb({ file: path.join(dir, "stuff.json") });

    await db.init();
    await db.createCollection({ name: "snacks" });
    await db.createIndex({ collection: "snacks", field: "rating" });

    await db.insertWithId({
      collection: "snacks",
      id: "pretzel",
      document: { name: "emergency pretzels", rating: 9 },
    });

    const snacks = await db.find({
      collection: "snacks",
      query: {
        filters: [{ field: "rating", op: "gte", value: 8 }],
        sort: { field: "rating", desc: true },
      },
    });

    assert.equal(snacks.length, 1);
    assert.equal(snacks[0].name, "emergency pretzels");

    const batchResults = await db.transaction((tx) => {
      tx.insertWithId({
        collection: "snacks",
        id: "pickle-chip",
        document: { name: "pickle chip", rating: 10 },
      });
      tx.stats();
    });

    assert.equal(batchResults.length, 2);
    assert.deepEqual(await db.listIndexes({ collection: "snacks" }), ["rating"]);

    const stats = await db.stats();
    assert.equal(stats.collections, 1);
    assert.equal(stats.documents, 2);
    assert.equal(stats.indexes, 1);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
