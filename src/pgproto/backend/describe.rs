use crate::pgproto::error::PgResult;
use crate::pgproto::value::{self, Format};
use pgwire::messages::data::{FieldDescription, RowDescription};
use postgres_types::{Oid, Type};
use sbroad::errors::{Entity, SbroadError};
use sbroad::ir::acl::{Acl, GrantRevokeType};
use sbroad::ir::block::Block;
use sbroad::ir::ddl::Ddl;
use sbroad::ir::expression::Expression;
use sbroad::ir::operator::Relational;
use sbroad::ir::{Node, Plan};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::iter::zip;
use std::os::raw::c_int;
use tarantool::proc::{Return, ReturnMsgpack};
use tarantool::tuple::FunctionCtx;

#[derive(Debug, Clone, Default, Deserialize_repr, Serialize_repr)]
#[repr(u8)]
pub enum QueryType {
    Acl = 0,
    Ddl = 1,
    Dml = 2,
    #[default]
    Dql = 3,
    Explain = 4,
}

#[derive(Clone, Debug, Default, Deserialize_repr, Serialize_repr)]
#[repr(u8)]
pub enum CommandTag {
    AlterRole = 0,
    CallProcedure = 16,
    CreateProcedure = 14,
    CreateRole = 1,
    CreateTable = 2,
    CreateIndex = 18,
    DropProcedure = 15,
    DropRole = 3,
    DropTable = 4,
    DropIndex = 19,
    Delete = 5,
    Explain = 6,
    Grant = 7,
    GrantRole = 8,
    Insert = 9,
    RenameRoutine = 17,
    Revoke = 10,
    RevokeRole = 11,
    #[default]
    Select = 12,
    Update = 13,
}

impl CommandTag {
    pub fn as_str(&self) -> &str {
        match *self {
            Self::AlterRole => "ALTER ROLE",
            Self::CreateRole => "CREATE ROLE",
            Self::CreateTable => "CREATE TABLE",
            Self::CreateIndex => "CREATE INDEX",
            Self::DropRole => "DROP ROLE",
            Self::DropTable => "DROP TABLE",
            Self::DropIndex => "DROP INDEX",
            Self::Delete => "DELETE",
            Self::Explain => "EXPLAIN",
            Self::Grant => "GRANT",
            Self::GrantRole => "GRANT ROLE",
            // ** from postgres sources **
            // In PostgreSQL versions 11 and earlier, it was possible to create a
            // table WITH OIDS.  When inserting into such a table, INSERT used to
            // include the Oid of the inserted record in the completion tag.  To
            // maintain compatibility in the wire protocol, we now write a "0" (for
            // InvalidOid) in the location where we once wrote the new record's Oid.
            Self::Insert => "INSERT 0",
            Self::Revoke => "REVOKE",
            Self::RevokeRole => "REVOKE ROLE",
            Self::Select => "SELECT",
            Self::Update => "UPDATE",
            Self::CreateProcedure => "CREATE PROCEDURE",
            Self::DropProcedure => "DROP PROCEDURE",
            Self::CallProcedure => "CALL",
            Self::RenameRoutine => "RENAME ROUTINE",
        }
    }
}

impl From<CommandTag> for QueryType {
    fn from(command_tag: CommandTag) -> Self {
        match command_tag {
            CommandTag::AlterRole
            | CommandTag::DropRole
            | CommandTag::CreateRole
            | CommandTag::Grant
            | CommandTag::GrantRole
            | CommandTag::Revoke
            | CommandTag::RevokeRole => QueryType::Acl,
            CommandTag::DropTable
            | CommandTag::CreateTable
            | CommandTag::CreateProcedure
            | CommandTag::CreateIndex
            | CommandTag::RenameRoutine
            | CommandTag::DropIndex
            | CommandTag::DropProcedure => QueryType::Ddl,
            CommandTag::Delete
            | CommandTag::Insert
            | CommandTag::Update
            | CommandTag::CallProcedure => QueryType::Dml,
            CommandTag::Explain => QueryType::Explain,
            CommandTag::Select => QueryType::Dql,
        }
    }
}

impl TryFrom<&Node> for CommandTag {
    type Error = SbroadError;

    fn try_from(node: &Node) -> Result<Self, Self::Error> {
        match node {
            Node::Acl(acl) => match acl {
                Acl::DropRole { .. } | Acl::DropUser { .. } => Ok(CommandTag::DropRole),
                Acl::CreateRole { .. } | Acl::CreateUser { .. } => Ok(CommandTag::CreateRole),
                Acl::AlterUser { .. } => Ok(CommandTag::AlterRole),
                Acl::GrantPrivilege { grant_type, .. } => match grant_type {
                    GrantRevokeType::RolePass { .. } => Ok(CommandTag::GrantRole),
                    _ => Ok(CommandTag::Grant),
                },
                Acl::RevokePrivilege { revoke_type, .. } => match revoke_type {
                    GrantRevokeType::RolePass { .. } => Ok(CommandTag::RevokeRole),
                    _ => Ok(CommandTag::Revoke),
                },
            },
            Node::Block(block) => match block {
                Block::Procedure { .. } => Ok(CommandTag::CallProcedure),
            },
            Node::Ddl(ddl) => match ddl {
                Ddl::DropTable { .. } => Ok(CommandTag::DropTable),
                Ddl::CreateTable { .. } => Ok(CommandTag::CreateTable),
                Ddl::CreateProc { .. } => Ok(CommandTag::CreateProcedure),
                Ddl::CreateIndex { .. } => Ok(CommandTag::CreateIndex),
                Ddl::DropProc { .. } => Ok(CommandTag::DropProcedure),
                Ddl::DropIndex { .. } => Ok(CommandTag::DropIndex),
                Ddl::RenameRoutine { .. } => Ok(CommandTag::RenameRoutine),
            },
            Node::Relational(rel) => match rel {
                Relational::Delete { .. } => Ok(CommandTag::Delete),
                Relational::Insert { .. } => Ok(CommandTag::Insert),
                Relational::Update { .. } => Ok(CommandTag::Update),
                Relational::Except { .. }
                | Relational::Join { .. }
                | Relational::Motion { .. }
                | Relational::Projection { .. }
                | Relational::Intersect { .. }
                | Relational::ScanCte { .. }
                | Relational::ScanRelation { .. }
                | Relational::ScanSubQuery { .. }
                | Relational::Selection { .. }
                | Relational::GroupBy { .. }
                | Relational::OrderBy { .. }
                | Relational::Having { .. }
                | Relational::Union { .. }
                | Relational::UnionAll { .. }
                | Relational::Values { .. }
                | Relational::ValuesRow { .. } => Ok(CommandTag::Select),
            },
            Node::Expression(_) | Node::Parameter => Err(SbroadError::Invalid(
                Entity::Node,
                Some(smol_str::format_smolstr!(
                    "{node:?} can't be converted to CommandTag"
                )),
            )),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct MetadataColumn {
    name: String,
    #[serde(rename = "type")]
    ty: String,
}

impl MetadataColumn {
    fn new(name: String, ty: String) -> Self {
        Self { name, ty }
    }
}

/// Get an output format from the dql query plan.
fn dql_output_format(ir: &Plan) -> Result<Vec<MetadataColumn>, SbroadError> {
    // Get metadata (column types) from the top node's output tuple.
    let top_id = ir.get_top()?;
    let top_output_id = ir.get_relation_node(top_id)?.output();
    let columns = ir.get_row_list(top_output_id)?;
    let mut metadata = Vec::with_capacity(columns.len());
    for col_id in columns {
        let column = ir.get_expression_node(*col_id)?;
        let column_type = column.calculate_type(ir)?.to_string();
        let column_name = if let Expression::Alias { name, .. } = column {
            name.clone()
        } else {
            return Err(SbroadError::Invalid(
                Entity::Expression,
                Some(smol_str::format_smolstr!("expected alias, got {column:?}")),
            ));
        };
        metadata.push(MetadataColumn::new(column_name.into(), column_type));
    }
    Ok(metadata)
}

/// Get the output format of explain message.
fn explain_output_format() -> Vec<MetadataColumn> {
    vec![MetadataColumn::new("QUERY PLAN".into(), "string".into())]
}

fn field_description(name: String, ty: Type, format: Format) -> FieldDescription {
    // ** From postgres sources **
    // resorigtbl/resorigcol identify the source of the column, if it is a
    // simple reference to a column of a base table (or view).  If it is not
    // a simple reference, these fields are zeroes.
    let resorigtbl = 0;
    let resorigcol = 0;

    // typmod records type-specific data supplied at table creation time
    // (for example, the max length of a varchar field).  The
    // value will generally be -1 for types that do not need typmod.
    let typemod = -1;

    let id = ty.oid();
    // TODO: add Type::len()
    let len = 0;

    FieldDescription::new(
        name,
        resorigtbl,
        resorigcol,
        id,
        len,
        typemod,
        format as i16,
    )
}

/// Contains a query description used by pgproto.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Describe {
    pub command_tag: CommandTag,
    pub query_type: QueryType,
    /// Output columns format.
    pub metadata: Vec<MetadataColumn>,
}

impl Describe {
    #[inline]
    pub fn with_command_tag(mut self, command_tag: CommandTag) -> Self {
        self.command_tag = command_tag.clone();
        self.query_type = command_tag.into();
        self
    }

    #[inline]
    pub fn with_metadata(mut self, metadata: Vec<MetadataColumn>) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn new(plan: &Plan) -> PgResult<Self> {
        let command_tag = if plan.is_explain() {
            CommandTag::Explain
        } else {
            let top = plan.get_top()?;
            let node = plan.get_node(top)?;
            CommandTag::try_from(node)?
        };
        let query_type = command_tag.clone().into();
        match query_type {
            QueryType::Acl | QueryType::Ddl | QueryType::Dml => {
                Ok(Describe::default().with_command_tag(command_tag))
            }
            QueryType::Dql => Ok(Describe::default()
                .with_command_tag(command_tag)
                .with_metadata(dql_output_format(plan)?)),
            QueryType::Explain => Ok(Describe::default()
                .with_command_tag(command_tag)
                .with_metadata(explain_output_format())),
        }
    }
}

impl Describe {
    pub fn query_type(&self) -> &QueryType {
        &self.query_type
    }

    pub fn command_tag(&self) -> &CommandTag {
        &self.command_tag
    }

    pub fn row_description(&self) -> PgResult<Option<RowDescription>> {
        match self.query_type() {
            QueryType::Acl | QueryType::Ddl | QueryType::Dml => Ok(None),
            QueryType::Dql | QueryType::Explain => {
                let row_description = self
                    .metadata
                    .iter()
                    .map(|col| {
                        let type_str = col.ty.as_str();
                        value::type_from_name(type_str)
                            .map(|ty| field_description(col.name.clone(), ty, Format::Text))
                    })
                    .collect::<PgResult<_>>()?;
                Ok(Some(RowDescription::new(row_description)))
            }
        }
    }
}

impl Return for Describe {
    fn ret(self, ctx: FunctionCtx) -> c_int {
        ReturnMsgpack(self).ret(ctx)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct StatementDescribe {
    #[serde(flatten)]
    pub describe: Describe,
    pub param_oids: Vec<Oid>,
}

impl StatementDescribe {
    pub fn new(describe: Describe, param_oids: Vec<Oid>) -> Self {
        Self {
            describe,
            param_oids,
        }
    }
}

impl StatementDescribe {
    pub fn ncolumns(&self) -> usize {
        self.describe.metadata.len()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortalDescribe {
    #[serde(flatten)]
    pub describe: Describe,
    pub output_format: Vec<Format>,
}

impl PortalDescribe {
    pub fn new(describe: Describe, output_format: Vec<Format>) -> Self {
        Self {
            describe,
            output_format,
        }
    }
}

impl PortalDescribe {
    pub fn row_description(&self) -> PgResult<Option<RowDescription>> {
        match self.query_type() {
            QueryType::Acl | QueryType::Ddl | QueryType::Dml => Ok(None),
            QueryType::Dql | QueryType::Explain => {
                let metadata = &self.describe.metadata;
                let output_format = &self.output_format;
                let row_description = zip(metadata, output_format)
                    .map(|(col, format)| {
                        let type_str = col.ty.as_str();
                        value::type_from_name(type_str)
                            .map(|ty| field_description(col.name.clone(), ty, *format))
                    })
                    .collect::<PgResult<_>>()?;
                Ok(Some(RowDescription::new(row_description)))
            }
        }
    }

    pub fn query_type(&self) -> &QueryType {
        self.describe.query_type()
    }

    pub fn command_tag(&self) -> &CommandTag {
        self.describe.command_tag()
    }

    pub fn output_format(&self) -> &[Format] {
        &self.output_format
    }
}