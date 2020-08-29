use crate::lang::execution_context::CommandContext;
use crate::{
    lang::errors::CrushError,
    lang::{data::table::Row, value::Value, value::ValueType},
};
use std::io::{BufReader, Write};

use crate::lang::argument::ArgumentHandler;
use crate::lang::command::OutputType::Unknown;
use crate::lang::errors::{error, mandate, to_crush_error, CrushResult};
use crate::lang::files::Files;
use crate::lang::data::scope::ScopeLoader;
use crate::lang::data::table::ColumnType;
use crate::lang::{data::list::List, data::r#struct::Struct, data::table::Table};
use signature::signature;
use std::collections::HashSet;
use std::convert::TryFrom;
use crate::lang::data::dict::Dict;

fn from_yaml(yaml_value: &serde_yaml::Value) -> CrushResult<Value> {
    match yaml_value {
        serde_yaml::Value::Null => Ok(Value::Empty()),
        serde_yaml::Value::Bool(b) => Ok(Value::Bool(*b)),
        serde_yaml::Value::Number(f) => {
            if f.is_u64() {
                Ok(Value::Integer(f.as_u64().expect("") as i128))
            } else if f.is_i64() {
                Ok(Value::Integer(f.as_i64().expect("") as i128))
            } else {
                Ok(Value::Float(f.as_f64().ok_or(CrushError::InvalidData(
                    "Not a valid number".to_string(),
                ))?))
            }
        }
        serde_yaml::Value::String(s) => Ok(Value::string(s.as_str())),
        serde_yaml::Value::Sequence(arr) => {
            let mut lst = arr
                .iter()
                .map(|v| from_yaml(v))
                .collect::<CrushResult<Vec<Value>>>()?;
            let types: HashSet<ValueType> = lst.iter().map(|v| v.value_type()).collect();
            let struct_types: HashSet<Vec<ColumnType>> = lst
                .iter()
                .flat_map(|v| match v {
                    Value::Struct(r) => vec![r.local_signature()],
                    _ => vec![],
                })
                .collect();

            match types.len() {
                0 => Ok(Value::Empty()),
                1 => {
                    let list_type = types.iter().next().unwrap();
                    match (list_type, struct_types.len()) {
                        (ValueType::Struct, 1) => {
                            let row_list = lst
                                .drain(..)
                                .map(|v| match v {
                                    Value::Struct(r) => Ok(r.to_row()),
                                    _ => error("Impossible!"),
                                })
                                .collect::<CrushResult<Vec<Row>>>()?;
                            Ok(Value::Table(Table::new(
                                struct_types.iter().next().unwrap().clone(),
                                row_list,
                            )))
                        }
                        _ => Ok(Value::List(List::new(list_type.clone(), lst))),
                    }
                }
                _ => Ok(Value::List(List::new(ValueType::Any, lst))),
            }
        }
        serde_yaml::Value::Mapping(o) => {
            let d = Dict::new(ValueType::Any, ValueType::Any);
            for (k, v) in o.into_iter() {
                d.insert(from_yaml(k)?, from_yaml(v)?)?;
            }
            Ok(Value::Dict(d))
        }

    }
}

fn to_yaml(value: Value) -> CrushResult<serde_yaml::Value> {
    match value.materialize() {
        Value::File(s) => Ok(serde_yaml::Value::from(mandate(
            s.to_str(),
            "Invalid filename",
        )?)),

        Value::String(s) => Ok(serde_yaml::Value::from(s)),

        Value::Integer(i) => Ok(serde_yaml::Value::from(to_crush_error(i64::try_from(i))?)),

        Value::List(l) => Ok(serde_yaml::Value::Sequence(
            l.dump()
                .drain(..)
                .map(to_yaml)
                .collect::<CrushResult<Vec<_>>>()?,
        )),

        Value::Table(t) => {
            let types = t.types().to_vec();
            let structs = t
                .rows()
                .iter()
                .map(|r| r.clone().into_struct(&types))
                .map(|s| to_yaml(Value::Struct(s)))
                .collect::<CrushResult<Vec<_>>>()?;
            Ok(serde_yaml::Value::Sequence(structs))
        }

        Value::Bool(b) => Ok(serde_yaml::Value::from(b)),

        Value::Float(f) => Ok(serde_yaml::Value::from(f)),

        Value::Struct(s) => {
            let mut map = serde_yaml::Mapping::new();
            for (k, v) in s.local_elements() {
                map.insert(to_yaml(Value::String(k))?, to_yaml(v)?);
            }
            Ok(serde_yaml::Value::Mapping(map))
        }

        Value::Duration(d) => Ok(serde_yaml::Value::from(d.num_seconds())),

        Value::Time(t) => Ok(serde_yaml::Value::from(t.to_rfc3339())),

        Value::Binary(b) => Ok(serde_yaml::Value::from(b)),

        Value::BinaryStream(_) => panic!("Impossible"),

        Value::TableStream(_) => panic!("Impossible"),

        v => error(&format!("Unsupported data type {}", v.value_type())),
    }
}

#[signature(
from,
can_block = true,
output = Unknown,
short = "Parse yaml format",
example = "(http \"https://jsonplaceholder.typicode.com/todos/3\"):body | yaml:from")]
struct From {
    #[unnamed()]
    files: Files,
}

pub fn from(context: CommandContext) -> CrushResult<()> {
    let cfg: From = From::parse(context.arguments, &context.printer)?;
    let reader = BufReader::new(cfg.files.reader(context.input)?);
    let serde_value = to_crush_error(serde_yaml::from_reader(reader))?;
    let crush_value = from_yaml(&serde_value)?;
    context.output.send(crush_value)
}

#[signature(
to,
can_block = true,
output = Unknown,
short = "Serialize to yaml format",
example = "ls | yaml:to")]
struct To {
    #[unnamed()]
    file: Files,
}

fn to(context: CommandContext) -> CrushResult<()> {
    let cfg: To = To::parse(context.arguments, &context.printer)?;
    let mut writer = cfg.file.writer(context.output)?;
    let value = context.input.recv()?;
    let yaml_value = to_yaml(value)?;
    to_crush_error(writer.write(to_crush_error(serde_yaml::to_string(&yaml_value))?.as_bytes()))?;
    Ok(())
}

pub fn declare(root: &mut ScopeLoader) -> CrushResult<()> {
    root.create_namespace(
        "yaml",
        Box::new(move |env| {
            From::declare(env)?;
            To::declare(env)?;
            Ok(())
        }),
    )?;
    Ok(())
}