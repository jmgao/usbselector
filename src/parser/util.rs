use winnow::{
  self, ModalResult, Parser,
  combinator::{alt, cut_err, preceded, repeat, separated_pair, terminated},
  error::StrContext,
  token::{any, none_of, take},
};

// Stolen from winnow's example json parser:
pub fn string(input: &mut &str) -> ModalResult<String> {
  preceded('"', cut_err(terminated(repeat(0.., character), '"')))
    .context(StrContext::Expected("string".into()))
    .parse_next(input)
}

pub fn character(input: &mut &str) -> ModalResult<char> {
  let c = none_of('"').parse_next(input)?;
  if c == '\\' {
    alt((
      any.verify_map(|c| {
        Some(match c {
          '"' | '\\' | '/' => c,
          'b' => '\x08',
          'f' => '\x0C',
          'n' => '\n',
          'r' => '\r',
          't' => '\t',
          _ => return None,
        })
      }),
      preceded('u', unicode_escape),
    ))
    .parse_next(input)
  } else {
    Ok(c)
  }
}

pub fn unicode_escape(input: &mut &str) -> ModalResult<char> {
  alt((
    // Not a surrogate
    u16_hex.verify(|cp| !(0xD800..0xE000).contains(cp)).map(|cp| cp as u32),
    // See https://en.wikipedia.org/wiki/UTF-16#Code_points_from_U+010000_to_U+10FFFF for details
    separated_pair(u16_hex, "\\u", u16_hex)
      .verify(|(high, low)| (0xD800..0xDC00).contains(high) && (0xDC00..0xE000).contains(low))
      .map(|(high, low)| {
        let high_ten = (high as u32) - 0xD800;
        let low_ten = (low as u32) - 0xDC00;
        (high_ten << 10) + low_ten + 0x10000
      }),
  ))
  .verify_map(std::char::from_u32)
  .parse_next(input)
}

pub fn u16_hex(input: &mut &str) -> ModalResult<u16> {
  take(4usize)
    .verify_map(|s| u16::from_str_radix(s, 16).ok())
    .parse_next(input)
}
