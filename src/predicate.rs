use crate::{
  PortNumber,
  node::{Node, NodeType},
};

#[derive(Default, Clone, Eq, PartialEq)]
pub struct Predicate {
  pub node_type: Option<NodeType>,
  pub port_number: Option<PortNumber>,
  pub vendor_id: Option<u16>,
  pub product_id: Option<u16>,
  pub serial_number: Option<String>,
  pub manufacturer: Option<String>,
  pub product: Option<String>,
}

impl std::fmt::Debug for Predicate {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let mut fields = Vec::new();
    if let Some(node_type) = self.node_type {
      fields.push(format!("{:?}", node_type));
    }
    if let Some(port_number) = self.port_number {
      fields.push(format!("port={}", port_number.0));
    }
    if let Some(vendor_id) = self.vendor_id {
      fields.push(format!("vid={}", vendor_id));
    }
    if let Some(product_id) = self.product_id {
      fields.push(format!("pid={}", product_id));
    }
    if let Some(ref serial_number) = self.serial_number {
      fields.push(format!("serial={}", serial_number));
    }
    if let Some(ref manufacturer) = self.manufacturer {
      fields.push(format!("manufacturer={}", manufacturer));
    }
    if let Some(ref product) = self.product {
      fields.push(format!("product={}", product));
    }
    write!(f, "Predicate({})", fields.join(", "))
  }
}

impl Predicate {
  pub fn new() -> Self {
    Predicate {
      node_type: None,
      port_number: None,
      vendor_id: None,
      product_id: None,
      serial_number: None,
      manufacturer: None,
      product: None,
    }
  }

  pub fn with_node_type(mut self, node_type: NodeType) -> Self {
    self.node_type = Some(node_type);
    self
  }

  pub fn with_port_number(mut self, port_number: u8) -> Self {
    self.port_number = Some(PortNumber(port_number));
    self
  }

  pub fn with_vendor_id(mut self, vendor_id: u16) -> Self {
    self.vendor_id = Some(vendor_id);
    self
  }

  pub fn with_product_id(mut self, product_id: u16) -> Self {
    self.product_id = Some(product_id);
    self
  }

  pub fn with_serial_number(mut self, serial_number: String) -> Self {
    self.serial_number = Some(serial_number);
    self
  }

  pub fn with_manufacturer(mut self, manufacturer: String) -> Self {
    self.manufacturer = Some(manufacturer);
    self
  }

  pub fn with_product(mut self, product: String) -> Self {
    self.product = Some(product);
    self
  }

  pub fn with_id(mut self, vendor_id: u16, product_id: u16) -> Self {
    self.vendor_id = Some(vendor_id);
    self.product_id = Some(product_id);
    self
  }

  pub fn apply<T>(&self, node: &Node<T>) -> bool {
    macro_rules! check_filter_impl {
      ($field: ident, $expected: expr) => {
        if self.$field.is_some() && self.$field != $expected {
          return false;
        }
      };
    }
    macro_rules! check_filter {
      ($field: ident) => {
        check_filter_impl!($field, Some(node.$field));
      };
    }
    macro_rules! check_filter_opt {
      ($field: ident) => {
        check_filter_impl!($field, node.$field);
      };
    }

    check_filter!(node_type);
    check_filter_impl!(port_number, Some(node.port_number()));
    check_filter!(vendor_id);
    check_filter!(product_id);
    check_filter_opt!(serial_number);
    check_filter_opt!(manufacturer);
    check_filter_opt!(product);

    true
  }
}
