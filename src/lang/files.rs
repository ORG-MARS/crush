use crate::lang::data::binary::{binary_channel, BinaryReader};
use crate::lang::errors::{argument_error_legacy, error, to_crush_error, CrushResult};
use crate::lang::printer::Printer;
use crate::lang::stream::{ValueReceiver, ValueSender};
use crate::lang::value::{Value, ValueType};
use crate::util::file::cwd;
use crate::util::regex::RegexFileMatcher;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug)]
pub struct Files {
    had_entries: bool,
    files: Vec<PathBuf>,
}

impl Files {
    pub fn new() -> Files {
        Files {
            had_entries: false,
            files: Vec::new(),
        }
    }

    pub fn had_entries(&self) -> bool {
        self.had_entries
    }

    pub fn into_vec(self) -> Vec<PathBuf> {
        self.files
    }

    pub fn into_file(&self) -> CrushResult<PathBuf> {
        if self.files.len() == 1 {
            Ok(self.files[0].clone())
        } else {
            error("Invalid file")
        }
    }

    pub fn reader(self, input: ValueReceiver) -> CrushResult<Box<dyn BinaryReader + Send + Sync>> {
        if !self.had_entries {
            match input.recv()? {
                Value::BinaryStream(b) => Ok(b),
                Value::Binary(b) => Ok(BinaryReader::vec(&b)),
                _ => argument_error_legacy("Expected either a file to read or binary pipe io"),
            }
        } else {
            BinaryReader::paths(self.files)
        }
    }

    pub fn writer(self, output: ValueSender) -> CrushResult<Box<dyn Write>> {
        if !self.had_entries {
            let (w, r) = binary_channel();
            output.send(Value::BinaryStream(r))?;
            Ok(w)
        } else if self.files.len() == 1 {
            output.send(Value::Empty())?;
            Ok(Box::from(to_crush_error(File::create(
                self.files[0].clone(),
            ))?))
        } else {
            argument_error_legacy("Expected exactly one desitnation file")
        }
    }

    pub fn expand(&mut self, value: Value, printer: &Printer) -> CrushResult<()> {
        match value {
            Value::File(p) => self.files.push(p),
            Value::Glob(pattern) => pattern.glob_files(&PathBuf::from("."), &mut self.files)?,
            Value::Regex(_, re) => re.match_files(&cwd()?, &mut self.files, printer),
            value => match value.stream() {
                None => return argument_error_legacy("Expected a file name"),
                Some(mut s) => {
                    let t = s.types();
                    if t.len() == 1 && t[0].cell_type == ValueType::File {
                        while let Ok(row) = s.read() {
                            if let Value::File(f) = row.into_vec().remove(0) {
                                self.files.push(f);
                            }
                        }
                    } else {
                        return argument_error_legacy("Table stream must contain one column of type file");
                    }
                }
            },
        }
        self.had_entries = true;
        Ok(())
    }
}
