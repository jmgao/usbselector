use super::predicate::parse_predicate;

use winnow::{
  self, ModalResult, Parser,
  ascii::{space0, space1},
  combinator::{alt, opt, preceded, terminated},
  error::{ContextError, StrContext},
};

// `terminated` is used in `parse_element_body` for sub-expression parsing.

use crate::{Combinator, Predicate, Selector, selector::Expression};

fn parse_combinator(input: &mut &str) -> ModalResult<Combinator> {
  alt((
    preceded(space0, ">").value(Combinator::Children),
    preceded(space0, "%>").value(Combinator::CurrentAndChildren),
    preceded(space0, "%").value(Combinator::CurrentAndDescendants),
    space1.value(Combinator::Descendants),
  ))
  .parse_next(input)
}

/// Body after a combinator: either a parenthesized sub-selector or a bare predicate.
enum ElementBody {
  Predicate(Predicate, bool),
  Selector(Selector),
}

/// Parses the first chain element: either a parenthesized sub-selector or a bare predicate.
/// Returns the combinator (always CurrentAndDescendants for the first element) and
/// the expression along with the is_subject flag.
fn parse_first_chain_element(input: &mut &str) -> ModalResult<(Combinator, Expression, bool)> {
  space0.parse_next(input)?;
  if input.starts_with('(') {
    '('.parse_next(input)?;
    space0.parse_next(input)?;
    let sub_selector = terminated(parse_selector, space0).parse_next(input)?;
    ')'.parse_next(input)?;
    Ok((
      Combinator::CurrentAndDescendants,
      Expression::Selector(Box::new(sub_selector)),
      false,
    ))
  } else {
    let (predicate, is_subject) = preceded(space0, parse_predicate).parse_next(input)?;
    Ok((
      Combinator::CurrentAndDescendants,
      Expression::Predicate(predicate),
      is_subject,
    ))
  }
}

pub(crate) fn parse_selector(input: &mut &str) -> ModalResult<Selector> {
  space0.parse_next(input)?;

  // Parse the first chain element (no combinator before it).
  // This can be either a bare predicate or a parenthesized sub-selector.
  let (first_combinator, first_element, first_is_subject) = parse_first_chain_element(input)?;
  let mut chain = vec![(first_combinator, first_element)];
  let mut subject_index = if first_is_subject { Some(0) } else { None };

  // Try to parse: combinator followed by either a sub-selector or a predicate.
  // We use a single opt combinator so that if no combinator is present at all,
  // nothing is consumed and the loop exits cleanly.
  while let Some((combinator, body)) = opt((parse_combinator, parse_element_body)).parse_next(input)? {
    match body {
      ElementBody::Predicate(predicate, is_subject) => {
        if is_subject {
          if subject_index.is_some() {
            let mut err = ContextError::new();
            err.push(StrContext::Label("duplicate subject selectors"));
            return Err(winnow::error::ErrMode::Cut(err));
          }
          subject_index = Some(chain.len());
        }
        chain.push((combinator, Expression::Predicate(predicate)));
      }
      ElementBody::Selector(sub_selector) => {
        chain.push((combinator, Expression::Selector(Box::new(sub_selector))));
      }
    }
  }
  space0.parse_next(input)?;

  // Ensure all input is consumed - reject any remaining unparsed content.
  // Allow ')' as remaining input when parsing inside parentheses (sub-expressions),
  // so the closing paren can be consumed by the caller.
  if !input.is_empty() && !input.starts_with(')') {
    let mut err = ContextError::new();
    err.push(StrContext::Label("unexpected remaining input"));
    return Err(winnow::error::ErrMode::Cut(err));
  }

  Ok(Selector::new_with_subject(chain, subject_index))
}

/// Parse the body after a combinator: either a parenthesized sub-selector or a bare predicate.
fn parse_element_body(input: &mut &str) -> ModalResult<ElementBody> {
  space0.parse_next(input)?;

  if input.starts_with('(') {
    // Sub-expression: parse it as a Selector.
    // The sub-selector handles its own subject_index internally.
    '('.parse_next(input)?;
    space0.parse_next(input)?;
    let sub_selector = terminated(parse_selector, space0).parse_next(input)?;
    ')'.parse_next(input)?;
    Ok(ElementBody::Selector(sub_selector))
  } else {
    // Regular predicate with optional subject flag
    let (predicate, is_subject) = preceded(space0, parse_predicate).parse_next(input)?;
    Ok(ElementBody::Predicate(predicate, is_subject))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{Combinator, NodeType, Predicate};

  #[test]
  fn test_parse_combinator_children() {
    let mut input = ">";
    assert_eq!(parse_combinator(&mut input).unwrap(), Combinator::Children);

    let mut input = " > ";
    assert_eq!(parse_combinator(&mut input).unwrap(), Combinator::Children);
  }

  #[test]
  fn test_parse_combinator_descendants() {
    let mut input = " ";
    assert_eq!(parse_combinator(&mut input).unwrap(), Combinator::Descendants);

    let mut input = "  ";
    assert_eq!(parse_combinator(&mut input).unwrap(), Combinator::Descendants);
  }

  #[test]
  fn test_parse_combinator_current_and_descendants() {
    let mut input = "%";
    assert_eq!(parse_combinator(&mut input).unwrap(), Combinator::CurrentAndDescendants);

    let mut input = " % ";
    assert_eq!(parse_combinator(&mut input).unwrap(), Combinator::CurrentAndDescendants);
  }

  #[test]
  fn test_parse_combinator_current_and_children() {
    let mut input = "%>";
    assert_eq!(parse_combinator(&mut input).unwrap(), Combinator::CurrentAndChildren);

    let mut input = " %> ";
    assert_eq!(parse_combinator(&mut input).unwrap(), Combinator::CurrentAndChildren);
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
  fn test_parse_selector_leading_trailing_whitespace() {
    let mut input = "   hub[port=1]   ";
    let selector = parse_selector(&mut input).unwrap();
    assert_eq!(selector.chain().len(), 1);
    assert_eq!(
      selector.chain()[0].1,
      Predicate::new().with_node_type(NodeType::Hub).with_port_number(1)
    );
  }

  #[test]
  fn test_parse_selector_simple() {
    let mut input = "hub";
    let selector = parse_selector(&mut input).unwrap();
    assert_eq!(selector.chain().len(), 1);
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(selector.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));
  }

  #[test]
  fn test_parse_selector_with_attributes() {
    let mut input = "hub[port=3]";
    let selector = parse_selector(&mut input).unwrap();
    assert_eq!(selector.chain().len(), 1);
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(
      selector.chain()[0].1,
      Predicate::new().with_node_type(NodeType::Hub).with_port_number(3)
    );
  }

  #[test]
  fn test_parse_selector_with_children() {
    let mut input = "hub > device";
    let selector = parse_selector(&mut input).unwrap();
    assert_eq!(selector.chain().len(), 2);
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(selector.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));
    assert_eq!(selector.chain()[1].0, Combinator::Children);
    assert_eq!(selector.chain()[1].1, Predicate::new().with_node_type(NodeType::Device));
  }

  #[test]
  fn test_parse_selector_with_descendants() {
    let mut input = "hub device";
    let selector = parse_selector(&mut input).unwrap();
    assert_eq!(selector.chain().len(), 2);
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(selector.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));
    assert_eq!(selector.chain()[1].0, Combinator::Descendants);
    assert_eq!(selector.chain()[1].1, Predicate::new().with_node_type(NodeType::Device));
  }

  #[test]
  fn test_parse_selector_with_current_and_descendants() {
    let mut input = "hub%device";
    let selector = parse_selector(&mut input).unwrap();
    assert_eq!(selector.chain().len(), 2);
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(selector.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));
    assert_eq!(selector.chain()[1].0, Combinator::CurrentAndDescendants);
    assert_eq!(selector.chain()[1].1, Predicate::new().with_node_type(NodeType::Device));
  }

  #[test]
  fn test_parse_selector_with_current_and_children() {
    let mut input = "hub%>device";
    let selector = parse_selector(&mut input).unwrap();
    assert_eq!(selector.chain().len(), 2);
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(selector.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));
    assert_eq!(selector.chain()[1].0, Combinator::CurrentAndChildren);
    assert_eq!(selector.chain()[1].1, Predicate::new().with_node_type(NodeType::Device));
  }

  #[test]
  fn test_parse_selector_mixed_combinators() {
    let mut input = "hub%>device > hub";
    let selector = parse_selector(&mut input).unwrap();
    assert_eq!(selector.chain().len(), 3);
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(selector.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));
    assert_eq!(selector.chain()[1].0, Combinator::CurrentAndChildren);
    assert_eq!(selector.chain()[1].1, Predicate::new().with_node_type(NodeType::Device));
    assert_eq!(selector.chain()[2].0, Combinator::Children);
    assert_eq!(selector.chain()[2].1, Predicate::new().with_node_type(NodeType::Hub));
  }

  #[test]
  fn test_parse_selector_complex() {
    let mut input = r#"hub[vid=1234]%>device[pid=5678]"#;
    let selector = parse_selector(&mut input).unwrap();
    assert_eq!(selector.chain().len(), 2);

    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(
      selector.chain()[0].1,
      Predicate::new().with_node_type(NodeType::Hub).with_vendor_id(0x1234)
    );

    assert_eq!(selector.chain()[1].0, Combinator::CurrentAndChildren);
    assert_eq!(
      selector.chain()[1].1,
      Predicate::new()
        .with_node_type(NodeType::Device)
        .with_product_id(0x5678)
    );
  }

  #[test]
  fn test_parse_selector_with_whitespace() {
    let mut input = "  hub  [  port  =  3  ]  >  device  ";
    let selector = parse_selector(&mut input).unwrap();

    assert_eq!(selector.chain().len(), 2);
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(
      selector.chain()[0].1,
      Predicate::new().with_node_type(NodeType::Hub).with_port_number(3)
    );

    assert_eq!(selector.chain()[1].0, Combinator::Children);
    assert_eq!(selector.chain()[1].1, Predicate::new().with_node_type(NodeType::Device));
  }

  #[test]
  fn test_parse_selector_multiple_spaces_between_combinators() {
    let mut input = "hub   >   device";
    let selector = parse_selector(&mut input).unwrap();
    assert_eq!(selector.chain().len(), 2);
    assert_eq!(selector.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));
    assert_eq!(selector.chain()[1].0, Combinator::Children);
    assert_eq!(selector.chain()[1].1, Predicate::new().with_node_type(NodeType::Device));
  }

  #[test]
  fn test_parse_error_malformed_brackets() {
    let mut input = "device[port=3"; // Missing closing bracket
    let result = parse_selector(&mut input);
    assert!(result.is_err(), "Missing closing bracket should be rejected");

    let mut input = "deviceport=3]"; // Missing opening bracket
    let result = parse_selector(&mut input);
    assert!(result.is_err(), "Invalid node type with trailing ] should be rejected");
  }

  #[test]
  fn test_parse_error_invalid_combinator_sequence() {
    let mut input = "hub > > device"; // Double combinator
    let result = parse_selector(&mut input);
    assert!(result.is_err(), "Double combinators should be invalid");
  }

  #[test]
  fn test_parse_subject_selector_simple() {
    let selector = Selector::parse("!hub").unwrap();
    assert_eq!(selector.chain().len(), 1);
    assert_eq!(selector.subject_index(), Some(0));
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(selector.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));
  }

  #[test]
  fn test_parse_subject_selector_with_child() {
    let selector = Selector::parse("!hub > device[vid=0x1235,pid=0x5678]").unwrap();
    assert_eq!(selector.chain().len(), 2);
    assert_eq!(selector.subject_index(), Some(0));

    // First predicate: !hub
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(selector.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));

    // Second predicate: > device[vid=0x1235,pid=0x5678]
    assert_eq!(selector.chain()[1].0, Combinator::Children);
    assert_eq!(
      selector.chain()[1].1,
      Predicate::new()
        .with_node_type(NodeType::Device)
        .with_vendor_id(0x1235)
        .with_product_id(0x5678)
    );
  }

  #[test]
  fn test_parse_subject_selector_middle() {
    let selector = Selector::parse("hub > !device[vid=1234] > *[port=2]").unwrap();
    assert_eq!(selector.chain().len(), 3);
    assert_eq!(selector.subject_index(), Some(1));

    // First: hub
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(selector.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));

    // Second: > !device[vid=1234] (subject)
    assert_eq!(selector.chain()[1].0, Combinator::Children);
    assert_eq!(
      selector.chain()[1].1,
      Predicate::new().with_node_type(NodeType::Device).with_vendor_id(0x1234)
    );

    // Third: > *[port=2]
    assert_eq!(selector.chain()[2].0, Combinator::Children);
    assert_eq!(selector.chain()[2].1, Predicate::new().with_port_number(2));
  }

  #[test]
  fn test_parse_subject_selector_last() {
    let selector = Selector::parse("hub > !device[vid=1234]").unwrap();
    assert_eq!(selector.chain().len(), 2);
    assert_eq!(selector.subject_index(), Some(1));

    // First: hub
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(selector.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));

    // Second: > !device[vid=1234] (subject)
    assert_eq!(selector.chain()[1].0, Combinator::Children);
    assert_eq!(
      selector.chain()[1].1,
      Predicate::new().with_node_type(NodeType::Device).with_vendor_id(0x1234)
    );
  }

  #[test]
  fn test_parse_no_subject_selector() {
    let selector = Selector::parse("hub > device").unwrap();
    assert_eq!(selector.chain().len(), 2);
    assert_eq!(selector.subject_index(), None);
  }

  #[test]
  fn test_parse_subject_selector_with_wildcard() {
    let selector = Selector::parse("!*[port=3] > device").unwrap();
    assert_eq!(selector.chain().len(), 2);
    assert_eq!(selector.subject_index(), Some(0));
    assert_eq!(selector.chain()[0].1, Predicate::new().with_port_number(3));
  }

  #[test]
  fn test_parse_multiple_subject_selectors_error() {
    let result = Selector::parse("!hub > !device");
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.contains("duplicate subject selectors"));
  }

  #[test]
  fn test_parse_subject_selector_with_attributes() {
    let selector = Selector::parse(r#"!hub[port=1, manufacturer="Internal"] > device[vid=046d]"#).unwrap();
    assert_eq!(selector.chain().len(), 2);
    assert_eq!(selector.subject_index(), Some(0));

    // Subject: !hub[port=1, manufacturer="Internal"]
    assert_eq!(
      selector.chain()[0].1,
      Predicate::new()
        .with_node_type(NodeType::Hub)
        .with_port_number(1)
        .with_manufacturer("Internal".to_string())
    );

    // Child: > device[vid=046d]
    assert_eq!(
      selector.chain()[1].1,
      Predicate::new().with_node_type(NodeType::Device).with_vendor_id(0x046d)
    );
  }

  #[test]
  fn test_parse_subject_selector_complex() {
    let selector = Selector::parse(r#"!hub[port=1] device[vid=1234] > device[product="Mouse"]"#).unwrap();
    assert_eq!(selector.chain().len(), 3);
    assert_eq!(selector.subject_index(), Some(0));

    // Subject: !hub[port=1]
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(
      selector.chain()[0].1,
      Predicate::new().with_node_type(NodeType::Hub).with_port_number(1)
    );

    // Descendant: device[vid=1234]
    assert_eq!(selector.chain()[1].0, Combinator::Descendants);
    assert_eq!(
      selector.chain()[1].1,
      Predicate::new().with_node_type(NodeType::Device).with_vendor_id(0x1234)
    );

    // Child: > device[product="Mouse"]
    assert_eq!(selector.chain()[2].0, Combinator::Children);
    assert_eq!(
      selector.chain()[2].1,
      Predicate::new()
        .with_node_type(NodeType::Device)
        .with_product("Mouse".to_string())
    );
  }

  #[test]
  fn test_subject_selector_with_all_combinator_types() {
    // Test subject selector with descendants combinator
    let selector = Selector::parse("!hub device").unwrap();
    assert_eq!(selector.subject_index(), Some(0));
    assert_eq!(selector.chain()[1].0, Combinator::Descendants);

    // Test subject selector with children combinator
    let selector = Selector::parse("!hub > device").unwrap();
    assert_eq!(selector.subject_index(), Some(0));
    assert_eq!(selector.chain()[1].0, Combinator::Children);
  }

  #[test]
  fn test_parse_subject_selector_whitespace_handling() {
    // Test subject selector with various whitespace patterns
    let selector = Selector::parse("  !hub  >  device  ").unwrap();
    assert_eq!(selector.subject_index(), Some(0));
    assert_eq!(selector.chain().len(), 2);

    let selector = Selector::parse("!hub  >  device").unwrap();
    assert_eq!(selector.subject_index(), Some(0));

    let selector = Selector::parse("hub  >  !device").unwrap();
    assert_eq!(selector.subject_index(), Some(1));

    // Test whitespace between ! and node type
    let selector = Selector::parse("!  hub > device").unwrap();
    assert_eq!(selector.subject_index(), Some(0));
    assert_eq!(selector.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));

    let selector = Selector::parse("hub > ! device").unwrap();
    assert_eq!(selector.subject_index(), Some(1));
    assert_eq!(selector.chain()[1].1, Predicate::new().with_node_type(NodeType::Device));

    // Test multiple spaces between ! and node type
    let selector = Selector::parse("!   hub").unwrap();
    assert_eq!(selector.subject_index(), Some(0));
    assert_eq!(selector.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));
  }

  #[test]
  fn test_subject_selector_mixed_with_wildcards() {
    let selector = Selector::parse("!* > device[vid=1234]").unwrap();
    assert_eq!(selector.subject_index(), Some(0));
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(selector.chain()[0].1, Predicate::new());
    assert_eq!(selector.chain()[1].0, Combinator::Children);
    assert_eq!(
      selector.chain()[1].1,
      Predicate::new().with_node_type(NodeType::Device).with_vendor_id(0x1234)
    );

    let selector = Selector::parse("hub > !*[port=3]").unwrap();
    assert_eq!(selector.subject_index(), Some(1));
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(selector.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));
    assert_eq!(selector.chain()[1].0, Combinator::Children);
    assert_eq!(selector.chain()[1].1, Predicate::new().with_port_number(3));
  }

  #[test]
  fn test_subject_selector_error_multiple_subjects() {
    // Test that multiple subject selectors are properly rejected
    let result = Selector::parse("!hub > !device");
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.contains("duplicate subject selectors"));

    let result = Selector::parse("!hub device !*");
    assert!(result.is_err());

    let result = Selector::parse("!* > !hub > device");
    assert!(result.is_err());
  }

  #[test]
  fn test_subject_selector_single_element() {
    // Test subject selector with only one element
    let selector = Selector::parse("!hub").unwrap();
    assert_eq!(selector.subject_index(), Some(0));
    assert_eq!(selector.chain().len(), 1);

    let selector = Selector::parse("!device[vid=1234]").unwrap();
    assert_eq!(selector.subject_index(), Some(0));
    assert_eq!(selector.chain().len(), 1);
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(
      selector.chain()[0].1,
      Predicate::new().with_node_type(NodeType::Device).with_vendor_id(0x1234)
    );
  }

  #[test]
  fn test_subject_selector_position_independence() {
    // Test that subject can be at any position in the chain
    let selector = Selector::parse("!hub > device > *").unwrap();
    assert_eq!(selector.subject_index(), Some(0));

    let selector = Selector::parse("hub > !device > *").unwrap();
    assert_eq!(selector.subject_index(), Some(1));

    let selector = Selector::parse("hub > device > !*").unwrap();
    assert_eq!(selector.subject_index(), Some(2));
  }

  #[test]
  fn test_subject_selector_with_complex_attributes() {
    let selector = Selector::parse(r#"!device[vid=0x1234, pid=0xabcd, manufacturer="Test"] > hub"#).unwrap();
    assert_eq!(selector.subject_index(), Some(0));
    assert_eq!(
      selector.chain()[0].1,
      Predicate::new()
        .with_node_type(NodeType::Device)
        .with_vendor_id(0x1234)
        .with_product_id(0xabcd)
        .with_manufacturer("Test".to_string())
    );
  }

  #[test]
  fn test_subject_selector_functional_behavior() {
    // Test that subject_index is properly set and handled in parsing
    let selector_without_subject = Selector::parse("hub > device").unwrap();
    assert_eq!(selector_without_subject.subject_index(), None);

    let selector_with_subject = Selector::parse("!hub > device").unwrap();
    assert_eq!(selector_with_subject.subject_index(), Some(0));

    // Verify chain structure is preserved
    assert_eq!(selector_without_subject.chain().len(), 2);
    assert_eq!(selector_with_subject.chain().len(), 2);

    // Verify the actual predicate content is the same
    assert_eq!(
      selector_without_subject.chain()[0].1,
      selector_with_subject.chain()[0].1
    );
    assert_eq!(
      selector_without_subject.chain()[1].1,
      selector_with_subject.chain()[1].1
    );
  }

  #[test]
  fn test_subexpression_simple() {
    // Fully parenthesized selector is equivalent to the inner selector (parens are stripped).
    let selector = Selector::parse("(!hub > device[vid=18d1, pid=5026])").unwrap();
    assert_eq!(selector.chain().len(), 2);
    assert_eq!(selector.subject_index(), Some(0));
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(selector.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));
    assert_eq!(selector.chain()[1].0, Combinator::Children);
    assert_eq!(
      selector.chain()[1].1,
      Predicate::new()
        .with_node_type(NodeType::Device)
        .with_vendor_id(0x18d1)
        .with_product_id(0x5026)
    );
  }

  #[test]
  fn test_debug_subexpression() {
    // Debug: print the structure of the problematic selector
    let selector =
      Selector::parse("(!hub[vid=2109] > hub[vid=2109]) > hub[vid=2109] > device[vid=18d1,pid=5026]").unwrap();
    eprintln!("selector.chain().len() = {}", selector.chain().len());
    eprintln!("selector.subject_index() = {:?}", selector.subject_index());
    for (i, (comb, expr)) in selector.chain().iter().enumerate() {
      eprintln!("  chain[{}]: comb={:?}", i, comb);
      match expr {
        Expression::Predicate(p) => {
          eprintln!("    Predicate: {:?}", p);
        }
        Expression::Selector(s) => {
          eprintln!("    Selector (subject_index={:?}):", s.subject_index());
          for (j, (comb2, expr2)) in s.chain().iter().enumerate() {
            eprintln!("      sub_chain[{}]: comb={:?}", j, comb2);
            match expr2 {
              Expression::Predicate(p) => {
                eprintln!("        Predicate: {:?}", p);
              }
              _ => {}
            }
          }
        }
      }
    }
    // The selector has 3 chain elements (outer parens don't fully wrap):
    // chain[0]: (CurrentAndDescendants, Selector(sub_sel))
    // chain[1]: (Children, Predicate(hub[vid=2109]))
    // chain[2]: (Children, Predicate(device[vid=18d1,pid=5026]))
    // No explicit subject in outer chain, so defaults to last element
    assert_eq!(selector.chain().len(), 3);
    assert_eq!(selector.subject_index(), None);
  }

  #[test]
  fn test_subexpression_with_outer_selector() {
    // The main example from the spec:
    // (!hub > device[vid=18d1, pid=5026]) > device[port=2]
    let selector = Selector::parse("(!hub > device[vid=18d1, pid=5026]) > device[port=2]").unwrap();
    assert_eq!(selector.chain().len(), 2);
    // No explicit subject in outer chain, defaults to last element
    assert_eq!(selector.subject_index(), None);

    // First chain element: sub-selector expression
    match &selector.chain()[0].1 {
      Expression::Selector(sub_sel) => {
        assert_eq!(sub_sel.chain().len(), 2);
        // Inner subject is Some(0) from the !hub inside the sub-expression
        assert_eq!(sub_sel.subject_index(), Some(0));
        assert_eq!(sub_sel.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));
        assert_eq!(
          sub_sel.chain()[1].1,
          Predicate::new()
            .with_node_type(NodeType::Device)
            .with_vendor_id(0x18d1)
            .with_product_id(0x5026)
        );
      }
      Expression::Predicate(_) => panic!("expected sub-selector in chain[0]"),
    }
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);

    // Second chain element: device[port=2]
    assert_eq!(selector.chain()[1].0, Combinator::Children);
    assert_eq!(
      selector.chain()[1].1,
      Predicate::new().with_node_type(NodeType::Device).with_port_number(2)
    );
  }

  #[test]
  fn test_subexpression_nested() {
    // Double-nested: ((a > b) > c) > d
    let selector = Selector::parse("((hub > device) > device) > device").unwrap();
    assert_eq!(selector.chain().len(), 2);

    // Outer chain[0]: sub-selector ((hub > device) > device)
    match &selector.chain()[0].1 {
      Expression::Selector(sub_sel) => {
        assert_eq!(sub_sel.chain().len(), 2);
        assert_eq!(sub_sel.chain()[0].0, Combinator::CurrentAndDescendants);
        // Inner sub-selector (hub > device)
        match &sub_sel.chain()[0].1 {
          Expression::Selector(inner) => {
            assert_eq!(inner.chain().len(), 2);
            assert_eq!(inner.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));
            assert_eq!(inner.chain()[1].1, Predicate::new().with_node_type(NodeType::Device));
          }
          _ => panic!("expected nested sub-selector"),
        }
        assert_eq!(sub_sel.chain()[1].0, Combinator::Children);
        assert_eq!(sub_sel.chain()[1].1, Predicate::new().with_node_type(NodeType::Device));
      }
      _ => panic!("expected sub-selector"),
    }
    assert_eq!(selector.chain()[1].0, Combinator::Children);
    assert_eq!(selector.chain()[1].1, Predicate::new().with_node_type(NodeType::Device));
  }

  #[test]
  fn test_subexpression_subject_isolation() {
    // A sub-expression can have its own subject, independent of the outer chain.
    // The outer `!` on the sub-chain does not conflict with inner subjects.
    // (!hub > device) > !*[port=1]
    // Outer subject is index 1 (*[port=1]), inner subject is index 0 (!hub).
    // The inner !hub is scoped to the sub-expression.
    let selector = Selector::parse("(!hub > device) > !*[port=1]").unwrap();
    assert_eq!(selector.chain().len(), 2);
    // Outer subject is index 1 (*[port=1])
    assert_eq!(selector.subject_index(), Some(1));

    // The sub-expression itself has its own subject_index (from !hub)
    match &selector.chain()[0].1 {
      Expression::Selector(sub_sel) => {
        assert_eq!(sub_sel.subject_index(), Some(0));
        assert_eq!(sub_sel.chain().len(), 2);
      }
      _ => panic!("expected sub-selector"),
    }

    // Outer chain[1] is the !*[port=1]
    assert_eq!(selector.chain()[1].0, Combinator::Children);
    assert_eq!(selector.chain()[1].1, Predicate::new().with_port_number(1));
  }

  #[test]
  fn test_subexpression_with_all_combinators() {
    // Fully parenthesized sub-expression using different combinator types.
    // !hub device[vid=1234] > device
    // The parens are stripped since they fully wrap the content.
    let selector = Selector::parse("(!hub device[vid=1234] > device)").unwrap();
    assert_eq!(selector.chain().len(), 3);
    assert_eq!(selector.subject_index(), Some(0));
    assert_eq!(selector.chain()[0].0, Combinator::CurrentAndDescendants);
    assert_eq!(selector.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));
    assert_eq!(selector.chain()[1].0, Combinator::Descendants);
    assert_eq!(
      selector.chain()[1].1,
      Predicate::new().with_node_type(NodeType::Device).with_vendor_id(0x1234)
    );
    assert_eq!(selector.chain()[2].0, Combinator::Children);
    assert_eq!(selector.chain()[2].1, Predicate::new().with_node_type(NodeType::Device));
  }

  #[test]
  fn test_subexpression_whitespace() {
    // Whitespace handling inside sub-expressions
    let selector = Selector::parse("( ! hub  >  device[vid=1234] ) > device[port=1]").unwrap();
    assert_eq!(selector.chain().len(), 2);
    // No explicit outer subject
    assert_eq!(selector.subject_index(), None);

    match &selector.chain()[0].1 {
      Expression::Selector(sub_sel) => {
        assert_eq!(sub_sel.chain().len(), 2);
        // Inner subject from !hub
        assert_eq!(sub_sel.subject_index(), Some(0));
        assert_eq!(sub_sel.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));
        assert_eq!(
          sub_sel.chain()[1].1,
          Predicate::new().with_node_type(NodeType::Device).with_vendor_id(0x1234)
        );
      }
      _ => panic!("expected sub-selector"),
    }
    assert_eq!(selector.chain()[1].0, Combinator::Children);
    assert_eq!(
      selector.chain()[1].1,
      Predicate::new().with_node_type(NodeType::Device).with_port_number(1)
    );
  }

  #[test]
  fn test_subexpression_deeply_nested() {
    // Three levels of nesting: (((!hub) > device) > device) > device
    // Structure:
    //   chain[0] = (CurrentAndDescendants, Selector for "((!hub) > device) > device")
    //     → chain[0] = (CurrentAndDescendants, Selector for "(!hub) > device")
    //       → chain[0] = (CurrentAndDescendants, Selector for "(!hub)") — chain len 1, subject Some(0)
    //       → chain[1] = (Children, Predicate(device))
    //     → chain[1] = (Children, Predicate(device))
    //   chain[1] = (Children, Predicate(device))
    let selector = Selector::parse("(((!hub) > device) > device) > device").unwrap();
    assert_eq!(selector.chain().len(), 2);

    match &selector.chain()[0].1 {
      Expression::Selector(sub_sel) => {
        assert_eq!(sub_sel.chain().len(), 2);

        match &sub_sel.chain()[0].1 {
          Expression::Selector(inner) => {
            assert_eq!(inner.chain().len(), 2);
            // inner.chain()[0]: (!hub) as a sub-expression
            match &inner.chain()[0].1 {
              Expression::Selector(innermost) => {
                assert_eq!(innermost.chain().len(), 1);
                assert_eq!(innermost.subject_index(), Some(0));
                assert_eq!(innermost.chain()[0].1, Predicate::new().with_node_type(NodeType::Hub));
              }
              _ => panic!("expected innermost sub-selector"),
            }
            // inner.chain()[1]: > device
            assert_eq!(inner.chain()[1].0, Combinator::Children);
            assert_eq!(inner.chain()[1].1, Predicate::new().with_node_type(NodeType::Device));
          }
          _ => panic!("expected nested sub-selector"),
        }

        // sub_sel.chain()[1]: > device
        assert_eq!(sub_sel.chain()[1].0, Combinator::Children);
        assert_eq!(sub_sel.chain()[1].1, Predicate::new().with_node_type(NodeType::Device));
      }
      _ => panic!("expected sub-selector"),
    }

    // Outer chain[1]: > device
    assert_eq!(selector.chain()[1].0, Combinator::Children);
    assert_eq!(selector.chain()[1].1, Predicate::new().with_node_type(NodeType::Device));
  }
}
