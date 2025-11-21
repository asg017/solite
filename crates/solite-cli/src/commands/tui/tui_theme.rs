use crate::themes::SoliteColor;
use std::sync::LazyLock;

#[derive(Clone)]
pub(crate) struct TuiTheme {
  pub base: SoliteColor,
  pub keycap: SoliteColor,
  pub null: SoliteColor,
  pub integer: SoliteColor,
  pub double: SoliteColor,
  pub text: SoliteColor,
  pub blob: SoliteColor,
  pub table_fg: SoliteColor,
  pub row_hl_bg: SoliteColor,
  pub cell_hl_bg: SoliteColor,
  pub cell_hl_fg: SoliteColor,
  pub header_fg: SoliteColor,
  pub header_bg: SoliteColor,
  pub header_selected_bg: SoliteColor,
  pub header_style_fg: SoliteColor,
}




use crate::themes::ctp_mocha_colors;

pub(crate) const CTP_MOCHA_THEME: LazyLock<TuiTheme> = LazyLock::new(|| TuiTheme {
  base: ctp_mocha_colors::BASE.clone(),
  keycap: ctp_mocha_colors::GREEN.clone(),

  null: ctp_mocha_colors::SUBTEXT1.clone(),
  integer: ctp_mocha_colors::PEACH.clone(),
  double: ctp_mocha_colors::PEACH.clone(),
  text: ctp_mocha_colors::TEXT.clone(),
  blob: ctp_mocha_colors::FLAMINGO.clone(),
  table_fg: ctp_mocha_colors::TEXT.clone(),
  row_hl_bg: ctp_mocha_colors::SURFACE0.clone(),
  cell_hl_bg: ctp_mocha_colors::CRUST.clone(),
  cell_hl_fg: ctp_mocha_colors::TEXT.clone(),
  header_fg: ctp_mocha_colors::BLUE.clone(),
  header_bg: ctp_mocha_colors::BASE.clone(),
  header_selected_bg: ctp_mocha_colors::OVERLAY0.clone(),
  header_style_fg: ctp_mocha_colors::YELLOW.clone(),

});

pub(crate) static BUILTIN_THEMES: LazyLock<Vec<(&'static str, TuiTheme)>> = LazyLock::new(|| vec![
  (
    "ctp-mocha",
    CTP_MOCHA_THEME.clone(),
  ),
]);