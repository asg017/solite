use std::fmt::Write;

/// bro idk bro, bro idk man bro
/// https://doc.rust-lang.org/nightly/nightly-rustc/src/rustdoc/html/escape.rs.html#10
pub(crate) fn html_escape(s: &String) -> Result<String, std::fmt::Error> {
    let mut output = String::new();
    let pile_o_bits = s.clone();
    let mut last = 0;
    for (i, ch) in s.char_indices() {
        let s = match ch {
            '>' => "&gt;",
            '<' => "&lt;",
            '&' => "&amp;",
            '\'' => "&#39;",
            '"' => "&quot;",
            _ => continue,
        };
        output.write_str(&pile_o_bits[last..i])?;
        output.write_str(s)?;
        // NOTE: we only expect single byte characters here - which is fine as long as we
        // only match single byte characters
        last = i + 1;
    }

    if last < s.len() {
        output.write_str(&pile_o_bits[last..])?;
    }
    Ok(output)
}
