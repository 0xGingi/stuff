use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

const FORMAT_VERSION: u32 = 1;
const ID_FIELD: &str = "_id";

pub type Result<T> = std::result::Result<T, StuffError>;

#[derive(Debug, Error)]
pub enum StuffError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("collection already exists: {0}")]
    CollectionExists(String),
    #[error("collection does not exist: {0}")]
    CollectionMissing(String),
    #[error("document already exists in {collection}: {id}")]
    DocumentExists { collection: String, id: String },
    #[error("document does not exist in {collection}: {id}")]
    DocumentMissing { collection: String, id: String },
    #[error("collection names must contain only ASCII letters, numbers, underscores, and dashes")]
    InvalidCollectionName,
    #[error("document ids must be non-empty strings without control characters")]
    InvalidDocumentId,
    #[error("documents must be JSON objects")]
    DocumentMustBeObject,
    #[error("field path must not be empty")]
    InvalidFieldPath,
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("merge patches must be JSON objects")]
    PatchMustBeObject,
    #[error("cannot compare values at field `{field}` with operator `{op}`")]
    IncomparableValues { field: String, op: &'static str },
    #[error("invalid database format version {found}; expected {expected}")]
    UnsupportedFormat { found: u32, expected: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DatabaseFile {
    meta: Metadata,
    collections: BTreeMap<String, Collection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Metadata {
    version: u32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    tx: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Collection {
    documents: BTreeMap<String, Value>,
    indexed_fields: BTreeSet<String>,
    #[serde(skip)]
    indexes: BTreeMap<String, BTreeMap<String, BTreeSet<String>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    Contains,
    Exists,
}

#[derive(Debug, Clone)]
pub struct Filter {
    field: String,
    op: Operator,
    value: Value,
}

#[derive(Debug, Clone, Default)]
pub struct Query {
    filters: Vec<Filter>,
    sort: Option<Sort>,
    limit: Option<usize>,
    offset: usize,
}

#[derive(Debug, Clone)]
struct Sort {
    field: String,
    descending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Stats {
    pub collections: usize,
    pub documents: usize,
    pub indexes: usize,
    pub by_collection: BTreeMap<String, CollectionStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionStats {
    pub documents: usize,
    pub indexes: usize,
}

#[derive(Debug, Clone)]
pub struct StuffDb {
    path: PathBuf,
    store: DatabaseFile,
}

#[derive(Debug)]
pub struct Transaction {
    store: DatabaseFile,
}

impl Query {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn filter(
        mut self,
        field: impl Into<String>,
        op: Operator,
        value: impl Into<Value>,
    ) -> Self {
        self.filters.push(Filter {
            field: field.into(),
            op,
            value: value.into(),
        });
        self
    }

    pub fn eq(self, field: impl Into<String>, value: impl Into<Value>) -> Self {
        self.filter(field, Operator::Eq, value)
    }

    pub fn ne(self, field: impl Into<String>, value: impl Into<Value>) -> Self {
        self.filter(field, Operator::Ne, value)
    }

    pub fn gt(self, field: impl Into<String>, value: impl Into<Value>) -> Self {
        self.filter(field, Operator::Gt, value)
    }

    pub fn gte(self, field: impl Into<String>, value: impl Into<Value>) -> Self {
        self.filter(field, Operator::Gte, value)
    }

    pub fn lt(self, field: impl Into<String>, value: impl Into<Value>) -> Self {
        self.filter(field, Operator::Lt, value)
    }

    pub fn lte(self, field: impl Into<String>, value: impl Into<Value>) -> Self {
        self.filter(field, Operator::Lte, value)
    }

    pub fn contains(self, field: impl Into<String>, value: impl Into<Value>) -> Self {
        self.filter(field, Operator::Contains, value)
    }

    pub fn exists(self, field: impl Into<String>, exists: bool) -> Self {
        self.filter(field, Operator::Exists, Value::Bool(exists))
    }

    pub fn sort_by(mut self, field: impl Into<String>) -> Self {
        self.sort = Some(Sort {
            field: field.into(),
            descending: false,
        });
        self
    }

    pub fn sort_desc(mut self, field: impl Into<String>) -> Self {
        self.sort = Some(Sort {
            field: field.into(),
            descending: true,
        });
        self
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }
}

impl StuffDb {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        ensure_parent_dir(&path)?;

        if !path.exists() || fs::metadata(&path)?.len() == 0 {
            write_database_file(&path, &DatabaseFile::new())?;
        }

        let mut store = read_database_file(&path)?;
        store.validate()?;
        store.rebuild_indexes();

        Ok(Self { path, store })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn create_collection(&mut self, name: impl AsRef<str>) -> Result<()> {
        self.store.create_collection(name.as_ref())?;
        self.persist()
    }

    pub fn drop_collection(&mut self, name: impl AsRef<str>) -> Result<()> {
        self.store.drop_collection(name.as_ref())?;
        self.persist()
    }

    pub fn list_collections(&self) -> Vec<String> {
        self.store.collections.keys().cloned().collect()
    }

    pub fn insert(&mut self, collection: impl AsRef<str>, document: Value) -> Result<String> {
        let id = self.store.insert(collection.as_ref(), None, document)?;
        self.persist()?;
        Ok(id)
    }

    pub fn insert_with_id(
        &mut self,
        collection: impl AsRef<str>,
        id: impl AsRef<str>,
        document: Value,
    ) -> Result<()> {
        self.store
            .insert(collection.as_ref(), Some(id.as_ref()), document)?;
        self.persist()
    }

    pub fn get(&self, collection: impl AsRef<str>, id: impl AsRef<str>) -> Result<Option<Value>> {
        self.store.get(collection.as_ref(), id.as_ref())
    }

    pub fn replace(
        &mut self,
        collection: impl AsRef<str>,
        id: impl AsRef<str>,
        document: Value,
    ) -> Result<()> {
        self.store
            .replace(collection.as_ref(), id.as_ref(), document)?;
        self.persist()
    }

    pub fn merge_patch(
        &mut self,
        collection: impl AsRef<str>,
        id: impl AsRef<str>,
        patch: Value,
    ) -> Result<Value> {
        let updated = self
            .store
            .merge_patch(collection.as_ref(), id.as_ref(), patch)?;
        self.persist()?;
        Ok(updated)
    }

    pub fn upsert(
        &mut self,
        collection: impl AsRef<str>,
        id: impl AsRef<str>,
        document: Value,
    ) -> Result<()> {
        self.store
            .upsert(collection.as_ref(), id.as_ref(), document)?;
        self.persist()
    }

    pub fn delete(&mut self, collection: impl AsRef<str>, id: impl AsRef<str>) -> Result<Value> {
        let deleted = self.store.delete(collection.as_ref(), id.as_ref())?;
        self.persist()?;
        Ok(deleted)
    }

    pub fn all(&self, collection: impl AsRef<str>) -> Result<Vec<Value>> {
        self.find(collection, Query::new())
    }

    pub fn find(&self, collection: impl AsRef<str>, query: Query) -> Result<Vec<Value>> {
        self.store.find(collection.as_ref(), query)
    }

    pub fn create_index(
        &mut self,
        collection: impl AsRef<str>,
        field: impl AsRef<str>,
    ) -> Result<()> {
        self.store
            .create_index(collection.as_ref(), field.as_ref())?;
        self.persist()
    }

    pub fn drop_index(
        &mut self,
        collection: impl AsRef<str>,
        field: impl AsRef<str>,
    ) -> Result<()> {
        self.store.drop_index(collection.as_ref(), field.as_ref())?;
        self.persist()
    }

    pub fn list_indexes(&self, collection: impl AsRef<str>) -> Result<Vec<String>> {
        self.store.list_indexes(collection.as_ref())
    }

    pub fn stats(&self) -> Stats {
        self.store.stats()
    }

    pub fn backup(&self, target: impl AsRef<Path>) -> Result<()> {
        ensure_parent_dir(target.as_ref())?;
        fs::copy(&self.path, target)?;
        Ok(())
    }

    pub fn transaction<F, T>(&mut self, action: F) -> Result<T>
    where
        F: FnOnce(&mut Transaction) -> Result<T>,
    {
        let mut tx = Transaction {
            store: self.store.clone(),
        };
        let result = action(&mut tx)?;
        self.store = tx.store;
        self.persist()?;
        Ok(result)
    }

    fn persist(&mut self) -> Result<()> {
        self.store.touch();
        write_database_file(&self.path, &self.store)
    }
}

impl Transaction {
    pub fn create_collection(&mut self, name: impl AsRef<str>) -> Result<()> {
        self.store.create_collection(name.as_ref())
    }

    pub fn drop_collection(&mut self, name: impl AsRef<str>) -> Result<()> {
        self.store.drop_collection(name.as_ref())
    }

    pub fn list_collections(&self) -> Vec<String> {
        self.store.collections.keys().cloned().collect()
    }

    pub fn insert(&mut self, collection: impl AsRef<str>, document: Value) -> Result<String> {
        self.store.insert(collection.as_ref(), None, document)
    }

    pub fn insert_with_id(
        &mut self,
        collection: impl AsRef<str>,
        id: impl AsRef<str>,
        document: Value,
    ) -> Result<()> {
        self.store
            .insert(collection.as_ref(), Some(id.as_ref()), document)?;
        Ok(())
    }

    pub fn get(&self, collection: impl AsRef<str>, id: impl AsRef<str>) -> Result<Option<Value>> {
        self.store.get(collection.as_ref(), id.as_ref())
    }

    pub fn replace(
        &mut self,
        collection: impl AsRef<str>,
        id: impl AsRef<str>,
        document: Value,
    ) -> Result<()> {
        self.store
            .replace(collection.as_ref(), id.as_ref(), document)
    }

    pub fn merge_patch(
        &mut self,
        collection: impl AsRef<str>,
        id: impl AsRef<str>,
        patch: Value,
    ) -> Result<Value> {
        self.store
            .merge_patch(collection.as_ref(), id.as_ref(), patch)
    }

    pub fn upsert(
        &mut self,
        collection: impl AsRef<str>,
        id: impl AsRef<str>,
        document: Value,
    ) -> Result<()> {
        self.store
            .upsert(collection.as_ref(), id.as_ref(), document)
    }

    pub fn delete(&mut self, collection: impl AsRef<str>, id: impl AsRef<str>) -> Result<Value> {
        self.store.delete(collection.as_ref(), id.as_ref())
    }

    pub fn find(&self, collection: impl AsRef<str>, query: Query) -> Result<Vec<Value>> {
        self.store.find(collection.as_ref(), query)
    }

    pub fn create_index(
        &mut self,
        collection: impl AsRef<str>,
        field: impl AsRef<str>,
    ) -> Result<()> {
        self.store.create_index(collection.as_ref(), field.as_ref())
    }

    pub fn drop_index(
        &mut self,
        collection: impl AsRef<str>,
        field: impl AsRef<str>,
    ) -> Result<()> {
        self.store.drop_index(collection.as_ref(), field.as_ref())
    }

    pub fn list_indexes(&self, collection: impl AsRef<str>) -> Result<Vec<String>> {
        self.store.list_indexes(collection.as_ref())
    }

    pub fn stats(&self) -> Stats {
        self.store.stats()
    }
}

impl DatabaseFile {
    fn new() -> Self {
        let now = Utc::now();
        Self {
            meta: Metadata {
                version: FORMAT_VERSION,
                created_at: now,
                updated_at: now,
                tx: 0,
            },
            collections: BTreeMap::new(),
        }
    }

    fn validate(&self) -> Result<()> {
        if self.meta.version != FORMAT_VERSION {
            return Err(StuffError::UnsupportedFormat {
                found: self.meta.version,
                expected: FORMAT_VERSION,
            });
        }
        Ok(())
    }

    fn touch(&mut self) {
        self.meta.updated_at = Utc::now();
        self.meta.tx += 1;
    }

    fn create_collection(&mut self, name: &str) -> Result<()> {
        validate_collection_name(name)?;
        if self.collections.contains_key(name) {
            return Err(StuffError::CollectionExists(name.to_string()));
        }
        self.collections.insert(name.to_string(), Collection::new());
        Ok(())
    }

    fn drop_collection(&mut self, name: &str) -> Result<()> {
        if self.collections.remove(name).is_none() {
            return Err(StuffError::CollectionMissing(name.to_string()));
        }
        Ok(())
    }

    fn insert(
        &mut self,
        collection: &str,
        id: Option<&str>,
        mut document: Value,
    ) -> Result<String> {
        let id = normalize_document_id(id, &mut document)?;
        let collection_ref = self.collection_mut(collection)?;

        if collection_ref.documents.contains_key(&id) {
            return Err(StuffError::DocumentExists {
                collection: collection.to_string(),
                id,
            });
        }

        collection_ref.documents.insert(id.clone(), document);
        collection_ref.rebuild_indexes();
        Ok(id)
    }

    fn get(&self, collection: &str, id: &str) -> Result<Option<Value>> {
        validate_id(id)?;
        let collection_ref = self.collection(collection)?;
        Ok(collection_ref.documents.get(id).cloned())
    }

    fn replace(&mut self, collection: &str, id: &str, mut document: Value) -> Result<()> {
        validate_id(id)?;
        ensure_object(&document)?;
        set_document_id(&mut document, id);

        let collection_ref = self.collection_mut(collection)?;
        if !collection_ref.documents.contains_key(id) {
            return Err(StuffError::DocumentMissing {
                collection: collection.to_string(),
                id: id.to_string(),
            });
        }

        collection_ref.documents.insert(id.to_string(), document);
        collection_ref.rebuild_indexes();
        Ok(())
    }

    fn merge_patch(&mut self, collection: &str, id: &str, patch: Value) -> Result<Value> {
        validate_id(id)?;
        ensure_patch_object(&patch)?;

        let collection_ref = self.collection_mut(collection)?;
        let Some(existing) = collection_ref.documents.get_mut(id) else {
            return Err(StuffError::DocumentMissing {
                collection: collection.to_string(),
                id: id.to_string(),
            });
        };

        apply_merge_patch(existing, &patch);
        ensure_object(existing)?;
        set_document_id(existing, id);
        let updated = existing.clone();
        collection_ref.rebuild_indexes();
        Ok(updated)
    }

    fn upsert(&mut self, collection: &str, id: &str, mut document: Value) -> Result<()> {
        validate_id(id)?;
        ensure_object(&document)?;
        set_document_id(&mut document, id);

        let collection_ref = self.collection_mut(collection)?;
        collection_ref.documents.insert(id.to_string(), document);
        collection_ref.rebuild_indexes();
        Ok(())
    }

    fn delete(&mut self, collection: &str, id: &str) -> Result<Value> {
        validate_id(id)?;

        let collection_ref = self.collection_mut(collection)?;
        let Some(deleted) = collection_ref.documents.remove(id) else {
            return Err(StuffError::DocumentMissing {
                collection: collection.to_string(),
                id: id.to_string(),
            });
        };

        collection_ref.rebuild_indexes();
        Ok(deleted)
    }

    fn find(&self, collection: &str, query: Query) -> Result<Vec<Value>> {
        let collection_ref = self.collection(collection)?;
        query.validate()?;

        let mut rows = collection_ref.candidates(&query).into_iter().try_fold(
            Vec::new(),
            |mut rows, document| {
                if matches_all_filters(document, &query.filters)? {
                    rows.push(document.clone());
                }
                Ok::<_, StuffError>(rows)
            },
        )?;

        if let Some(sort) = query.sort {
            rows.sort_by(|left, right| {
                let left = get_path(left, &sort.field);
                let right = get_path(right, &sort.field);
                let ordering = compare_json_values(left, right);
                if sort.descending {
                    ordering.reverse()
                } else {
                    ordering
                }
            });
        }

        Ok(rows
            .into_iter()
            .skip(query.offset)
            .take(query.limit.unwrap_or(usize::MAX))
            .collect())
    }

    fn create_index(&mut self, collection: &str, field: &str) -> Result<()> {
        validate_field_path(field)?;
        let collection_ref = self.collection_mut(collection)?;
        collection_ref.indexed_fields.insert(field.to_string());
        collection_ref.rebuild_indexes();
        Ok(())
    }

    fn drop_index(&mut self, collection: &str, field: &str) -> Result<()> {
        validate_field_path(field)?;
        let collection_ref = self.collection_mut(collection)?;
        collection_ref.indexed_fields.remove(field);
        collection_ref.indexes.remove(field);
        Ok(())
    }

    fn list_indexes(&self, collection: &str) -> Result<Vec<String>> {
        Ok(self
            .collection(collection)?
            .indexed_fields
            .iter()
            .cloned()
            .collect())
    }

    fn stats(&self) -> Stats {
        let by_collection: BTreeMap<_, _> = self
            .collections
            .iter()
            .map(|(name, collection)| {
                (
                    name.clone(),
                    CollectionStats {
                        documents: collection.documents.len(),
                        indexes: collection.indexed_fields.len(),
                    },
                )
            })
            .collect();

        Stats {
            collections: by_collection.len(),
            documents: by_collection.values().map(|stats| stats.documents).sum(),
            indexes: by_collection.values().map(|stats| stats.indexes).sum(),
            by_collection,
        }
    }

    fn rebuild_indexes(&mut self) {
        for collection in self.collections.values_mut() {
            collection.rebuild_indexes();
        }
    }

    fn collection(&self, name: &str) -> Result<&Collection> {
        self.collections
            .get(name)
            .ok_or_else(|| StuffError::CollectionMissing(name.to_string()))
    }

    fn collection_mut(&mut self, name: &str) -> Result<&mut Collection> {
        self.collections
            .get_mut(name)
            .ok_or_else(|| StuffError::CollectionMissing(name.to_string()))
    }
}

impl Collection {
    fn new() -> Self {
        Self {
            documents: BTreeMap::new(),
            indexed_fields: BTreeSet::new(),
            indexes: BTreeMap::new(),
        }
    }

    fn rebuild_indexes(&mut self) {
        self.indexes.clear();

        for field in &self.indexed_fields {
            let mut index: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            for (id, document) in &self.documents {
                if let Some(value) = get_path(document, field) {
                    index
                        .entry(index_key(value))
                        .or_default()
                        .insert(id.to_string());
                }
            }
            self.indexes.insert(field.clone(), index);
        }
    }

    fn candidates<'a>(&'a self, query: &Query) -> Vec<&'a Value> {
        if let Some(ids) = query.filters.iter().find_map(|filter| {
            if filter.op != Operator::Eq {
                return None;
            }
            let index = self.indexes.get(&filter.field)?;
            let key = index_key(&filter.value);
            index.get(&key)
        }) {
            return ids
                .iter()
                .filter_map(|id| self.documents.get(id))
                .collect::<Vec<_>>();
        }

        self.documents.values().collect()
    }
}

impl Query {
    fn validate(&self) -> Result<()> {
        for filter in &self.filters {
            validate_field_path(&filter.field)?;
            if filter.op == Operator::Exists && !filter.value.is_boolean() {
                return Err(StuffError::IncomparableValues {
                    field: filter.field.clone(),
                    op: "exists",
                });
            }
        }
        if let Some(sort) = &self.sort {
            validate_field_path(&sort.field)?;
        }
        Ok(())
    }
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn read_database_file(path: &Path) -> Result<DatabaseFile> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    file.lock_shared()?;

    let mut contents = String::new();
    let read_result = file.read_to_string(&mut contents);
    let unlock_result = file.unlock();

    read_result?;
    unlock_result?;

    if contents.trim().is_empty() {
        return Ok(DatabaseFile::new());
    }

    Ok(serde_json::from_str(&contents)?)
}

fn write_database_file(path: &Path, store: &DatabaseFile) -> Result<()> {
    ensure_parent_dir(path)?;

    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    lock_file.lock_exclusive()?;

    let write_result = (|| {
        let mut bytes = serde_json::to_vec_pretty(store)?;
        bytes.push(b'\n');

        let tmp_path = temporary_path(path);
        {
            let mut tmp = File::create(&tmp_path)?;
            tmp.write_all(&bytes)?;
            tmp.sync_all()?;
        }

        fs::rename(&tmp_path, path)?;
        sync_parent(path)?;
        Ok::<_, StuffError>(())
    })();

    let unlock_result = lock_file.unlock();

    write_result?;
    unlock_result?;
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("stuff.json");
    path.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        Uuid::new_v4()
    ))
}

fn sync_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn validate_collection_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');

    if valid {
        Ok(())
    } else {
        Err(StuffError::InvalidCollectionName)
    }
}

fn validate_id(id: &str) -> Result<()> {
    if !id.is_empty() && !id.chars().any(char::is_control) {
        Ok(())
    } else {
        Err(StuffError::InvalidDocumentId)
    }
}

fn validate_field_path(field: &str) -> Result<()> {
    if field.trim().is_empty() {
        Err(StuffError::InvalidFieldPath)
    } else {
        Ok(())
    }
}

fn ensure_object(value: &Value) -> Result<()> {
    if value.is_object() {
        Ok(())
    } else {
        Err(StuffError::DocumentMustBeObject)
    }
}

fn ensure_patch_object(value: &Value) -> Result<()> {
    if value.is_object() {
        Ok(())
    } else {
        Err(StuffError::PatchMustBeObject)
    }
}

fn normalize_document_id(id: Option<&str>, document: &mut Value) -> Result<String> {
    ensure_object(document)?;

    let id = match id {
        Some(id) => {
            validate_id(id)?;
            id.to_string()
        }
        None => document
            .get(ID_FIELD)
            .map(|value| {
                value
                    .as_str()
                    .filter(|id| validate_id(id).is_ok())
                    .map(ToOwned::to_owned)
                    .ok_or(StuffError::InvalidDocumentId)
            })
            .transpose()?
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
    };

    set_document_id(document, &id);
    Ok(id)
}

fn set_document_id(document: &mut Value, id: &str) {
    if let Value::Object(object) = document {
        object.insert(ID_FIELD.to_string(), Value::String(id.to_string()));
    }
}

fn get_path<'a>(value: &'a Value, field: &str) -> Option<&'a Value> {
    let mut current = value;

    for part in field.split('.') {
        current = match current {
            Value::Object(object) => object.get(part)?,
            Value::Array(array) => array.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }

    Some(current)
}

fn index_key(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn matches_all_filters(document: &Value, filters: &[Filter]) -> Result<bool> {
    for filter in filters {
        if !matches_filter(document, filter)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn matches_filter(document: &Value, filter: &Filter) -> Result<bool> {
    let actual = get_path(document, &filter.field);

    match filter.op {
        Operator::Eq => Ok(actual == Some(&filter.value)),
        Operator::Ne => Ok(actual != Some(&filter.value)),
        Operator::Exists => Ok(actual.is_some() == filter.value.as_bool().unwrap_or(false)),
        Operator::Contains => Ok(matches_contains(actual, &filter.value)),
        Operator::Gt | Operator::Gte | Operator::Lt | Operator::Lte => {
            let Some(actual) = actual else {
                return Ok(false);
            };
            let Some(ordering) = compare_ordered(actual, &filter.value) else {
                return Err(StuffError::IncomparableValues {
                    field: filter.field.clone(),
                    op: filter.op.name(),
                });
            };

            Ok(match filter.op {
                Operator::Gt => ordering == Ordering::Greater,
                Operator::Gte => ordering != Ordering::Less,
                Operator::Lt => ordering == Ordering::Less,
                Operator::Lte => ordering != Ordering::Greater,
                _ => unreachable!(),
            })
        }
    }
}

fn matches_contains(actual: Option<&Value>, needle: &Value) -> bool {
    match (actual, needle) {
        (Some(Value::Array(values)), needle) => values.iter().any(|value| value == needle),
        (Some(Value::String(haystack)), Value::String(needle)) => haystack.contains(needle),
        (Some(Value::Object(object)), Value::String(key)) => object.contains_key(key),
        _ => false,
    }
}

fn compare_ordered(left: &Value, right: &Value) -> Option<Ordering> {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => left
            .as_f64()
            .zip(right.as_f64())
            .and_then(|(left, right)| left.partial_cmp(&right)),
        (Value::String(left), Value::String(right)) => Some(left.cmp(right)),
        (Value::Bool(left), Value::Bool(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

fn compare_json_values(left: Option<&Value>, right: Option<&Value>) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(left), Some(right)) => compare_ordered(left, right).unwrap_or_else(|| {
            json_type_rank(left)
                .cmp(&json_type_rank(right))
                .then_with(|| index_key(left).cmp(&index_key(right)))
        }),
    }
}

fn json_type_rank(value: &Value) -> u8 {
    match value {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    }
}

fn apply_merge_patch(target: &mut Value, patch: &Value) {
    let Value::Object(patch_object) = patch else {
        *target = patch.clone();
        return;
    };

    if !target.is_object() {
        *target = Value::Object(Map::new());
    }

    let Value::Object(target_object) = target else {
        unreachable!();
    };

    for (key, value) in patch_object {
        if value.is_null() {
            target_object.remove(key);
        } else {
            apply_merge_patch(
                target_object.entry(key.clone()).or_insert(Value::Null),
                value,
            );
        }
    }
}

impl Operator {
    fn name(self) -> &'static str {
        match self {
            Operator::Eq => "eq",
            Operator::Ne => "ne",
            Operator::Gt => "gt",
            Operator::Gte => "gte",
            Operator::Lt => "lt",
            Operator::Lte => "lte",
            Operator::Contains => "contains",
            Operator::Exists => "exists",
        }
    }
}

pub fn sample_document() -> Value {
    json!({
        "kind": "example",
        "name": "Pocket Sandwich",
        "tags": ["demo", "snack"],
        "calories": 420
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open_temp_db() -> (tempfile::TempDir, StuffDb) {
        let dir = tempdir().unwrap();
        let db = StuffDb::open(dir.path().join("stuff.json")).unwrap();
        (dir, db)
    }

    #[test]
    fn creates_collections_and_persists_documents() {
        let (dir, mut db) = open_temp_db();

        db.create_collection("people").unwrap();
        db.insert_with_id("people", "ada", json!({"name": "Ada", "language": "math"}))
            .unwrap();

        let reopened = StuffDb::open(dir.path().join("stuff.json")).unwrap();
        let ada = reopened.get("people", "ada").unwrap().unwrap();

        assert_eq!(ada["name"], "Ada");
        assert_eq!(ada["_id"], "ada");
    }

    #[test]
    fn generated_insert_ids_are_added_to_documents() {
        let (_dir, mut db) = open_temp_db();

        db.create_collection("notes").unwrap();
        let id = db
            .insert("notes", json!({"body": "remember toast"}))
            .unwrap();
        let doc = db.get("notes", &id).unwrap().unwrap();

        assert_eq!(doc["_id"], id);
        assert_eq!(doc["body"], "remember toast");
    }

    #[test]
    fn merge_patch_updates_nested_values_and_deletes_nulls() {
        let (_dir, mut db) = open_temp_db();

        db.create_collection("things").unwrap();
        db.insert_with_id(
            "things",
            "one",
            json!({"name": "one", "stats": {"count": 1, "stale": true}}),
        )
        .unwrap();

        let updated = db
            .merge_patch(
                "things",
                "one",
                json!({"stats": {"count": 2, "stale": null}}),
            )
            .unwrap();

        assert_eq!(updated["stats"]["count"], 2);
        assert!(updated["stats"].get("stale").is_none());
        assert_eq!(updated["_id"], "one");
    }

    #[test]
    fn queries_filter_sort_limit_and_offset() {
        let (_dir, mut db) = open_temp_db();

        db.create_collection("tasks").unwrap();
        db.insert_with_id("tasks", "a", json!({"name": "a", "done": false, "rank": 2}))
            .unwrap();
        db.insert_with_id("tasks", "b", json!({"name": "b", "done": true, "rank": 1}))
            .unwrap();
        db.insert_with_id("tasks", "c", json!({"name": "c", "done": false, "rank": 3}))
            .unwrap();

        let results = db
            .find(
                "tasks",
                Query::new()
                    .eq("done", false)
                    .sort_desc("rank")
                    .offset(0)
                    .limit(1),
            )
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["name"], "c");
    }

    #[test]
    fn indexes_survive_reopen_and_queries_still_work() {
        let (dir, mut db) = open_temp_db();

        db.create_collection("users").unwrap();
        db.create_index("users", "email").unwrap();
        db.insert_with_id("users", "1", json!({"email": "a@example.com"}))
            .unwrap();

        let reopened = StuffDb::open(dir.path().join("stuff.json")).unwrap();
        assert_eq!(reopened.list_indexes("users").unwrap(), vec!["email"]);

        let results = reopened
            .find("users", Query::new().eq("email", "a@example.com"))
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["_id"], "1");
    }

    #[test]
    fn transaction_rolls_back_on_error() {
        let (_dir, mut db) = open_temp_db();

        db.create_collection("numbers").unwrap();
        let result = db.transaction(|tx| {
            tx.insert_with_id("numbers", "one", json!({"value": 1}))?;
            tx.insert_with_id("numbers", "one", json!({"value": 2}))?;
            Ok(())
        });

        assert!(result.is_err());
        assert!(db.get("numbers", "one").unwrap().is_none());
    }

    #[test]
    fn stats_count_documents_and_indexes() {
        let (_dir, mut db) = open_temp_db();

        db.create_collection("items").unwrap();
        db.create_index("items", "kind").unwrap();
        db.insert_with_id("items", "1", json!({"kind": "book"}))
            .unwrap();

        let stats = db.stats();
        assert_eq!(stats.collections, 1);
        assert_eq!(stats.documents, 1);
        assert_eq!(stats.indexes, 1);
    }
}
