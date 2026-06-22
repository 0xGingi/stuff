import { spawn } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";

export type JsonPrimitive = null | boolean | number | string;
export type JsonValue = JsonPrimitive | readonly JsonValue[] | JsonObject;
export type JsonObject = { readonly [key: string]: JsonValue };

export type FilterOperator =
  | "eq"
  | "ne"
  | "gt"
  | "gte"
  | "lt"
  | "lte"
  | "contains"
  | "exists";

export type Filter = Readonly<{
  field: string;
  op: FilterOperator;
  value: JsonValue;
}>;

export type Query = Readonly<{
  filters?: readonly Filter[];
  sort?: Readonly<{
    field: string;
    desc?: boolean;
  }>;
  limit?: number;
  offset?: number;
}>;

export type CollectionStats = Readonly<{
  documents: number;
  indexes: number;
}>;

export type Stats = Readonly<{
  collections: number;
  documents: number;
  indexes: number;
  by_collection: Readonly<Record<string, CollectionStats>>;
}>;

export type StuffDbOptions = Readonly<{
  file: string;
  binary?: string;
}>;

export type BackupParams = Readonly<{
  target: string;
}>;

export type CollectionParams = Readonly<{
  name: string;
}>;

export type CollectionDocumentParams = Readonly<{
  collection: string;
  document: JsonObject;
  id?: string;
}>;

export type DocumentIdParams = Readonly<{
  collection: string;
  id: string;
}>;

export type ReplaceParams = Readonly<{
  collection: string;
  id: string;
  document: JsonObject;
}>;

export type PatchParams = Readonly<{
  collection: string;
  id: string;
  patch: JsonObject;
}>;

export type QueryParams = Readonly<{
  collection: string;
  query?: Query;
}>;

export type IndexParams = Readonly<{
  collection: string;
  field: string;
}>;

export type BatchOperation =
  | Readonly<{ op: "create_collection"; name: string }>
  | Readonly<{ op: "drop_collection"; name: string }>
  | Readonly<{ op: "list_collections" }>
  | Readonly<{
      op: "insert";
      collection: string;
      document: JsonObject;
      id?: string;
    }>
  | Readonly<{ op: "get"; collection: string; id: string }>
  | Readonly<{
      op: "replace";
      collection: string;
      id: string;
      document: JsonObject;
    }>
  | Readonly<{
      op: "patch";
      collection: string;
      id: string;
      patch: JsonObject;
    }>
  | Readonly<{
      op: "upsert";
      collection: string;
      id: string;
      document: JsonObject;
    }>
  | Readonly<{ op: "delete"; collection: string; id: string }>
  | Readonly<{ op: "query"; collection: string; query: BatchQuery }>
  | Readonly<{ op: "create_index"; collection: string; field: string }>
  | Readonly<{ op: "drop_index"; collection: string; field: string }>
  | Readonly<{ op: "list_indexes"; collection: string }>
  | Readonly<{ op: "stats" }>;

export type BatchQuery = Readonly<{
  filters: readonly Filter[];
  sort?: Readonly<{
    field: string;
    desc?: boolean;
  }>;
  limit?: number;
  offset: number;
}>;

export class StuffCommandError extends Error {
  readonly code: number | null;
  readonly stdout: string;
  readonly stderr: string;

  constructor(params: {
    readonly message: string;
    readonly code: number | null;
    readonly stdout: string;
    readonly stderr: string;
  }) {
    super(params.message);
    this.name = "StuffCommandError";
    this.code = params.code;
    this.stdout = params.stdout;
    this.stderr = params.stderr;
  }
}

export class StuffDb {
  readonly file: string;
  readonly binary: string;

  constructor(options: StuffDbOptions) {
    this.file = options.file;
    this.binary = options.binary ?? defaultBinaryPath();
  }

  async init(): Promise<void> {
    expectOk(await this.command(["init"]));
  }

  async stats(): Promise<Stats> {
    return expectStats(await this.command(["stats"]));
  }

  async backup(params: BackupParams): Promise<void> {
    expectOk(await this.command(["backup", params.target]));
  }

  async listCollections(): Promise<readonly string[]> {
    return expectStringArray(
      await this.command(["collections", "list"]),
      "collections",
    );
  }

  async createCollection(params: CollectionParams): Promise<void> {
    expectOk(await this.command(["collections", "create", params.name]));
  }

  async dropCollection(params: CollectionParams): Promise<void> {
    expectOk(await this.command(["collections", "drop", params.name]));
  }

  async insert(params: CollectionDocumentParams): Promise<string> {
    const args = ["insert", params.collection, JSON.stringify(params.document)];
    if (params.id !== undefined) {
      args.push("--id", params.id);
    }

    return expectIdObject(await this.command(args)).id;
  }

  async insertWithId(params: RequiredIdDocumentParams): Promise<void> {
    await this.insert({
      collection: params.collection,
      id: params.id,
      document: params.document,
    });
  }

  async get(params: DocumentIdParams): Promise<JsonObject | null> {
    return expectDocumentOrNull(
      await this.command(["get", params.collection, params.id]),
      "document",
    );
  }

  async replace(params: ReplaceParams): Promise<void> {
    expectOk(
      await this.command([
        "replace",
        params.collection,
        params.id,
        JSON.stringify(params.document),
      ]),
    );
  }

  async patch(params: PatchParams): Promise<JsonObject> {
    return expectDocument(
      await this.command([
        "patch",
        params.collection,
        params.id,
        JSON.stringify(params.patch),
      ]),
      "patched document",
    );
  }

  async upsert(params: RequiredIdDocumentParams): Promise<void> {
    expectOk(
      await this.command([
        "upsert",
        params.collection,
        params.id,
        JSON.stringify(params.document),
      ]),
    );
  }

  async delete(params: DocumentIdParams): Promise<JsonObject> {
    return expectDocument(
      await this.command(["delete", params.collection, params.id]),
      "deleted document",
    );
  }

  async find(params: QueryParams): Promise<readonly JsonObject[]> {
    const args = ["query", params.collection];
    appendQueryArgs(args, params.query);
    return expectDocuments(await this.command(args), "query result");
  }

  async createIndex(params: IndexParams): Promise<void> {
    expectOk(
      await this.command(["indexes", "create", params.collection, params.field]),
    );
  }

  async dropIndex(params: IndexParams): Promise<void> {
    expectOk(
      await this.command(["indexes", "drop", params.collection, params.field]),
    );
  }

  async listIndexes(
    params: Readonly<{ collection: string }>,
  ): Promise<readonly string[]> {
    return expectStringArray(
      await this.command(["indexes", "list", params.collection]),
      "indexes",
    );
  }

  async sample(): Promise<JsonObject> {
    return expectDocument(await this.command(["sample"]), "sample document");
  }

  async batch(operations: readonly BatchOperation[]): Promise<readonly JsonValue[]> {
    return expectJsonArray(
      await this.command(["batch", JSON.stringify(operations)]),
      "batch result",
    );
  }

  async transaction(
    action: (tx: StuffTransaction) => void | Promise<void>,
  ): Promise<readonly JsonValue[]> {
    const tx = new StuffTransaction();
    await action(tx);
    return this.batch(tx.operations);
  }

  private async command(args: readonly string[]): Promise<unknown> {
    const stdout = await runProcess(this.binary, [
      "--file",
      this.file,
      ...args,
    ]);
    return parseJson(stdout);
  }
}

export type RequiredIdDocumentParams = Readonly<{
  collection: string;
  id: string;
  document: JsonObject;
}>;

export class StuffTransaction {
  readonly operations: BatchOperation[] = [];

  createCollection(params: CollectionParams): this {
    this.operations.push({
      op: "create_collection",
      name: params.name,
    } satisfies BatchOperation);
    return this;
  }

  dropCollection(params: CollectionParams): this {
    this.operations.push({
      op: "drop_collection",
      name: params.name,
    } satisfies BatchOperation);
    return this;
  }

  listCollections(): this {
    this.operations.push({ op: "list_collections" } satisfies BatchOperation);
    return this;
  }

  insert(params: CollectionDocumentParams): this {
    if (params.id === undefined) {
      this.operations.push({
        op: "insert",
        collection: params.collection,
        document: params.document,
      } satisfies BatchOperation);
    } else {
      this.operations.push({
        op: "insert",
        collection: params.collection,
        document: params.document,
        id: params.id,
      } satisfies BatchOperation);
    }
    return this;
  }

  insertWithId(params: RequiredIdDocumentParams): this {
    return this.insert({
      collection: params.collection,
      id: params.id,
      document: params.document,
    });
  }

  get(params: DocumentIdParams): this {
    this.operations.push({
      op: "get",
      collection: params.collection,
      id: params.id,
    } satisfies BatchOperation);
    return this;
  }

  replace(params: ReplaceParams): this {
    this.operations.push({
      op: "replace",
      collection: params.collection,
      id: params.id,
      document: params.document,
    } satisfies BatchOperation);
    return this;
  }

  patch(params: PatchParams): this {
    this.operations.push({
      op: "patch",
      collection: params.collection,
      id: params.id,
      patch: params.patch,
    } satisfies BatchOperation);
    return this;
  }

  upsert(params: RequiredIdDocumentParams): this {
    this.operations.push({
      op: "upsert",
      collection: params.collection,
      id: params.id,
      document: params.document,
    } satisfies BatchOperation);
    return this;
  }

  delete(params: DocumentIdParams): this {
    this.operations.push({
      op: "delete",
      collection: params.collection,
      id: params.id,
    } satisfies BatchOperation);
    return this;
  }

  find(params: QueryParams): this {
    this.operations.push({
      op: "query",
      collection: params.collection,
      query: normalizeQuery(params.query),
    } satisfies BatchOperation);
    return this;
  }

  createIndex(params: IndexParams): this {
    this.operations.push({
      op: "create_index",
      collection: params.collection,
      field: params.field,
    } satisfies BatchOperation);
    return this;
  }

  dropIndex(params: IndexParams): this {
    this.operations.push({
      op: "drop_index",
      collection: params.collection,
      field: params.field,
    } satisfies BatchOperation);
    return this;
  }

  listIndexes(params: Readonly<{ collection: string }>): this {
    this.operations.push({
      op: "list_indexes",
      collection: params.collection,
    } satisfies BatchOperation);
    return this;
  }

  stats(): this {
    this.operations.push({ op: "stats" } satisfies BatchOperation);
    return this;
  }
}

export function openStuff(options: StuffDbOptions): StuffDb {
  return new StuffDb(options);
}

function appendQueryArgs(args: string[], query: Query | undefined): void {
  const normalized = normalizeQuery(query);

  for (const filter of normalized.filters) {
    args.push(
      "--filter",
      `${filter.field}:${filter.op}:${JSON.stringify(filter.value)}`,
    );
  }

  if (normalized.sort !== undefined) {
    args.push("--sort", normalized.sort.field);
    if (normalized.sort.desc === true) {
      args.push("--desc");
    }
  }

  if (normalized.limit !== undefined) {
    args.push("--limit", normalized.limit.toString());
  }

  if (normalized.offset > 0) {
    args.push("--offset", normalized.offset.toString());
  }
}

function normalizeQuery(query: Query | undefined): BatchQuery {
  const normalized: BatchQuery = {
    filters: query?.filters ?? [],
    offset: query?.offset ?? 0,
  };

  if (query?.sort !== undefined) {
    return query.limit === undefined
      ? { ...normalized, sort: query.sort }
      : { ...normalized, sort: query.sort, limit: query.limit };
  }

  if (query?.limit !== undefined) {
    return { ...normalized, limit: query.limit };
  }

  return normalized;
}

function defaultBinaryPath(): string {
  const configured = process.env.STUFF_BIN;
  if (configured !== undefined && configured.length > 0) {
    return configured;
  }

  const executable = process.platform === "win32" ? "stuff.exe" : "stuff";
  const candidates = [
    path.resolve(__dirname, "..", "..", "target", "release", executable),
    path.resolve(__dirname, "..", "..", "target", "debug", executable),
  ];

  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }

  return executable;
}

function runProcess(binary: string, args: readonly string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, args, {
      stdio: ["ignore", "pipe", "pipe"],
    });
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];

    if (child.stdout === null || child.stderr === null) {
      reject(new Error("failed to open stuff process pipes"));
      return;
    }

    child.stdout.on("data", (chunk: Buffer) => {
      stdout.push(chunk);
    });
    child.stderr.on("data", (chunk: Buffer) => {
      stderr.push(chunk);
    });
    child.on("error", (error) => {
      reject(error);
    });
    child.on("close", (code) => {
      const stdoutText = Buffer.concat(stdout).toString("utf8");
      const stderrText = Buffer.concat(stderr).toString("utf8");
      if (code === 0) {
        resolve(stdoutText);
      } else {
        reject(
          new StuffCommandError({
            message: stderrText.trim() || `stuff exited with code ${code}`,
            code,
            stdout: stdoutText,
            stderr: stderrText,
          }),
        );
      }
    });
  });
}

function parseJson(text: string): unknown {
  const parsed: unknown = JSON.parse(text);
  return parsed;
}

function expectOk(value: unknown): void {
  if (!isRecord(value) || value.ok !== true) {
    throw new Error("stuff returned a non-ok response");
  }
}

function expectIdObject(value: unknown): Readonly<{ id: string }> {
  if (!isRecord(value) || typeof value.id !== "string") {
    throw new Error("stuff returned a response without an id");
  }

  return { id: value.id };
}

function expectStats(value: unknown): Stats {
  if (!isStats(value)) {
    throw new Error("stuff returned invalid stats");
  }

  return value;
}

function expectStringArray(value: unknown, label: string): readonly string[] {
  if (!isStringArray(value)) {
    throw new Error(`stuff returned invalid ${label}`);
  }

  return value;
}

function expectDocument(value: unknown, label: string): JsonObject {
  if (!isJsonObject(value)) {
    throw new Error(`stuff returned invalid ${label}`);
  }

  return value;
}

function expectDocumentOrNull(value: unknown, label: string): JsonObject | null {
  if (value === null) {
    return null;
  }

  return expectDocument(value, label);
}

function expectDocuments(value: unknown, label: string): readonly JsonObject[] {
  if (!isDocumentArray(value)) {
    throw new Error(`stuff returned invalid ${label}`);
  }

  return value;
}

function expectJsonArray(value: unknown, label: string): readonly JsonValue[] {
  if (!Array.isArray(value) || !value.every(isJsonValue)) {
    throw new Error(`stuff returned invalid ${label}`);
  }

  return value;
}

function isStats(value: unknown): value is Stats {
  if (
    !isRecord(value) ||
    typeof value.collections !== "number" ||
    typeof value.documents !== "number" ||
    typeof value.indexes !== "number" ||
    !isRecord(value.by_collection)
  ) {
    return false;
  }

  return Object.values(value.by_collection).every(isCollectionStats);
}

function isCollectionStats(value: unknown): value is CollectionStats {
  return (
    isRecord(value) &&
    typeof value.documents === "number" &&
    typeof value.indexes === "number"
  );
}

function isStringArray(value: unknown): value is readonly string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

function isDocumentArray(value: unknown): value is readonly JsonObject[] {
  return Array.isArray(value) && value.every(isJsonObject);
}

function isJsonValue(value: unknown): value is JsonValue {
  if (value === null) {
    return true;
  }

  switch (typeof value) {
    case "boolean":
    case "string":
      return true;
    case "number":
      return Number.isFinite(value);
    case "object":
      if (Array.isArray(value)) {
        return value.every(isJsonValue);
      }
      return isJsonObject(value);
    default:
      return false;
  }
}

function isJsonObject(value: unknown): value is JsonObject {
  return isRecord(value) && Object.values(value).every(isJsonValue);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
