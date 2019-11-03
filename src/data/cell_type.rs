use crate::errors::{error, JobError, mandate};
use crate::data::{Cell, ColumnType};
use crate::glob::Glob;
use regex::Regex;
use std::error::Error;
use crate::parser::parse_name;

#[derive(Clone)]
#[derive(PartialEq)]
#[derive(Eq)]
#[derive(PartialOrd)]
#[derive(Ord)]
#[derive(Debug)]
pub enum CellType {
    Text,
    Integer,
    Time,
    Duration,
    Field,
    Glob,
    Regex,
    Op,
    Command,
    Closure,
    File,
    Output(Vec<ColumnType>),
    Rows(Vec<ColumnType>),
    List(Box<CellType>),
    Env,
}

impl CellType {
    pub fn from(s: &str) -> Result<CellType, JobError> {
        match s {
            "text" => Ok(CellType::Text),
            "integer" => Ok(CellType::Integer),
            "time" => Ok(CellType::Time),
            "field" => Ok(CellType::Field),
            "glob" => Ok(CellType::Glob),
            "regex" => Ok(CellType::Regex),
            "op" => Ok(CellType::Op),
            "command" => Ok(CellType::Command),
            "file" => Ok(CellType::File),
            _ => Err(error(format!("Unknown cell type {}", s).as_str())),
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            CellType::Text => "text".to_string(),
            CellType::Integer => "integer".to_string(),
            CellType::Time => "time".to_string(),
            CellType::Duration => "duration".to_string(),
            CellType::Field => "field".to_string(),
            CellType::Glob => "glob".to_string(),
            CellType::Regex => "regex".to_string(),
            CellType::Op => "op".to_string(),
            CellType::Command => "command".to_string(),
            CellType::Closure => "closure".to_string(),
            CellType::File => "file".to_string(),
            CellType::Output(o) => format!("output<{}>", o.iter().map(|i| i.to_string()).collect::<Vec<String>>().join(",")),
            CellType::Rows(r) => format!("rows<{}>", r.iter().map(|i| i.to_string()).collect::<Vec<String>>().join(",")),
            CellType::List(l) => format!("list<{}>", l.to_string()),
            CellType::Env => "env".to_string(),
        }
    }

    pub fn parse(&self, s: &str) -> Result<Cell, JobError> {
        match self {
            CellType::Text => Ok(Cell::Text(Box::from(s))),
            CellType::Integer => match s.parse::<i128>() {
                Ok(n) => Ok(Cell::Integer(n)),
                Err(e) => Err(error(e.description())),
            }
            CellType::Field => Ok(Cell::Field(mandate(parse_name(s), "Invalid field name")?)),
            CellType::Glob => Ok(Cell::Glob(Glob::new(s))),
            CellType::Regex => match Regex::new(s) {
                Ok(r) => Ok(Cell::Regex(Box::from(s), r)),
                Err(e) => Err(error(e.description())),
            }
            CellType::File => Ok(Cell::Text(Box::from(s))),
            CellType::Op => match s {
                "==" | "!=" | ">" | ">=" | "<" | "<=" | "=~" | "!~" => Ok(Cell::Op(Box::from(s))),
                _ => Err(error("Invalid operator")),
            }
            _ => Err(error("Failed to parse cell")),
        }
    }
}