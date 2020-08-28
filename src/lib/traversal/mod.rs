use crate::lang::argument::ArgumentHandler;
use crate::lang::command::OutputType::Known;
use crate::lang::errors::{argument_error, error, to_crush_error, CrushResult};
use crate::lang::execution_context::ArgumentVector;
use crate::lang::execution_context::CommandContext;
use crate::lang::help::Help;
use crate::lang::printer::Printer;
use crate::lang::data::scope::Scope;
use crate::lang::value::Value;
use crate::lang::value::ValueType;
use crate::util::file::{cwd, home};
use std::path::PathBuf;

mod find;

pub fn cd(context: CommandContext) -> CrushResult<()> {
    let dir = match context.arguments.len() {
        0 => home(),
        1 => {
            let dir = &context.arguments[0];
            match &dir.value {
                Value::String(val) => Ok(PathBuf::from(val)),
                Value::File(val) => Ok(val.clone()),
                Value::Glob(val) => val.glob_to_single_file(&cwd()?),
                _ => error(
                    format!(
                        "Wrong parameter type, expected text or file, found {}",
                        &dir.value.value_type().to_string()
                    )
                    .as_str(),
                ),
            }
        }
        _ => error("Wrong number of arguments"),
    }?;
    context.output.send(Value::Empty())?;
    to_crush_error(std::env::set_current_dir(dir))
}

pub fn pwd(context: CommandContext) -> CrushResult<()> {
    context.output.send(Value::File(cwd()?))
}

fn halp(o: &dyn Help, printer: &Printer) {
    printer.line(
        match o.long_help() {
            None => format!("{}\n\n    {}", o.signature(), o.short_help()),
            Some(long_help) => format!(
                "{}\n\n    {}\n\n{}",
                o.signature(),
                o.short_help(),
                long_help
            ),
        }
        .as_str(),
    );
}

pub fn help(mut context: CommandContext) -> CrushResult<()> {
    match context.arguments.len() {
        0 => {
            context.printer.line(
                r#"
Welcome to Crush!

If this is your first time using Crush, congratulations on just entering your
first command! If you haven't already, you might want to check out the Readme
for an introduction at https://github.com/liljencrantz/crush/.

Call the help command with the name of any value, including a command or a
type in order to get help about it. For example, you might want to run the
commands "help help", "help string", "help if" or "help where".

To get a list of everything in your namespace, write "var:env". To list the
members of a value, write "dir <value>".
"#,
            );
            context.output.send(Value::Empty())
        }
        1 => {
            let v = context.arguments.value(0)?;
            match v {
                Value::Command(cmd) => halp(cmd.help(), &context.printer),
                Value::Type(t) => halp(&t, &context.printer),
                v => halp(&v, &context.printer),
            }
            context.output.send(Value::Empty())
        }
        _ => argument_error("The help command expects at most one argument"),
    }
}

pub fn declare(root: &Scope) -> CrushResult<()> {
    let e = root.create_namespace(
        "traversal",
        Box::new(move |env| {
            find::Find::declare(env)?;
            env.declare_command(
                "cd",
                cd,
                true,
                "cd directory:(file,string,glob)",
                "Change to the specified working directory",
                None,
                Known(ValueType::Empty),
            )?;
            env.declare_command(
                "pwd",
                pwd,
                false,
                "pwd",
                "Return the current working directory",
                None,
                Known(ValueType::File),
            )?;
            env.declare_command(
                "help",
                help,
                false,
                "help topic:any",
                "Show help about the specified thing",
                Some(
                    r#"    Examples:

    help ls
    help integer
    help help"#,
                ),
                Known(ValueType::Empty),
            )?;
            Ok(())
        }),
    )?;
    root.r#use(&e);
    Ok(())
}
