//! Router runtime module for the clusterwide SQL.
//! Implements infrastructure to build a distributed
//! query plan and dispatch it to the storage nodes.

use sbroad::errors::{Action, Entity, SbroadError};
use sbroad::executor::bucket::Buckets;
use sbroad::executor::engine::helpers::sharding_keys_from_tuple;
use sbroad::executor::engine::helpers::vshard::{
    exec_ir_on_all_buckets, exec_ir_on_some_buckets, get_random_bucket,
};
use sbroad::executor::engine::helpers::{
    dispatch, explain_format, materialize_motion, sharding_keys_from_map,
};
use sbroad::executor::engine::{QueryCache, Router, Vshard};
use sbroad::executor::ir::{ConnectionType, ExecutionPlan, QueryType};
use sbroad::executor::lru::{Cache, LRUCache, DEFAULT_CAPACITY};
use sbroad::executor::protocol::Binary;
use sbroad::frontend::sql::ast::AbstractSyntaxTree;
use sbroad::ir::value::{MsgPackValue, Value};
use sbroad::ir::Plan;

use std::any::Any;
use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::sql::DEFAULT_BUCKET_COUNT;

use crate::schema::{Distribution, ShardingFn, SpaceDef};
use crate::storage::{space_by_name, ClusterwideSpace};

use sbroad::executor::engine::helpers::storage::meta::{
    DEFAULT_JAEGER_AGENT_HOST, DEFAULT_JAEGER_AGENT_PORT,
};
use sbroad::executor::engine::helpers::{
    normalize_name_for_space_api, normalize_name_from_schema, normalize_name_from_sql,
};
use sbroad::executor::engine::Metadata;
use sbroad::ir::function::Function;
use sbroad::ir::relation::{Column, ColumnRole, Table, Type};

use std::borrow::Cow;

use tarantool::space::Space;
use tarantool::tuple::{KeyDef, Tuple};
use tarantool::util::Value as TarantoolValue;

thread_local! (
    static PLAN_CACHE: Rc<RefCell<LRUCache<String, Plan>>> = Rc::new(
        RefCell::new(LRUCache::new(DEFAULT_CAPACITY, None).unwrap())
    )
);

pub const DEFAULT_BUCKET_COLUMN: &str = "bucket_id";

#[allow(clippy::module_name_repetitions)]
pub struct RouterRuntime {
    metadata: RefCell<RouterMetadata>,
    bucket_count: u64,
    ir_cache: Rc<RefCell<LRUCache<String, Plan>>>,
}

impl RouterRuntime {
    /// Build a new router runtime.
    ///
    /// # Errors
    /// - If the cache cannot be initialized.
    pub fn new() -> Result<Self, SbroadError> {
        let metadata = RouterMetadata::default();
        let bucket_count = DEFAULT_BUCKET_COUNT;
        let runtime = PLAN_CACHE.with(|cache| RouterRuntime {
            metadata: RefCell::new(metadata),
            bucket_count,
            ir_cache: cache.clone(),
        });
        Ok(runtime)
    }
}

impl QueryCache for RouterRuntime {
    type Cache = LRUCache<String, Plan>;

    fn cache(&self) -> &RefCell<Self::Cache> {
        &self.ir_cache
    }

    fn cache_capacity(&self) -> Result<usize, SbroadError> {
        Ok(self
            .ir_cache
            .try_borrow()
            .map_err(|e| SbroadError::FailedTo(Action::Get, Some(Entity::Cache), format!("{e:?}")))?
            .capacity())
    }

    fn clear_cache(&self) -> Result<(), SbroadError> {
        *self.ir_cache.try_borrow_mut().map_err(|e| {
            SbroadError::FailedTo(Action::Clear, Some(Entity::Cache), format!("{e:?}"))
        })? = Self::Cache::new(self.cache_capacity()?, None)?;
        Ok(())
    }
}

impl Router for RouterRuntime {
    type ParseTree = AbstractSyntaxTree;
    type MetadataProvider = RouterMetadata;

    fn metadata(&self) -> Result<Ref<Self::MetadataProvider>, SbroadError> {
        self.metadata.try_borrow().map_err(|e| {
            SbroadError::FailedTo(Action::Get, Some(Entity::Metadata), format!("{e:?}"))
        })
    }

    fn materialize_motion(
        &self,
        plan: &mut sbroad::executor::ir::ExecutionPlan,
        motion_node_id: usize,
        buckets: &sbroad::executor::bucket::Buckets,
    ) -> Result<sbroad::executor::vtable::VirtualTable, SbroadError> {
        materialize_motion(self, plan, motion_node_id, buckets)
    }

    fn dispatch(
        &self,
        plan: &mut sbroad::executor::ir::ExecutionPlan,
        top_id: usize,
        buckets: &sbroad::executor::bucket::Buckets,
    ) -> Result<Box<dyn std::any::Any>, SbroadError> {
        dispatch(self, plan, top_id, buckets)
    }

    fn explain_format(&self, explain: String) -> Result<Box<dyn std::any::Any>, SbroadError> {
        explain_format(&explain)
    }

    fn extract_sharding_keys_from_map<'rec>(
        &self,
        space: String,
        args: &'rec HashMap<String, Value>,
    ) -> Result<Vec<&'rec Value>, SbroadError> {
        let metadata = self.metadata.try_borrow().map_err(|e| {
            SbroadError::FailedTo(Action::Borrow, Some(Entity::Metadata), format!("{e:?}"))
        })?;
        sharding_keys_from_map(&*metadata, &space, args)
    }

    fn extract_sharding_keys_from_tuple<'rec>(
        &self,
        space: String,
        args: &'rec [Value],
    ) -> Result<Vec<&'rec Value>, SbroadError> {
        sharding_keys_from_tuple(&*self.metadata()?, &space, args)
    }
}

pub(crate) fn calculate_bucket_id(tuple: &[&Value], bucket_count: u64) -> Result<u64, SbroadError> {
    let wrapped_tuple = tuple
        .iter()
        .map(|v| MsgPackValue::from(*v))
        .collect::<Vec<_>>();
    let tnt_tuple = Tuple::new(&wrapped_tuple).map_err(|e| {
        SbroadError::FailedTo(Action::Create, Some(Entity::Tuple), format!("{e:?}"))
    })?;
    let mut key_parts = Vec::with_capacity(tuple.len());
    for (pos, value) in tuple.iter().enumerate() {
        let pos = u32::try_from(pos).map_err(|_| {
            SbroadError::FailedTo(
                Action::Create,
                Some(Entity::KeyDef),
                "Tuple is too long".to_string(),
            )
        })?;
        key_parts.push(value.as_key_def_part(pos));
    }
    let key = KeyDef::new(key_parts.as_slice()).map_err(|e| {
        SbroadError::FailedTo(Action::Create, Some(Entity::KeyDef), format!("{e:?}"))
    })?;
    Ok(u64::from(key.hash(&tnt_tuple)) % bucket_count)
}

impl Vshard for RouterRuntime {
    fn exec_ir_on_all(
        &self,
        required: Binary,
        optional: Binary,
        query_type: QueryType,
        conn_type: ConnectionType,
    ) -> Result<Box<dyn Any>, SbroadError> {
        exec_ir_on_all_buckets(
            &*self.metadata()?,
            required,
            optional,
            query_type,
            conn_type,
        )
    }

    fn bucket_count(&self) -> u64 {
        self.bucket_count
    }

    fn get_random_bucket(&self) -> Buckets {
        get_random_bucket(self)
    }

    fn determine_bucket_id(&self, s: &[&Value]) -> Result<u64, SbroadError> {
        calculate_bucket_id(s, self.bucket_count())
    }

    fn exec_ir_on_some(
        &self,
        sub_plan: ExecutionPlan,
        buckets: &Buckets,
    ) -> Result<Box<dyn Any>, SbroadError> {
        exec_ir_on_some_buckets(self, sub_plan, buckets)
    }
}

impl Vshard for &RouterRuntime {
    fn exec_ir_on_all(
        &self,
        required: Binary,
        optional: Binary,
        query_type: QueryType,
        conn_type: ConnectionType,
    ) -> Result<Box<dyn Any>, SbroadError> {
        exec_ir_on_all_buckets(
            &*self.metadata()?,
            required,
            optional,
            query_type,
            conn_type,
        )
    }

    fn bucket_count(&self) -> u64 {
        self.bucket_count
    }

    fn get_random_bucket(&self) -> Buckets {
        get_random_bucket(self)
    }

    fn determine_bucket_id(&self, s: &[&Value]) -> Result<u64, SbroadError> {
        calculate_bucket_id(s, self.bucket_count())
    }

    fn exec_ir_on_some(
        &self,
        sub_plan: ExecutionPlan,
        buckets: &Buckets,
    ) -> Result<Box<dyn Any>, SbroadError> {
        exec_ir_on_some_buckets(*self, sub_plan, buckets)
    }
}

/// Router runtime configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::module_name_repetitions)]
pub struct RouterMetadata {
    /// Execute response waiting timeout in seconds.
    pub waiting_timeout: u64,

    /// Query cache capacity.
    pub cache_capacity: usize,

    /// Bucket column name.
    pub sharding_column: String,

    /// Jaeger agent host.
    pub jaeger_agent_host: &'static str,

    /// Jaeger agent port.
    pub jaeger_agent_port: u16,

    /// IR functions
    pub functions: HashMap<String, Function>,
}

impl Default for RouterMetadata {
    fn default() -> Self {
        Self::new()
    }
}

impl RouterMetadata {
    #[must_use]
    pub fn new() -> Self {
        RouterMetadata {
            waiting_timeout: 360,
            cache_capacity: DEFAULT_CAPACITY,
            jaeger_agent_host: DEFAULT_JAEGER_AGENT_HOST,
            jaeger_agent_port: DEFAULT_JAEGER_AGENT_PORT,
            sharding_column: DEFAULT_BUCKET_COLUMN.to_string(),
            functions: HashMap::new(),
        }
    }
}

impl Metadata for RouterMetadata {
    #[allow(dead_code)]
    #[allow(clippy::too_many_lines)]
    fn table(&self, table_name: &str) -> Result<Table, SbroadError> {
        let name = normalize_name_for_space_api(table_name);

        // // Get the space columns and engine of the space.
        let space = Space::find(&name)
            .ok_or_else(|| SbroadError::NotFound(Entity::Space, name.to_string()))?;
        let meta = space.meta().map_err(|e| {
            SbroadError::FailedTo(Action::Get, Some(Entity::SpaceMetadata), e.to_string())
        })?;
        let engine = meta.engine;
        let mut columns: Vec<Column> = Vec::with_capacity(meta.format.len());
        for column_meta in &meta.format {
            let name_value = column_meta.get(&Cow::from("name")).ok_or_else(|| {
                SbroadError::FailedTo(
                    Action::Get,
                    Some(Entity::SpaceMetadata),
                    format!("column name not found in the space format: {column_meta:?}"),
                )
            })?;
            let col_name = if let TarantoolValue::Str(name) = name_value {
                name
            } else {
                return Err(SbroadError::FailedTo(
                    Action::Get,
                    Some(Entity::SpaceMetadata),
                    format!("column name is not a string: {name_value:?}"),
                ));
            };
            let type_value = column_meta.get(&Cow::from("type")).ok_or_else(|| {
                SbroadError::FailedTo(
                    Action::Get,
                    Some(Entity::SpaceMetadata),
                    format!("column type not found in the space format: {column_meta:?}"),
                )
            })?;
            let col_type: Type = if let TarantoolValue::Str(col_type) = type_value {
                Type::new(col_type)?
            } else {
                return Err(SbroadError::FailedTo(
                    Action::Get,
                    Some(Entity::SpaceMetadata),
                    format!("column type is not a string: {type_value:?}"),
                ));
            };
            let role = if col_name == DEFAULT_BUCKET_COLUMN {
                ColumnRole::Sharding
            } else {
                ColumnRole::User
            };
            let column = Column {
                name: normalize_name_from_schema(col_name),
                r#type: col_type,
                role,
            };
            columns.push(column);
        }

        // Try to find the sharding columns of the space in "_pico_space".
        // If nothing found then the space is local and we can't query it with
        // distributed SQL.
        let pico_space = space_by_name(&ClusterwideSpace::Space)
            .map_err(|e| SbroadError::NotFound(Entity::Space, format!("{e:?}")))?;
        let tuple = pico_space.get(&[meta.id]).map_err(|e| {
            SbroadError::FailedTo(
                Action::Get,
                Some(Entity::ShardingKey),
                format!("space id {}: {e}", meta.id),
            )
        })?;
        let tuple =
            tuple.ok_or_else(|| SbroadError::NotFound(Entity::ShardingKey, name.to_string()))?;
        let space_def: SpaceDef = tuple.decode().map_err(|e| {
            SbroadError::FailedTo(
                Action::Deserialize,
                Some(Entity::SpaceMetadata),
                format!("serde error: {e}"),
            )
        })?;
        let keys: Vec<_> = match &space_def.distribution {
            Distribution::Global => {
                return Err(SbroadError::Invalid(
                    Entity::Distribution,
                    Some("global distribution is not supported".into()),
                ));
            }
            Distribution::ShardedImplicitly {
                sharding_key,
                sharding_fn,
            } => {
                if !matches!(sharding_fn, ShardingFn::Murmur3) {
                    return Err(SbroadError::NotImplemented(
                        Entity::Distribution,
                        format!("by hash function {sharding_fn}"),
                    ));
                }
                sharding_key
                    .iter()
                    .map(|field| normalize_name_from_schema(field))
                    .collect()
            }
            Distribution::ShardedByField { field } => {
                return Err(SbroadError::NotImplemented(
                    Entity::Distribution,
                    format!("explicitly by field '{field}'"),
                ));
            }
        };
        let sharding_keys: &[&str] = &keys.iter().map(String::as_str).collect::<Vec<_>>();
        Table::new_seg(
            &normalize_name_from_sql(table_name),
            columns,
            sharding_keys,
            engine.into(),
        )
    }

    fn function(&self, fn_name: &str) -> Result<&Function, SbroadError> {
        let name = normalize_name_from_sql(fn_name);
        match self.functions.get(&name) {
            Some(v) => Ok(v),
            None => Err(SbroadError::NotFound(Entity::SQLFunction, name)),
        }
    }

    /// Get response waiting timeout for executor
    fn waiting_timeout(&self) -> u64 {
        self.waiting_timeout
    }

    fn sharding_column(&self) -> &str {
        self.sharding_column.as_str()
    }

    /// Get sharding key's column names by a space name
    fn sharding_key_by_space(&self, space: &str) -> Result<Vec<String>, SbroadError> {
        let table = self.table(space)?;
        table.get_sharding_column_names()
    }

    fn sharding_positions_by_space(&self, space: &str) -> Result<Vec<usize>, SbroadError> {
        let table = self.table(space)?;
        Ok(table.get_sharding_positions().to_vec())
    }
}