//! Minimal HTML builder for generating HTML strings with a fluent API.
//! Example usage:
//! let doc = HtmlDoc::new();
//! let mut div = doc.div();
//! div.attr("class", "section");
//! div.style("color", "red");
//! div.style("display", "block");
//! let mut p = div.p();
//! p.set_text("asdf");
//! let html = div.to_html();

#[derive(Debug, Clone, Default)]
pub struct HtmlDoc;

impl HtmlDoc {
    pub fn new() -> Self {
        HtmlDoc
    }

    /// Create a <div> element
    pub fn div(&self) -> Element {
        Element::new("div")
    }

    /// Create an element with an arbitrary tag name
    pub fn el(&self, tag: &str) -> Element {
        Element::new(tag)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Element {
    tag: String,
    attrs: Vec<(String, String)>,
    styles: Vec<(String, String)>,
    children: Vec<Element>,
    text: Option<String>,
}

impl Element {
    pub fn new(tag: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            attrs: Vec::new(),
            styles: Vec::new(),
            children: Vec::new(),
            text: None,
        }
    }

    /// Set or add a generic attribute
    pub fn attr(&mut self, name: impl Into<String>, value: impl Into<String>) -> &mut Self {
        let name = name.into();
        let value = value.into();
        // If attribute exists, replace; otherwise push
        if let Some(pos) = self
            .attrs
            .iter()
            .position(|(n, _)| n.eq_ignore_ascii_case(&name))
        {
            self.attrs[pos] = (name, value);
        } else {
            self.attrs.push((name, value));
        }
        self
    }

    /// Add a style declaration (merged into the style attribute)
    pub fn style(&mut self, name: impl Into<String>, value: impl Into<String>) -> &mut Self {
        let name = name.into();
        let value = value.into();
        // Replace existing style with same key, otherwise push
        if let Some(pos) = self
            .styles
            .iter()
            .position(|(n, _)| n.eq_ignore_ascii_case(&name))
        {
            self.styles[pos] = (name, value);
        } else {
            self.styles.push((name, value));
        }
        self
    }

    /// Set plain text content of this element (overwrites previous text)
    pub fn set_text(&mut self, text: impl Into<String>) -> &mut Self {
        self.text = Some(text.into());
        self
    }

    /// Append a new child element with the given tag and return a mutable reference to it
    pub fn child(&mut self, tag: impl Into<String>) -> &mut Element {
        self.children.push(Element::new(tag));
        let idx = self.children.len() - 1;
        &mut self.children[idx]
    }

    /// Convenience: append a <div> child
    pub fn div(&mut self) -> &mut Element {
        self.child("div")
    }

    /// Convenience: append a <p> child
    pub fn p(&mut self) -> &mut Element {
        self.child("p")
    }

    /// Convenience: append a <span> child
    pub fn span(&mut self) -> &mut Element {
        self.child("span")
    }

    /// Render the element and its subtree into an HTML string
    pub fn to_html(&self) -> String {
        let mut s = String::new();
        self.write_html(&mut s);
        s
    }

    fn write_html(&self, out: &mut String) {
        out.push('<');
        out.push_str(&self.tag);

        // Collect attributes into a single vector, merging styles into a style attribute
        if !self.styles.is_empty() {
            let mut style_value = String::new();
            for (i, (k, v)) in self.styles.iter().enumerate() {
                if i > 0 {
                    style_value.push(';');
                }
                style_value.push_str(k);
                style_value.push(':');
                style_value.push_str(v);
            }
            self.push_attr(out, "style", &style_value);
        }
        for (name, value) in &self.attrs {
            // Skip style here if present; style() already handled it above
            if name.eq_ignore_ascii_case("style") {
                continue;
            }
            self.push_attr(out, name, value);
        }

        if self.text.is_none() && self.children.is_empty() {
            // Empty node: use short form <tag></tag> (avoid self-closing for HTML)
            out.push('>');
            out.push_str("</");
            out.push_str(&self.tag);
            out.push('>');
            return;
        }

        out.push('>');

        if let Some(text) = &self.text {
            push_escaped(out, text);
        }
        for child in &self.children {
            child.write_html(out);
        }

        out.push_str("</");
        out.push_str(&self.tag);
        out.push('>');
    }

    fn push_attr(&self, out: &mut String, name: &str, value: &str) {
        out.push(' ');
        out.push_str(name);
        out.push('=');
        out.push('"');
        push_escaped_attr(out, value);
        out.push('"');
    }
}

fn push_escaped(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
}

fn push_escaped_attr(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_expected_html() {
        let doc = HtmlDoc::new();
        let mut div = doc.div();
        div.attr("class", "section");
        div.style("color", "red");
        div.style("display", "block");
        let mut p = div.p();
        p.set_text("asdf");
        let html = div.to_html();
        assert_eq!(
            html,
            "<div style=\"color:red;display:block\" class=\"section\"><p>asdf</p></div>"
        );
    }

    #[test]
    fn escaping_works() {
        let mut el = Element::new("span");
        el.attr("title", "a < b & c \"quoted\"");
        el.set_text("Tom & Jerry <3");
        let html = el.to_html();
        assert_eq!(
            html,
            "<span title=\"a &lt; b &amp; c &quot;quoted&quot;\">Tom &amp; Jerry &lt;3</span>"
        );
    }
}
