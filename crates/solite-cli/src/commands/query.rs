use solite_core::{exporter::ExportFormat, replacement_scans::replacement_scan, Runtime};
use std::{
    fmt,
    io::{stdout, Write},
};

use crate::cli::QueryArgs;

fn query_impl(args: QueryArgs, is_exec: bool) -> anyhow::Result<()> {
    let mut runtime = Runtime::new(args.database.map(|p| p.to_string_lossy().to_string()));
    for chunk in args.parameters.chunks(2) {
        runtime
            .define_parameter(chunk[0].clone(), chunk[1].clone())
            .unwrap();
    }
    let statement = args.statement;
    let mut stmt;
    loop {
        stmt = match runtime.prepare_with_parameters(statement.as_str()) {
            Ok((_, Some(stmt))) => Some(stmt),
            Ok((_, None)) => todo!(),
            Err(err) => match replacement_scan(&err, &runtime.connection) {
                Some(Ok(stmt)) => {
                    stmt.execute().unwrap();
                    None
                }
                Some(Err(_)) => todo!(),
                None => {
                    crate::errors::report_error("[input]", statement.as_str(), &err, None);
                    return Err(MyError::new().into());
                }
            },
        };
        if stmt.is_some() {
            break;
        }
    }
    let mut stmt = stmt.unwrap();

    if !is_exec && !stmt.readonly() {
        return Err(anyhow::anyhow!("only read-only statements are allowed in `solite query`. Use `solite exec` instead to modify the database."));
    }

    let output: Box<dyn Write> = match args.output {
        Some(ref output) => solite_core::exporter::output_from_path(output)?,
        None => Box::new(stdout()),
    };

    if is_exec && stmt.column_names().unwrap().len() == 0 {
        loop {
            match stmt.next() {
                Ok(Some(row)) => (),
                Ok(None) => break,
                Err(error) => {
                    eprintln!("{}", error);
                    todo!()
                }
            }
        }
        println!("✔︎");
        return Ok(());
    }

    let format = match args.format {
        Some(format) => format.into(),
        None => match args.output {
            Some(p) => solite_core::exporter::format_from_path(&p).unwrap_or(ExportFormat::Json),
            None => ExportFormat::Json,
        },
    };

    solite_core::exporter::write_output(&mut stmt, output, format)?;

    Ok(())
}


#[derive(Debug)] // Debug is required for all Error types
pub struct MyError {
    details: String,
}

impl MyError {
    pub fn new() -> Self {
        Self {
            details: "".to_owned(),
        }
    }
}

// 2. Implement Display (human-readable error message)
impl fmt::Display for MyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}

// 3. Implement the std::error::Error marker trait
impl std::error::Error for MyError {
    // Optional: report an underlying cause, if you store one
    // fn source(&self) -> Option<&(dyn Error + 'static)> { None }
}
pub(crate) fn query(args: QueryArgs, is_exec: bool) -> Result<(), ()> {
    match query_impl(args, is_exec) {
        Ok(_) => Ok(()),
        Err(err) => {
            if !err.is::<MyError>() {
                eprintln!("{}", err);
            }
            Err(())
        }
    }
}
