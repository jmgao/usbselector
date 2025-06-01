//! Selectors are comprised of a chain of predicates and joiners between them.
//! An example selector `hub [port=3] > device [manufacturer="Logitech"]` that looks for
//! a Logitech device connected to a hub that's connected to port 3 of its parent hub
//! is represented in code as follows:
//!
//! Selector {
//!   chain: vec![
//!     (Combinator::None, Predicate::new().with_node_type(NodeType::Hub).with_port_number(3)),
//!     (Combinator::Child, Predicate::new().with_node_type(NodeType::Device).with_manufacturer("Logitech".into()))
//!   ],
//! }

/// Find the matching closing paren for an opening paren at position 0.
/// Returns the index of the matching `)` or an error string if unmatched.
fn matching_close_paren(s: &str) -> Result<usize, String> {
  let mut depth = 0usize;
  let mut in_string = false;
  let mut escape = false;
  for (i, c) in s.chars().enumerate() {
    if escape {
      escape = false;
      continue;
    }
    if c == '\\' && in_string {
      escape = true;
      continue;
    }
    if c == '"' {
      in_string = !in_string;
      continue;
    }
    if in_string {
      continue;
    }
    if c == '(' {
      depth += 1;
    } else if c == ')' {
      depth -= 1;
      if depth == 0 {
        return Ok(i);
      }
    }
  }
  Err("unmatched parenthesis".into())
}

use std::collections::{BTreeMap, HashMap, HashSet};

use std::rc::Rc;
use winnow::Parser;

use crate::node::{Node, PortNumber};
use crate::predicate::Predicate;

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum Combinator {
  /// Matches all descendants of an element.
  /// Represented by whitespace in the textual format.
  Descendants,

  /// Matches only the elements that are direct children of a given element.
  ///
  /// Note that for hubs, one physical hub can be implemented with multiple hub ICs internally.
  /// For example, most 7-port hubs are actually two 4-port hubs, with 3 ports on the parent hub
  /// and 4 on the child.
  ///
  /// Represented by '>' in the textual format.
  Children,

  /// Matches the current element and all of its descendants.
  ///
  /// Represented by '%' in the textual format.
  CurrentAndDescendants,

  /// Matches the current element and all of its direct children.
  ///
  /// Represented by '%>' in the textual format.
  CurrentAndChildren,
}

/// Roots from which a frontier node is accessible.
pub(crate) type FrontierRoots<T> = HashSet<Rc<Node<T>>>;

/// Map from a `Node` on the frontier to the original nodes from which it's reachable.
pub(crate) type Frontier<T> = HashMap<Rc<Node<T>>, FrontierRoots<T>>;

#[derive(Debug, PartialEq)]
pub enum Expression {
  Predicate(Predicate),
  Selector(Box<Selector>),
}

impl PartialEq<Predicate> for Expression {
  fn eq(&self, other: &Predicate) -> bool {
    match self {
      Expression::Predicate(predicate) => predicate == other,
      Expression::Selector(_) => false,
    }
  }
}

impl PartialEq<Expression> for Predicate {
  fn eq(&self, other: &Expression) -> bool {
    PartialEq::<Predicate>::eq(other, self)
  }
}

#[derive(Debug, PartialEq)]
pub struct Selector {
  chain: Vec<(Combinator, Expression)>,
  subject_index: Option<usize>,
}

impl Selector {
  pub fn new(chain: Vec<(Combinator, Expression)>) -> Self {
    Selector {
      chain,
      subject_index: None,
    }
  }

  pub fn new_with_subject(chain: Vec<(Combinator, Expression)>, subject_index: Option<usize>) -> Self {
    Selector { chain, subject_index }
  }

  /// Returns a reference to the chain for testing purposes
  #[cfg(test)]
  pub fn chain(&self) -> &Vec<(Combinator, Expression)> {
    &self.chain
  }

  /// Returns the subject index for testing purposes
  #[cfg(test)]
  pub fn subject_index(&self) -> Option<usize> {
    self.subject_index
  }

  fn step<T>(filter: &(Combinator, Expression), frontier: Frontier<T>) -> Frontier<T> {
    let (combinator, expression) = filter;

    let include_self =
      *combinator == Combinator::CurrentAndChildren || *combinator == Combinator::CurrentAndDescendants;
    let include_descendants =
      *combinator == Combinator::Descendants || *combinator == Combinator::CurrentAndDescendants;

    let mut next_frontier = Frontier::new();
    for (node, roots) in frontier {
      Node::collect_children(&mut next_frontier, node, &roots, include_self, include_descendants);
    }

    match expression {
      Expression::Predicate(predicate) => {
        next_frontier.retain(|k, _| predicate.apply(k));
        next_frontier
      }
      Expression::Selector(selector) => {
        let sub_frontier = next_frontier
          .into_keys()
          .map(|node| (node.clone(), HashSet::new()))
          .collect();
        // Evaluate the sub-selector with its own subject handling, using the
        // subject_index to determine which nodes should be returned.
        Selector::select_from_frontier(&selector.chain, selector.subject_index, sub_frontier)
      }
    }
  }

  fn select_slice<T>(filters: &[(Combinator, Expression)], mut frontier: Frontier<T>) -> Frontier<T> {
    for filter in filters {
      frontier = Selector::step(filter, frontier);
    }
    frontier
  }

  /// Evaluate a selector chain from a given frontier, handling subject selection internally.
  /// This is used for sub-expressions that have their own `subject_index`.
  /// Returns a frontier where the keys are the SUBJECT nodes, so the outer chain
  /// can correctly continue from them.
  fn select_from_frontier<T>(
    chain: &[(Combinator, Expression)],
    subject_index: Option<usize>,
    mut frontier: Frontier<T>,
  ) -> Frontier<T> {
    if chain.is_empty() {
      return Frontier::new();
    }

    let subject = subject_index.unwrap_or(chain.len() - 1);
    let head = &chain[0..=subject];
    let tail = &chain[subject + 1..chain.len()];

    // Process the head to get subject nodes
    frontier = Selector::select_slice(head, frontier);

    if !tail.is_empty() {
      // There's a tail: restart from subject nodes and process the tail.
      // Each subject node becomes its own root for tracking.
      let subject_nodes: Vec<Rc<Node<T>>> = frontier.into_keys().collect();
      let frontier_with_roots = subject_nodes
        .iter()
        .map(|node| {
          let mut roots = HashSet::new();
          roots.insert(node.clone());
          (node.clone(), roots)
        })
        .collect();
      let tail_result = Selector::select_slice(tail, frontier_with_roots);
      // Filter subject nodes to only those that produced tail results.
      // The tail_result frontier has roots that are the subjects which matched.
      let matching_subjects: HashSet<Rc<Node<T>>> = tail_result.values().flatten().cloned().collect();
      frontier = subject_nodes
        .into_iter()
        .filter(|node| matching_subjects.contains(node))
        .map(|node| {
          let mut roots = HashSet::new();
          roots.insert(node.clone());
          (node, roots)
        })
        .collect();
    }
    // No tail: frontier already contains the subject nodes

    frontier
  }

  pub fn select<T>(&self, devices: &BTreeMap<PortNumber, Rc<Node<T>>>) -> HashSet<Rc<Node<T>>> {
    if self.chain.is_empty() {
      return HashSet::new();
    }

    let subject = self.subject_index.unwrap_or(self.chain.len() - 1);
    let head = &self.chain[0..=subject];
    let tail = &self.chain[subject + 1..self.chain.len()];

    let mut frontier = Frontier::new();
    frontier.extend(devices.values().map(|device| (device.clone(), HashSet::new())));
    frontier = Selector::select_slice(head, frontier);

    let mut result = HashSet::new();

    if !tail.is_empty() {
      frontier = frontier
        .into_keys()
        .map(|node| {
          let mut roots = HashSet::new();
          roots.insert(node.clone());
          (node.clone(), roots)
        })
        .collect();
      frontier = Selector::select_slice(tail, frontier);
      for (_, roots) in frontier {
        result.extend(roots);
      }
    } else {
      result.extend(frontier.keys().cloned());
    }

    result
  }

  pub fn parse(string: &str) -> Result<Self, String> {
    // Strip outer parentheses if they fully wrap the selector.
    // E.g., "(!hub > device)" should be parsed as a selector with chain [hub, device].
    // But "(!hub > device) > port" should parse the sub-expression as a chain element.
    let trimmed = string.trim();
    let parse_input = if trimmed.starts_with('(') {
      let closing = matching_close_paren(trimmed)?;
      if closing + 1 == trimmed.len() {
        &trimmed[1..closing]
      } else {
        trimmed
      }
    } else {
      trimmed
    };
    crate::parse_selector.parse(parse_input).map_err(|e| {
      let error = format!("{}", e);
      if error.is_empty() {
        "unknown error".into()
      } else {
        error
      }
    })
  }
}
