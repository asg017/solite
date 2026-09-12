//! General CSS custom properties for every theme role.
//!
//! Generalizes the `json_viewer_theme_vars` pattern (which declares a small,
//! fixed set of `--jt-*` variables for the interactive JSON tree) to every
//! role on [`Theme`], each named `--solite-<role>` (underscores become
//! hyphens, e.g. `string_literal` -> `--solite-string-literal`).
//!
//! Declared once as a `style` attribute on a wrapping element, consumers
//! then reference `var(--solite-integer, currentColor)` instead of baking a
//! resolved color into every single cell/span — smaller HTML for large
//! result sets, and a page embedding Solite output can override a role by
//! redefining its variable without any re-rendering.

use solite_theme::Theme;

/// Generate `--solite-<role>: <css-color>;` declarations for every role in
/// [`Theme::ROLE_NAMES`]. Intended for a `style` attribute on a container
/// element (e.g. the `.solite-output` wrapper div).
pub fn theme_css_vars(theme: &Theme) -> String {
    let mut out = String::new();
    for name in Theme::ROLE_NAMES {
        let style = theme
            .role(name)
            .expect("Theme::ROLE_NAMES entries are always valid role names");
        let var_name = name.replace('_', "-");
        out.push_str(&format!("--solite-{var_name}: {}; ", style.fg.to_css()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_a_var_for_every_role() {
        let theme = Theme::catppuccin_mocha();
        let vars = theme_css_vars(&theme);
        for name in Theme::ROLE_NAMES {
            let var_name = format!("--solite-{}:", name.replace('_', "-"));
            assert!(vars.contains(&var_name), "missing {var_name} in {vars}");
        }
    }

    #[test]
    fn integer_var_carries_catppuccin_peach() {
        let theme = Theme::catppuccin_mocha();
        let vars = theme_css_vars(&theme);
        assert!(vars.contains("--solite-integer: #fab387"));
    }

    #[test]
    fn terminal_theme_uses_ansi_css_vars() {
        let theme = Theme::terminal();
        let vars = theme_css_vars(&theme);
        // Integer is ANSI yellow in the terminal theme: emitted as a CSS
        // variable reference with a hex fallback, never a bare hex literal.
        assert!(vars.contains("--solite-integer: var(--solite-ansi-yellow"));
        // Text is the terminal-default sentinel: inherits the page's color.
        assert!(vars.contains("--solite-text: currentColor"));
    }
}
