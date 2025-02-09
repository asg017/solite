use arboard::Clipboard;
use solite_core::{
    replacement_scans::replacement_scan,
    sqlite::{ValueRefX, ValueRefXValue},
    Runtime,
};
use std::{
    fs::File,
    io::{stdout, BufWriter, Write},
};

use crate::cli::QueryFlags;

fn write_json_row<W>(
    output: &mut W,
    columns: &[String],
    row: Vec<ValueRefX>,
) -> Result<(), serde_json::Error>
where
    W: std::io::Write,
{
    let mut obj = serde_json::Map::new();
    for (idx, value) in row.iter().enumerate() {
        let k = columns.get(idx).unwrap().to_owned();
        let jvalue = match value.value {
            ValueRefXValue::Null => serde_json::Value::Null,
            ValueRefXValue::Int(value) => serde_json::Value::Number((value).into()),
            ValueRefXValue::Double(value) => {
                serde_json::Value::Number(serde_json::Number::from_f64(value).unwrap())
            }
            ValueRefXValue::Text(text) => {
                if let Some(74) = value.subtype() {
                    serde_json::from_slice(text).unwrap()
                } else {
                    serde_json::Value::String(unsafe { String::from_utf8_unchecked(text.to_vec()) })
                }
            }
            // BLOBs can't be serialized to JSON easily.
            // TODO: maybe base64 option?
            ValueRefXValue::Blob(_value) => serde_json::Value::Null,
        };
        obj.insert(k, jvalue);
    }
    let obj = serde_json::Value::Object(obj);
    serde_json::to_writer(output, &obj)
}
fn write_csv_row<W>(writer: &mut csv::Writer<W>, row: Vec<ValueRefX>) -> Result<(), csv::Error>
where
    W: std::io::Write,
{
    writer.write_record(row.iter().map(|value| match value.value {
        ValueRefXValue::Null => String::new(),
        ValueRefXValue::Blob(_) => String::new(),
        ValueRefXValue::Int(value) => value.to_string(),
        ValueRefXValue::Double(value) => value.to_string(),
        ValueRefXValue::Text(value) => unsafe { String::from_utf8_unchecked(value.to_vec()) },
    }))
}

pub(crate) fn query(flags: QueryFlags) -> Result<(), ()> {
    let mut runtime = Runtime::new(flags.database);
    for (key, value) in flags.parameters {
        runtime.define_parameter(key, value).unwrap();
    }
    let mut stmt;
    loop {
        stmt = match runtime.prepare_with_parameters(flags.statement.as_str()) {
            Ok((_, Some(stmt))) => Some(stmt),
            Ok((_, None)) => todo!(),
            Err(err) => match replacement_scan(&err, &runtime.connection) {
                Some(Ok(stmt)) => {
                    stmt.execute().unwrap();
                    None
                }
                Some(Err(_)) => todo!(),
                None => {
                    crate::errors::report_error("[input]", flags.statement.as_str(), &err, None);
                    return Err(());
                }
            },
        };
        if stmt.is_some() {
            break;
        }
    }
    let stmt = stmt.unwrap();

    if !stmt.readonly() {
        eprintln!("only read-only statements are allowed in `solite query`. Use `solite exec` instead to modify the database.");
        return Err(());
    }

    let mut output: Box<dyn Write> = match flags.output {
        Some(ref output) => {
            let f = File::create(output).unwrap();

            // TODO make sure there's no compression going on if --format=value
            if output.extension().map(|v| v == "gz").unwrap_or(false) {
                let encoder = flate2::write::GzEncoder::new(f, flate2::Compression::default());
                Box::new(BufWriter::new(encoder))
            } else if output.extension().map(|v| v == "zst").unwrap_or(false) {
                let encoder = zstd::stream::write::Encoder::new(f, 3).unwrap();
                Box::new(BufWriter::new(encoder))
            } else {
                Box::new(BufWriter::new(f))
            }
        }
        None => Box::new(stdout()),
    };
    let format = match flags.format {
        Some(format) => format,
        None => match flags.output {
            Some(p) => match p.extension() {
                Some(ext) => {
                    let mut ext = ext.to_str().unwrap().to_string();
                    if ext == "gz" || ext == "zst" {
                        let p = p.with_extension("");
                        ext = p.extension().unwrap().to_str().unwrap().to_string();
                    }
                    match ext.as_str() {
                        "csv" => crate::cli::QueryFormat::Csv,
                        "tsv" => crate::cli::QueryFormat::Tsv,
                        "json" => crate::cli::QueryFormat::Json,
                        "ndjson" | "jsonl" => crate::cli::QueryFormat::Ndjson,
                        _ => todo!("unknown extension"),
                    }
                }
                None => crate::cli::QueryFormat::Json,
            },
            None => crate::cli::QueryFormat::Json,
        },
    };

    match format {
        crate::cli::QueryFormat::Csv => {
            let mut writer = csv::Writer::from_writer(output);
            writer.write_record(stmt.column_names().unwrap()).unwrap();
            loop {
                match stmt.next() {
                    Ok(Some(row)) => {
                        write_csv_row(&mut writer, row).unwrap();
                    }
                    Ok(None) => break,
                    Err(error) => {
                        eprintln!("{}", error);
                        return Err(());
                    }
                }
            }
            writer.flush().unwrap();
        }
        crate::cli::QueryFormat::Tsv => {
            let mut writer = csv::WriterBuilder::new()
                .delimiter(b'\t')
                .from_writer(output);
            writer.write_record(stmt.column_names().unwrap()).unwrap();
            loop {
                match stmt.next() {
                    Ok(Some(row)) => {
                        write_csv_row(&mut writer, row).unwrap();
                    }
                    Ok(None) => break,
                    Err(error) => {
                        eprintln!("{}", error);
                        return Err(());
                    }
                }
            }
            writer.flush().unwrap();
        }
        crate::cli::QueryFormat::Json => {
            output.write_all(&[b'[']).unwrap();
            let columns = stmt.column_names().unwrap();
            let mut first = true;
            loop {
                match stmt.next() {
                    Ok(Some(row)) => {
                        if first {
                            first = false;
                        } else {
                            output.write_all(&[b',']).unwrap();
                        }
                        write_json_row(&mut output, &columns, row).unwrap();
                    }
                    Ok(None) => break,
                    Err(error) => {
                        eprintln!("{}", error);
                        return Err(());
                    }
                }
            }
            output.write_all(&[b']', b'\n']).unwrap();
        }
        crate::cli::QueryFormat::Ndjson => {
            let columns = stmt.column_names().unwrap();
            loop {
                match stmt.next() {
                    Ok(Some(row)) => {
                        write_json_row(&mut output, &columns, row).unwrap();
                        output.write_all(&[b'\n']).unwrap();
                    }
                    Ok(None) => break,
                    Err(error) => {
                        eprintln!("{error}");
                        return Err(());
                    }
                }
            }
        }
        crate::cli::QueryFormat::Clipboard => {
            let mut num_rows = 0;
            let mut html = "".to_owned();
            html += "<table> <thead> <tr>";

            let columns = stmt.column_names().unwrap();
            for column in columns {
                html.push_str("<td>");
                html.push_str(column.as_str());
                html.push_str("</td>");
            }

            html += "</tr> </thead>";
            html += "<tbody>";
            loop {
                match stmt.next() {
                    Ok(Some(row)) => {
                        html += "<tr>";
                        for cell in row {
                            let v = match cell.value {
                                ValueRefXValue::Null => "".to_owned(),
                                ValueRefXValue::Int(v) => v.to_string(),
                                ValueRefXValue::Double(v) => v.to_string(),
                                ValueRefXValue::Text(v) => {
                                    std::str::from_utf8(v).unwrap().to_owned()
                                }
                                ValueRefXValue::Blob(_) => todo!(),
                            };
                            html.push_str("<td>");
                            html.push_str(v.as_str());
                            html.push_str("</td>");
                        }
                        html += "</tr>";
                        num_rows += 1;
                        //output.write_all(&[b'\n']).unwrap();
                    }
                    Ok(None) => break,
                    Err(error) => {
                        eprintln!("{error}");
                        return Err(());
                    }
                }
            }
            html += "</tbody>";
            html += "</table>";

            let mut clipboard = Clipboard::new().unwrap();
            // TODO write TSV equivalent to alt_text
            clipboard.set_html(html, Some("".to_owned())).unwrap();
            println!(
                "✓ Wrote {} {} to clipboard",
                num_rows,
                if num_rows == 1 { "row" } else { "rows" }
            );
        }
        crate::cli::QueryFormat::Value => {
            match stmt.next() {
                Ok(Some(row)) => {
                    let value = row.get(0).unwrap();
                    match value.value {
                        ValueRefXValue::Null => (),
                        ValueRefXValue::Int(value) => {
                            output.write_fmt(format_args!("{}", value)).unwrap()
                        }
                        ValueRefXValue::Double(value) => {
                            output.write_fmt(format_args!("{}", value)).unwrap()
                        }
                        ValueRefXValue::Blob(value) | ValueRefXValue::Text(value) => {
                            output.write_all(value).unwrap()
                        }
                    };
                }
                Ok(None) => {
                    eprintln!("No rows returned in query.");
                    return Err(());
                }
                Err(error) => {
                    eprintln!("Error running query: {}", error);
                    return Err(());
                }
            };
            match stmt.next() {
                Ok(None) => (),
                Ok(Some(_)) => {
                    eprintln!("More than 1 query returned, exepcted a single row. Try a `LIMIT 1`");
                    return Err(());
                }
                Err(error) => {
                    eprintln!("Error stepping through next row: {error}");
                    return Err(());
                }
            }
        }
    }

    Ok(())
}
