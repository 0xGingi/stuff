use clap::{Args, Parser, Subcommand};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use stuff::{Operator, Query, StuffDb, StuffError, Transaction};

type CliResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Parser)]
#[command(name = "stuff")]
#[command(about = "A tiny JSON-file document database. Serious storage, unserious name.")]
struct Cli {
    #[arg(short, long, default_value = "stuff.json", global = true)]
    file: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init,
    Stats,
    Backup {
        target: PathBuf,
    },
    Batch {
        operations: String,
    },
    Collections {
        #[command(subcommand)]
        command: CollectionCommand,
    },
    Insert {
        collection: String,
        document: String,
        #[arg(long)]
        id: Option<String>,
    },
    Get {
        collection: String,
        id: String,
    },
    Replace {
        collection: String,
        id: String,
        document: String,
    },
    Patch {
        collection: String,
        id: String,
        patch: String,
    },
    Upsert {
        collection: String,
        id: String,
        document: String,
    },
    Delete {
        collection: String,
        id: String,
    },
    Query(QueryArgs),
    Indexes {
        #[command(subcommand)]
        command: IndexCommand,
    },
    Sample,
}

#[derive(Debug, Subcommand)]
enum CollectionCommand {
    List,
    Create { name: String },
    Drop { name: String },
}

#[derive(Debug, Args)]
struct QueryArgs {
    collection: String,
    #[arg(long = "filter", value_name = "FIELD:OP:JSON")]
    filters: Vec<String>,
    #[arg(long)]
    sort: Option<String>,
    #[arg(long)]
    desc: bool,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long, default_value_t = 0)]
    offset: usize,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum BatchOperation {
    CreateCollection {
        name: String,
    },
    DropCollection {
        name: String,
    },
    ListCollections,
    Insert {
        collection: String,
        document: Value,
        id: Option<String>,
    },
    Get {
        collection: String,
        id: String,
    },
    Replace {
        collection: String,
        id: String,
        document: Value,
    },
    Patch {
        collection: String,
        id: String,
        patch: Value,
    },
    Upsert {
        collection: String,
        id: String,
        document: Value,
    },
    Delete {
        collection: String,
        id: String,
    },
    Query {
        collection: String,
        query: BatchQuery,
    },
    CreateIndex {
        collection: String,
        field: String,
    },
    DropIndex {
        collection: String,
        field: String,
    },
    ListIndexes {
        collection: String,
    },
    Stats,
}

#[derive(Debug, Deserialize)]
struct BatchQuery {
    #[serde(default)]
    filters: Vec<BatchFilter>,
    sort: Option<BatchSort>,
    limit: Option<usize>,
    #[serde(default)]
    offset: usize,
}

#[derive(Debug, Deserialize)]
struct BatchFilter {
    field: String,
    op: String,
    value: Value,
}

#[derive(Debug, Deserialize)]
struct BatchSort {
    field: String,
    #[serde(default)]
    desc: bool,
}

#[derive(Debug, Subcommand)]
enum IndexCommand {
    List { collection: String },
    Create { collection: String, field: String },
    Drop { collection: String, field: String },
}

fn main() -> CliResult<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init => {
            let db = StuffDb::open(&cli.file)?;
            print_json(&json!({
                "ok": true,
                "file": db.path(),
                "message": "stuff acquired"
            }))?;
        }
        Command::Stats => {
            let db = StuffDb::open(&cli.file)?;
            print_json(&db.stats())?;
        }
        Command::Backup { target } => {
            let db = StuffDb::open(&cli.file)?;
            db.backup(&target)?;
            print_json(&json!({"ok": true, "backup": target}))?;
        }
        Command::Batch { operations } => {
            let operations: Vec<BatchOperation> = serde_json::from_str(&operations)?;
            let mut db = StuffDb::open(&cli.file)?;
            let results = db.transaction(|tx| run_batch(tx, operations))?;
            print_json(&results)?;
        }
        Command::Collections { command } => {
            let mut db = StuffDb::open(&cli.file)?;
            match command {
                CollectionCommand::List => print_json(&db.list_collections())?,
                CollectionCommand::Create { name } => {
                    db.create_collection(&name)?;
                    print_json(&json!({"ok": true, "collection": name}))?;
                }
                CollectionCommand::Drop { name } => {
                    db.drop_collection(&name)?;
                    print_json(&json!({"ok": true, "dropped": name}))?;
                }
            }
        }
        Command::Insert {
            collection,
            document,
            id,
        } => {
            let mut db = StuffDb::open(&cli.file)?;
            let document = parse_json(&document)?;
            let id = match id {
                Some(id) => {
                    db.insert_with_id(&collection, &id, document)?;
                    id
                }
                None => db.insert(&collection, document)?,
            };
            print_json(&json!({"ok": true, "id": id}))?;
        }
        Command::Get { collection, id } => {
            let db = StuffDb::open(&cli.file)?;
            let document = db.get(collection, id)?.unwrap_or(Value::Null);
            print_json(&document)?;
        }
        Command::Replace {
            collection,
            id,
            document,
        } => {
            let mut db = StuffDb::open(&cli.file)?;
            db.replace(&collection, &id, parse_json(&document)?)?;
            print_json(&json!({"ok": true, "id": id}))?;
        }
        Command::Patch {
            collection,
            id,
            patch,
        } => {
            let mut db = StuffDb::open(&cli.file)?;
            let updated = db.merge_patch(&collection, &id, parse_json(&patch)?)?;
            print_json(&updated)?;
        }
        Command::Upsert {
            collection,
            id,
            document,
        } => {
            let mut db = StuffDb::open(&cli.file)?;
            db.upsert(&collection, &id, parse_json(&document)?)?;
            print_json(&json!({"ok": true, "id": id}))?;
        }
        Command::Delete { collection, id } => {
            let mut db = StuffDb::open(&cli.file)?;
            let deleted = db.delete(&collection, &id)?;
            print_json(&deleted)?;
        }
        Command::Query(args) => {
            let db = StuffDb::open(&cli.file)?;
            let collection = args.collection.clone();
            let results = db.find(&collection, args.into_query()?)?;
            print_json(&results)?;
        }
        Command::Indexes { command } => {
            let mut db = StuffDb::open(&cli.file)?;
            match command {
                IndexCommand::List { collection } => print_json(&db.list_indexes(collection)?)?,
                IndexCommand::Create { collection, field } => {
                    db.create_index(&collection, &field)?;
                    print_json(&json!({"ok": true, "collection": collection, "field": field}))?;
                }
                IndexCommand::Drop { collection, field } => {
                    db.drop_index(&collection, &field)?;
                    print_json(&json!({"ok": true, "collection": collection, "dropped": field}))?;
                }
            }
        }
        Command::Sample => {
            print_json(&stuff::sample_document())?;
        }
    }

    Ok(())
}

impl QueryArgs {
    fn into_query(self) -> CliResult<Query> {
        let mut query = Query::new().offset(self.offset);

        for filter in self.filters {
            let (field, op, value) = parse_filter(&filter)?;
            query = query.filter(field, op, value);
        }

        if let Some(sort) = self.sort {
            query = if self.desc {
                query.sort_desc(sort)
            } else {
                query.sort_by(sort)
            };
        }

        if let Some(limit) = self.limit {
            query = query.limit(limit);
        }

        Ok(query)
    }
}

impl BatchQuery {
    fn into_query(self) -> stuff::Result<Query> {
        let mut query = Query::new().offset(self.offset);

        for filter in self.filters {
            query = query.filter(filter.field, parse_operator(&filter.op)?, filter.value);
        }

        if let Some(sort) = self.sort {
            query = if sort.desc {
                query.sort_desc(sort.field)
            } else {
                query.sort_by(sort.field)
            };
        }

        if let Some(limit) = self.limit {
            query = query.limit(limit);
        }

        Ok(query)
    }
}

fn run_batch(tx: &mut Transaction, operations: Vec<BatchOperation>) -> stuff::Result<Vec<Value>> {
    let mut results = Vec::with_capacity(operations.len());

    for operation in operations {
        let result = match operation {
            BatchOperation::CreateCollection { name } => {
                tx.create_collection(name)?;
                json!({"ok": true})
            }
            BatchOperation::DropCollection { name } => {
                tx.drop_collection(name)?;
                json!({"ok": true})
            }
            BatchOperation::ListCollections => json!(tx.list_collections()),
            BatchOperation::Insert {
                collection,
                document,
                id,
            } => {
                let id = match id {
                    Some(id) => {
                        tx.insert_with_id(collection, &id, document)?;
                        id
                    }
                    None => tx.insert(collection, document)?,
                };
                json!({"id": id})
            }
            BatchOperation::Get { collection, id } => {
                tx.get(collection, id)?.unwrap_or(Value::Null)
            }
            BatchOperation::Replace {
                collection,
                id,
                document,
            } => {
                tx.replace(collection, id, document)?;
                json!({"ok": true})
            }
            BatchOperation::Patch {
                collection,
                id,
                patch,
            } => tx.merge_patch(collection, id, patch)?,
            BatchOperation::Upsert {
                collection,
                id,
                document,
            } => {
                tx.upsert(collection, id, document)?;
                json!({"ok": true})
            }
            BatchOperation::Delete { collection, id } => tx.delete(collection, id)?,
            BatchOperation::Query { collection, query } => {
                json!(tx.find(collection, query.into_query()?)?)
            }
            BatchOperation::CreateIndex { collection, field } => {
                tx.create_index(collection, field)?;
                json!({"ok": true})
            }
            BatchOperation::DropIndex { collection, field } => {
                tx.drop_index(collection, field)?;
                json!({"ok": true})
            }
            BatchOperation::ListIndexes { collection } => json!(tx.list_indexes(collection)?),
            BatchOperation::Stats => json!(tx.stats()),
        };

        results.push(result);
    }

    Ok(results)
}

fn parse_filter(spec: &str) -> CliResult<(String, Operator, Value)> {
    let mut parts = spec.splitn(3, ':');
    let field = parts
        .next()
        .filter(|part| !part.is_empty())
        .ok_or("filter field is required")?;
    let op = parts.next().ok_or("filter operator is required")?;
    let raw_value = parts.next().ok_or("filter JSON value is required")?;

    Ok((
        field.to_string(),
        parse_operator(op)?,
        parse_json(raw_value)?,
    ))
}

fn parse_operator(op: &str) -> stuff::Result<Operator> {
    let operator = match op {
        "eq" => Operator::Eq,
        "ne" => Operator::Ne,
        "gt" => Operator::Gt,
        "gte" => Operator::Gte,
        "lt" => Operator::Lt,
        "lte" => Operator::Lte,
        "contains" => Operator::Contains,
        "exists" => Operator::Exists,
        _ => {
            return Err(StuffError::InvalidRequest(format!(
                "unknown filter operator: {op}"
            )));
        }
    };

    Ok(operator)
}

fn parse_json(input: &str) -> CliResult<Value> {
    Ok(serde_json::from_str(input)?)
}

fn print_json(value: &impl serde::Serialize) -> CliResult<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
