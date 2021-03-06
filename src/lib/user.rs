use crate::lang::command::OutputType::Known;
use crate::lang::errors::{CrushResult, argument_error_legacy, to_crush_error};
use crate::lang::execution_context::CommandContext;
use crate::lang::data::scope::Scope;
use crate::lang::data::r#struct::Struct;
use crate::lang::value::{Value, ValueType};
use signature::signature;
use std::ffi::CStr;
use std::path::PathBuf;
use crate::lang::{data::table::ColumnType, data::table::Row};
use lazy_static::lazy_static;
use crate::util::user_map::get_current_username;

#[signature(
me,
can_block = false,
short = "current user",
)]
struct Me {}

fn me(context: CommandContext) -> CrushResult<()> {
    unsafe {
        context.output.send(search(&get_current_username()?)?)
    }
}

lazy_static! {
    static ref LIST_OUTPUT_TYPE: Vec<ColumnType> = vec![
        ColumnType::new("name", ValueType::String),
        ColumnType::new("home", ValueType::File),
        ColumnType::new("shell", ValueType::File),
        ColumnType::new("information", ValueType::String),
        ColumnType::new("uid", ValueType::Integer),
        ColumnType::new("gid", ValueType::Integer),
    ];
}

#[signature(
list,
can_block = true,
output = Known(ValueType::TableInputStream(LIST_OUTPUT_TYPE.clone())),
short = "List all users on the system",
)]
struct List {}

fn list(context: CommandContext) -> CrushResult<()> {
    let output = context.output.initialize(LIST_OUTPUT_TYPE.clone())?;
    unsafe {
        nix::libc::setpwent();
        loop {
            let passwd = nix::libc::getpwent();
            if passwd.is_null() {
                break;
            }
            output.send(Row::new(
                vec![
                    Value::String(parse((*passwd).pw_name)?),
                    Value::File(PathBuf::from(parse((*passwd).pw_dir)?)),
                    Value::File(PathBuf::from(parse((*passwd).pw_shell)?)),
                    Value::String(parse((*passwd).pw_gecos)?),
                    Value::Integer((*passwd).pw_uid as i128),
                    Value::Integer((*passwd).pw_gid as i128),
                ]))?;
        }
        nix::libc::endpwent();
    }
    Ok(())
}

unsafe fn search(input_name: &str) -> CrushResult<Value> {
    nix::libc::setpwent();
    loop {
        let passwd = nix::libc::getpwent();
        if passwd.is_null() {
            return argument_error_legacy(format!("Unknown user {}", input_name));
        }
        let name = parse((*passwd).pw_name)?;
        if name == input_name {
            let res = Value::Struct(
                Struct::new(
                    vec![
                        ("name", Value::String(input_name.to_string())),
                        ("home", Value::File(PathBuf::from(parse((*passwd).pw_dir)?))),
                        ("shell", Value::File(PathBuf::from(parse((*passwd).pw_shell)?))),
                        ("information", Value::String(parse((*passwd).pw_gecos)?)),
                        ("uid", Value::Integer((*passwd).pw_uid as i128)),
                        ("gid", Value::Integer((*passwd).pw_gid as i128)),
                    ],
                    None,
                )
            );
            nix::libc::endpwent();
            return Ok(res);
        }
    }
}


#[signature(
find,
can_block = false,
short = "find a user by name",
)]
struct Find {
    #[description("the of the user to find.")]
    name: String,
}

fn find(context: CommandContext) -> CrushResult<()> {
    let cfg: Find = Find::parse(context.arguments, &context.global_state.printer())?;
    unsafe {
        context.output.send(search(&cfg.name)?)
    }
}

unsafe fn parse(s: *const i8) -> CrushResult<String> {
    Ok(to_crush_error(CStr::from_ptr(s).to_str())?.to_string())
}


pub fn declare(root: &Scope) -> CrushResult<()> {
    root.create_namespace(
        "user",
        "User commands",
        Box::new(move |user| {
            Me::declare(user)?;
            Find::declare(user)?;
            List::declare(user)?;
            Ok(())
        }),
    )?;
    Ok(())
}
