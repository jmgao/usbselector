use super::util::*;
use std::collections::HashMap;

use winnow::{
  self, ModalResult, Parser,
  ascii::space0,
  combinator::{alt, cut_err, fail, opt, preceded, separated, seq},
  error::{ContextError, StrContext, StrContextValue},
  token::take_while,
};

use crate::{NodeType, PortNumber, Predicate};

fn parse_node_type(input: &mut &str) -> ModalResult<Option<NodeType>> {
  alt((
    "*".value(None),
    "hub".value(Some(NodeType::Hub)),
    "device".value(Some(NodeType::Device)),
    fail
      .context(StrContext::Label("node type"))
      .context(StrContext::Expected(StrContextValue::StringLiteral("*")))
      .context(StrContext::Expected(StrContextValue::StringLiteral("hub")))
      .context(StrContext::Expected(StrContextValue::StringLiteral("device"))),
  ))
  .parse_next(input)
}

fn noncomma<'s>(input: &mut &'s str) -> ModalResult<&'s str> {
  take_while(1.., ((0 as char..(b',') as char), ((b',' + 1) as char..=0xff as char))).parse_next(input)
}

fn parse_attribute_key<'s>(input: &mut &'s str) -> winnow::ModalResult<&'s str> {
  alt((
    "port",
    "id",
    "vid",
    "pid",
    "serial",
    "manufacturer",
    "product",
    fail
      .context(StrContext::Label("attribute name"))
      .context(StrContext::Expected(StrContextValue::StringLiteral("port")))
      .context(StrContext::Expected(StrContextValue::StringLiteral("id")))
      .context(StrContext::Expected(StrContextValue::StringLiteral("vid")))
      .context(StrContext::Expected(StrContextValue::StringLiteral("pid")))
      .context(StrContext::Expected(StrContextValue::StringLiteral("serial")))
      .context(StrContext::Expected(StrContextValue::StringLiteral("manufacturer")))
      .context(StrContext::Expected(StrContextValue::StringLiteral("product"))),
  ))
  .parse_next(input)
}

fn parse_attribute<'s>(input: &mut &'s str) -> ModalResult<(&'s str, String)> {
  let key = seq!(_: space0, parse_attribute_key, _: (space0, '=', space0)).parse_next(input)?;
  let value = match key.0 {
    "port" => cut_err(take_while(1.., '0'..='9'))
      .context(StrContext::Label("port"))
      .parse_next(input)?
      .into(),
    "id" => {
      opt("0x").parse_next(input)?;
      let vid = cut_err(take_while(1..=4, ('0'..='9', 'a'..='f', 'A'..='F')))
        .context(StrContext::Label("id"))
        .parse_next(input)?;
      cut_err(':').context(StrContext::Label("id")).parse_next(input)?;
      opt("0x").parse_next(input)?;
      let pid = cut_err(take_while(1..=4, ('0'..='9', 'a'..='f', 'A'..='F')))
        .context(StrContext::Label("id"))
        .parse_next(input)?;
      format!("{}:{}", vid, pid)
    }
    "vid" => cut_err(preceded(
      opt("0x"),
      take_while(1..=4, ('0'..='9', 'a'..='f', 'A'..='F')),
    ))
    .context(StrContext::Label("vendor id"))
    .parse_next(input)?
    .into(),
    "pid" => cut_err(preceded(
      opt("0x"),
      take_while(1..=4, ('0'..='9', 'a'..='f', 'A'..='F')),
    ))
    .context(StrContext::Label("product id"))
    .parse_next(input)?
    .into(),
    "serial" | "manufacturer" | "product" => string.context(StrContext::Label("quoted string")).parse_next(input)?,
    _ => unreachable!(),
  };

  Ok((key.0, value))
}

fn parse_attributes<'s>(input: &mut &'s str) -> ModalResult<HashMap<&'s str, String>> {
  let result = seq!(
    _: (space0, '[', space0), separated(1.., parse_attribute, (space0, ',', space0)), _:(space0, ']'))
  .parse_next(input)?;
  let attributes: Vec<_> = result.0;

  let mut result = HashMap::new();
  for (key, value) in attributes {
    if let Some(existing) = result.insert(key, value) {
      let mut err = ContextError::new();
      err.push(StrContext::Label("duplicate attribute"));
      return Err(winnow::error::ErrMode::Cut(err));
    }
  }

  Ok(result)
}

fn parse_hex_id(input: Option<&String>) -> ModalResult<Option<u16>> {
  let result = match input {
    None => None,
    Some(s) => {
      let parsed = u16::from_str_radix(s, 16).map_err(|e| {
        let mut err = ContextError::new();
        err.push(StrContext::Label("id"));
        winnow::error::ErrMode::Cut(err)
      });
      Some(parsed?)
    }
  };
  Ok(result)
}

pub(crate) fn parse_predicate(input: &mut &str) -> ModalResult<(Predicate, bool)> {
  space0.parse_next(input)?;
  let is_subject = opt("!").parse_next(input)?.is_some();
  space0.parse_next(input)?;
  let node_type = parse_node_type.parse_next(input)?;
  let mut attributes = {
    let checkpoint = *input;
    space0.parse_next(input)?;
    if input.starts_with('[') {
      *input = checkpoint; // Reset to before space0
      parse_attributes.parse_next(input)?
    } else {
      *input = checkpoint; // Reset to before space0
      HashMap::new()
    }
  };
  let port_number = match attributes.get(&"port") {
    Some(p) => Some(PortNumber(str::parse(p).map_err(|e| {
      let mut err = ContextError::new();
      err.push(StrContext::Label("port"));
      winnow::error::ErrMode::Cut(err)
    })?)),
    None => None,
  };

  let id = attributes.remove(&"id");
  let vendor_id = parse_hex_id(attributes.get(&"vid"))?;
  let product_id = parse_hex_id(attributes.get(&"pid"))?;

  // Check for conflicts between id and vid/pid
  if id.is_some() && (vendor_id.is_some() || product_id.is_some()) {
    let mut err = ContextError::new();
    err.push(StrContext::Label("conflicting id and vid/pid"));
    return Err(winnow::error::ErrMode::Cut(err));
  }

  let (vendor_id, product_id) = match id {
    Some(id_str) => {
      let parts: Vec<_> = id_str.split(':').collect();
      let vid = u16::from_str_radix(parts[0], 16).map_err(|e| {
        let mut err = ContextError::new();
        err.push(StrContext::Label("id"));
        winnow::error::ErrMode::Cut(err)
      })?;
      let pid = u16::from_str_radix(parts[1], 16).map_err(|e| {
        let mut err = ContextError::new();
        err.push(StrContext::Label("id"));
        winnow::error::ErrMode::Cut(err)
      })?;
      (Some(vid), Some(pid))
    }
    None => (vendor_id, product_id),
  };

  Ok((
    Predicate {
      node_type,
      port_number,
      vendor_id,
      product_id,
      serial_number: attributes.remove(&"serial"),
      manufacturer: attributes.remove(&"manufacturer"),
      product: attributes.remove(&"product"),
    },
    is_subject,
  ))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{NodeType, PortNumber};

  #[test]
  fn test_parse_node_type() {
    let mut input = "*";
    assert_eq!(parse_node_type(&mut input).unwrap(), None);

    let mut input = "hub";
    assert_eq!(parse_node_type(&mut input).unwrap(), Some(NodeType::Hub));

    let mut input = "device";
    assert_eq!(parse_node_type(&mut input).unwrap(), Some(NodeType::Device));

    let mut input = "invalid";
    assert!(parse_node_type(&mut input).is_err());
  }

  #[test]
  fn test_parse_attribute_key() {
    let mut input = "port";
    assert_eq!(parse_attribute_key(&mut input).unwrap(), "port");

    let mut input = "vid";
    assert_eq!(parse_attribute_key(&mut input).unwrap(), "vid");

    let mut input = "pid";
    assert_eq!(parse_attribute_key(&mut input).unwrap(), "pid");

    let mut input = "serial";
    assert_eq!(parse_attribute_key(&mut input).unwrap(), "serial");

    let mut input = "manufacturer";
    assert_eq!(parse_attribute_key(&mut input).unwrap(), "manufacturer");

    let mut input = "product";
    assert_eq!(parse_attribute_key(&mut input).unwrap(), "product");

    let mut input = "invalid";
    assert!(parse_attribute_key(&mut input).is_err());
  }

  #[test]
  fn test_parse_attribute_port() {
    let mut input = "port=123";
    let (key, value) = parse_attribute(&mut input).unwrap();
    assert_eq!(key, "port");
    assert_eq!(value, "123");
  }

  #[test]
  fn test_parse_attribute_vid() {
    let mut input = "vid=1234";
    let (key, value) = parse_attribute(&mut input).unwrap();
    assert_eq!(key, "vid");
    assert_eq!(value, "1234");

    let mut input = "vid=0x1234";
    let (key, value) = parse_attribute(&mut input).unwrap();
    assert_eq!(key, "vid");
    assert_eq!(value, "1234");
  }

  #[test]
  fn test_parse_attribute_pid() {
    let mut input = "pid=abcd";
    let (key, value) = parse_attribute(&mut input).unwrap();
    assert_eq!(key, "pid");
    assert_eq!(value, "abcd");

    let mut input = "pid=0xABCD";
    let (key, value) = parse_attribute(&mut input).unwrap();
    assert_eq!(key, "pid");
    assert_eq!(value, "ABCD");
  }

  #[test]
  fn test_parse_attribute_string() {
    let mut input = r#"serial="USB123""#;
    let (key, value) = parse_attribute(&mut input).unwrap();
    assert_eq!(key, "serial");
    assert_eq!(value, "USB123");

    let mut input = r#"manufacturer="Logitech""#;
    let (key, value) = parse_attribute(&mut input).unwrap();
    assert_eq!(key, "manufacturer");
    assert_eq!(value, "Logitech");

    let mut input = r#"product="USB Mouse""#;
    let (key, value) = parse_attribute(&mut input).unwrap();
    assert_eq!(key, "product");
    assert_eq!(value, "USB Mouse");
  }

  #[test]
  fn test_parse_attributes_single() {
    let mut input = "[port=3]";
    let attrs = parse_attributes(&mut input).unwrap();
    assert_eq!(attrs.len(), 1);
    assert_eq!(attrs.get("port").unwrap(), "3");
  }

  #[test]
  fn test_parse_attributes_multiple() {
    let mut input = r#"[port=3, vid=1234, manufacturer="Logitech"]"#;
    let attrs = parse_attributes(&mut input).unwrap();
    assert_eq!(attrs.len(), 3);
    assert_eq!(attrs.get("port").unwrap(), "3");
    assert_eq!(attrs.get("vid").unwrap(), "1234");
    assert_eq!(attrs.get("manufacturer").unwrap(), "Logitech");
  }

  #[test]
  fn test_parse_attributes_with_spaces() {
    let mut input = r#"[ port = 3 , vid = 1234 ]"#;
    let attrs = parse_attributes(&mut input).unwrap();
    assert_eq!(attrs.len(), 2);
    assert_eq!(attrs.get("port").unwrap(), "3");
    assert_eq!(attrs.get("vid").unwrap(), "1234");
  }

  #[test]
  fn test_parse_attributes_duplicate_error() {
    let mut input = "[port=3, port=4]";
    assert!(parse_attributes(&mut input).is_err());
  }

  #[test]
  fn test_parse_hex_id() {
    assert_eq!(parse_hex_id(None).unwrap(), None);
    assert_eq!(parse_hex_id(Some(&"1234".to_string())).unwrap(), Some(0x1234));
    assert_eq!(parse_hex_id(Some(&"abcd".to_string())).unwrap(), Some(0xABCD));
    assert_eq!(parse_hex_id(Some(&"ABCD".to_string())).unwrap(), Some(0xABCD));
    assert!(parse_hex_id(Some(&"invalid".to_string())).is_err());
  }

  #[test]
  fn test_parse_predicate_wildcard() {
    let mut input = "*";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(!is_subject);
    assert_eq!(predicate, Predicate::new());
  }

  #[test]
  fn test_parse_predicate_hub() {
    let mut input = "hub";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(!is_subject);
    assert_eq!(predicate, Predicate::new().with_node_type(NodeType::Hub));
  }

  #[test]
  fn test_parse_predicate_device() {
    let mut input = "device";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(!is_subject);
    assert_eq!(predicate, Predicate::new().with_node_type(NodeType::Device));
  }

  #[test]
  fn test_parse_predicate_with_port() {
    let mut input = "hub[port=3]";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(!is_subject);
    assert_eq!(
      predicate,
      Predicate::new().with_node_type(NodeType::Hub).with_port_number(3)
    );
  }

  #[test]
  fn test_parse_predicate_with_ids() {
    let mut input = "device[vid=1234, pid=5678]";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(!is_subject);
    assert_eq!(
      predicate,
      Predicate::new()
        .with_node_type(NodeType::Device)
        .with_vendor_id(0x1234)
        .with_product_id(0x5678)
    );
  }

  #[test]
  fn test_parse_predicate_with_id() {
    let mut input = "device[id=18d1:1234]";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(!is_subject);
    assert_eq!(
      predicate,
      Predicate::new()
        .with_node_type(NodeType::Device)
        .with_vendor_id(0x18d1)
        .with_product_id(0x1234)
    );
  }

  #[test]
  fn test_parse_predicate_with_id_uppercase() {
    let mut input = "device[id=ABCD:EF01]";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(!is_subject);
    assert_eq!(
      predicate,
      Predicate::new()
        .with_node_type(NodeType::Device)
        .with_vendor_id(0xABCD)
        .with_product_id(0xEF01)
    );
  }

  #[test]
  fn test_parse_predicate_with_id_0x_prefix() {
    let mut input = "device[id=0x18d1:1234]";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(!is_subject);
    assert_eq!(
      predicate,
      Predicate::new()
        .with_node_type(NodeType::Device)
        .with_vendor_id(0x18d1)
        .with_product_id(0x1234)
    );
  }

  #[test]
  fn test_parse_predicate_id_conflict_with_vid() {
    let mut input = "device[id=18d1:1234, vid=0000]";
    assert!(parse_predicate(&mut input).is_err());
  }

  #[test]
  fn test_parse_predicate_id_conflict_with_pid() {
    let mut input = "device[id=18d1:1234, pid=0000]";
    assert!(parse_predicate(&mut input).is_err());
  }

  #[test]
  fn test_parse_predicate_with_strings() {
    let mut input = r#"device[serial="USB123", manufacturer="Logitech", product="USB Mouse"]"#;
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(!is_subject);
    assert_eq!(
      predicate,
      Predicate::new()
        .with_node_type(NodeType::Device)
        .with_serial_number("USB123".into())
        .with_manufacturer("Logitech".into())
        .with_product("USB Mouse".into())
    );
  }

  #[test]
  fn test_parse_port_number_error() {
    let mut input = "hub[port=invalid]";
    assert!(parse_predicate(&mut input).is_err());
  }

  #[test]
  fn test_parse_hex_id_error() {
    let mut input = "device[vid=xyz]";
    assert!(parse_predicate(&mut input).is_err());
  }

  #[test]
  fn test_parse_attributes_with_extra_spaces() {
    let mut input = "[  port  =  123  ,  vid  =  abcd  ]";
    let attrs = parse_attributes(&mut input).unwrap();
    assert_eq!(attrs.len(), 2);
    assert_eq!(attrs.get("port").unwrap(), "123");
    assert_eq!(attrs.get("vid").unwrap(), "abcd");
  }

  #[test]
  fn test_parse_hex_id_boundary_values() {
    assert_eq!(parse_hex_id(Some(&"0000".to_string())).unwrap(), Some(0x0000));
    assert_eq!(parse_hex_id(Some(&"9999".to_string())).unwrap(), Some(0x9999));
    assert_eq!(parse_hex_id(Some(&"aaaa".to_string())).unwrap(), Some(0xaaaa));
    assert_eq!(parse_hex_id(Some(&"AAAA".to_string())).unwrap(), Some(0xAAAA));
    assert_eq!(parse_hex_id(Some(&"ffff".to_string())).unwrap(), Some(0xffff));
    assert_eq!(parse_hex_id(Some(&"FFFF".to_string())).unwrap(), Some(0xFFFF));
  }

  #[test]
  fn test_parse_port_boundary_values() {
    let mut input = "*[port=0]";
    let (predicate, _) = parse_predicate(&mut input).unwrap();
    assert_eq!(predicate.port_number, Some(PortNumber(0)));

    let mut input = "*[port=255]";
    let (predicate, _) = parse_predicate(&mut input).unwrap();
    assert_eq!(predicate.port_number, Some(PortNumber(255)));

    let mut input = "*[port=256]";
    assert!(parse_predicate(&mut input).is_err());
  }

  #[test]
  fn test_parse_predicate_all_attributes() {
    let mut input = r#"device[port=3, vid=1234, pid=5678, serial="SN123", manufacturer="ACME", product="Widget"]"#;
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(!is_subject);
    assert_eq!(predicate.node_type, Some(NodeType::Device));
    assert_eq!(predicate.port_number, Some(PortNumber(3)));
    assert_eq!(predicate.vendor_id, Some(0x1234));
    assert_eq!(predicate.product_id, Some(0x5678));
    assert_eq!(predicate.serial_number, Some("SN123".to_string()));
    assert_eq!(predicate.manufacturer, Some("ACME".to_string()));
    assert_eq!(predicate.product, Some("Widget".to_string()));
  }

  #[test]
  fn test_parse_error_invalid_hex_length() {
    // Hex values longer than 4 characters should be rejected
    let mut input = "device[vid=12345]"; // Too long
    let result = parse_predicate(&mut input);
    assert!(result.is_err(), "5-character hex should be rejected");

    // Hex values shorter than 4 characters should be accepted (equivalent to 0-padded)
    let mut input = "device[vid=123]"; // Short but valid
    let result = parse_predicate(&mut input);
    assert!(result.is_ok(), "3-character hex should be accepted");
    let (predicate, _) = result.unwrap();
    assert_eq!(predicate.vendor_id, Some(0x123)); // Should parse as 0x0123
  }

  #[test]
  fn test_parse_error_missing_quotes() {
    let mut input = "device[manufacturer=Logitech]"; // Missing quotes
    let result = parse_predicate(&mut input);
    assert!(result.is_err(), "String attributes should require quotes");
  }

  #[test]
  fn test_parse_error_empty_attribute_value() {
    let mut input = "device[port=]";
    assert!(parse_predicate(&mut input).is_err());

    let mut input = r#"device[serial=""]"#; // Empty string should be OK
    let (predicate, _) = parse_predicate(&mut input).unwrap();
    assert_eq!(predicate.serial_number, Some("".to_string()));
  }

  #[test]
  fn test_parse_predicate_subject_selector() {
    let mut input = "!hub";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(is_subject);
    assert_eq!(predicate.node_type, Some(NodeType::Hub));

    let mut input = "! hub";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(is_subject);
    assert_eq!(predicate.node_type, Some(NodeType::Hub));

    let mut input = "!device";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(is_subject);
    assert_eq!(predicate.node_type, Some(NodeType::Device));

    let mut input = "!*";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(is_subject);
    assert_eq!(predicate.node_type, None);

    // Test subject selector with attributes
    let mut input = "!hub[port=3]";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(is_subject);
    assert_eq!(predicate.node_type, Some(NodeType::Hub));
    assert_eq!(predicate.port_number, Some(PortNumber(3)));

    // Test subject selector with whitespace and attributes
    let mut input = "! device[vid=1234]";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(is_subject);
    assert_eq!(predicate.node_type, Some(NodeType::Device));
    assert_eq!(predicate.vendor_id, Some(0x1234));
  }

  #[test]
  fn test_parse_predicate_whitespace_handling() {
    // Test various whitespace patterns with subject selectors
    let mut input = "  !  hub  ";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(is_subject);
    assert_eq!(predicate.node_type, Some(NodeType::Hub));

    // Test whitespace before and after !
    let mut input = "  !   device  ";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(is_subject);
    assert_eq!(predicate.node_type, Some(NodeType::Device));

    // Test normal predicate (no !) with whitespace
    let mut input = "  hub  ";
    let (predicate, is_subject) = parse_predicate(&mut input).unwrap();
    assert!(!is_subject);
    assert_eq!(predicate.node_type, Some(NodeType::Hub));
  }
}
