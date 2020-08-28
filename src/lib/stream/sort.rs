use crate::lang::argument::ArgumentHandler;
use crate::lang::command::OutputType::Passthrough;
use crate::lang::errors::{error, CrushResult};
use crate::lang::execution_context::CommandContext;
use crate::lang::stream::CrushStream;
use crate::lang::data::table::ColumnVec;
use crate::lang::data::table::Row;
use crate::lang::value::Field;
use crate::{lang::errors::argument_error, lang::stream::OutputStream};
use signature::signature;

#[signature(
    sort,
    can_block=true,
    short="Sort io based on column",
    long="ps | sort ^cpu",
    output=Passthrough)]
pub struct Sort {
    #[description("the column to sort on. Not required if there is only one column.")]
    field: Option<Field>,
}

pub fn run(idx: usize, input: &mut dyn CrushStream, output: OutputStream) -> CrushResult<()> {
    let mut res: Vec<Row> = Vec::new();
    while let Ok(row) = input.read() {
        res.push(row);
    }

    res.sort_by(|a, b| a.cells()[idx].partial_cmp(&b.cells()[idx]).expect("OH NO!"));

    for row in res {
        output.send(row)?;
    }

    Ok(())
}

pub fn sort(context: CommandContext) -> CrushResult<()> {
    match context.input.recv()?.stream() {
        Some(mut input) => {
            let output = context.output.initialize(input.types().to_vec())?;
            let cfg: Sort = Sort::parse(context.arguments, &context.printer)?;
            let idx = match cfg.field {
                None => {
                    if input.types().len() == 1 {
                        0
                    } else {
                        return argument_error("Missing comparison key");
                    }
                }
                Some(field) => input.types().find(&field)?,
            };

            if input.types()[idx].cell_type.is_comparable() {
                run(idx, input.as_mut(), output)
            } else {
                argument_error("Bad comparison key")
            }
        }
        None => error("Expected a stream"),
    }
}
