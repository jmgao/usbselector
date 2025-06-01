use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::fmt::{Display, Formatter, Write};
use std::hash::{Hash, Hasher};
use std::ptr::NonNull;
use std::rc::Rc;
use std::str::FromStr;

use crate::selector::{Frontier, FrontierRoots};

#[cfg(feature = "rusb")]
use rusb::{constants::LIBUSB_CLASS_HUB, ffi::libusb_device};

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct PortNumber(pub u8);

impl Display for PortNumber {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.0)
  }
}

pub type BusAddress = Vec<PortNumber>;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NodeType {
  Hub,
  Device,
}

impl FromStr for NodeType {
  type Err = ();
  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "hub" => Ok(NodeType::Hub),
      "device" => Ok(NodeType::Device),
      _ => Err(()),
    }
  }
}

#[derive(Clone)]
pub struct Node<T> {
  pub children: BTreeMap<PortNumber, Rc<Node<T>>>,

  pub node_type: NodeType,
  pub bus_address: BusAddress,
  pub vendor_id: u16,
  pub product_id: u16,
  pub serial_number: Option<String>,
  pub manufacturer: Option<String>,
  pub product: Option<String>,

  pub userdata: Option<T>,
}

impl<T> std::fmt::Debug for Node<T> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "{:?} ({:04x}:{:04x})",
      self.node_type, self.vendor_id, self.product_id,
    )
  }
}

impl<T> Eq for Node<T> {}
impl<T> PartialEq for Node<T> {
  fn eq(&self, other: &Self) -> bool {
    self.bus_address == other.bus_address
  }
}

impl<T> Hash for Node<T> {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.bus_address.hash(state)
  }
}

impl<T> Node<T> {
  pub fn collect_children(
    out: &mut Frontier<T>,
    node: Rc<Node<T>>,
    roots: &FrontierRoots<T>,
    include_self: bool,
    include_descendants: bool,
  ) {
    if include_self {
      out.entry(node.clone()).or_default().extend(roots.iter().cloned());
    }
    for child in node.children.values() {
      match out.entry(child.clone()) {
        Entry::Vacant(entry) => {
          entry.insert(roots.clone());
          if include_descendants {
            Node::collect_children(out, child.clone(), roots, false, true);
          }
        }
        Entry::Occupied(mut entry) => {
          entry.get_mut().extend(roots.iter().cloned());
        }
      }
    }
  }

  pub fn port_number(&self) -> PortNumber {
    if let Some(last_port) = self.bus_address.last() {
      *last_port
    } else {
      // Fallback to PortNumber(0) if bus_address is empty (shouldn't happen)
      PortNumber(0)
    }
  }

  pub fn format(&self, full_bus_address: bool) -> String {
    let mut result = String::new();
    let f = &mut result;
    write!(
      f,
      "[{}] ID {:04x}:{:04x}",
      if full_bus_address {
        self
          .bus_address
          .iter()
          .map(ToString::to_string)
          .collect::<Vec<_>>()
          .join(".")
      } else {
        self.port_number().to_string()
      },
      self.vendor_id,
      self.product_id
    )
    .unwrap();

    match (&self.manufacturer, &self.product) {
      (Some(manufacturer), Some(product)) => write!(f, " {} {}", manufacturer, product).unwrap(),
      (Some(manufacturer), None) => write!(f, " {}", manufacturer).unwrap(),
      (None, Some(product)) => write!(f, " {}", product).unwrap(),
      (None, None) => {}
    }

    if let Some(ref serial) = self.serial_number {
      write!(f, " (Serial: {})", serial).unwrap();
    }

    write!(f, " ({:?})", self.node_type).unwrap();
    result
  }

  pub fn dump(&self, depth: u32) {
    for i in 0..depth {
      print!("  ")
    }
    println!("{}", self);
    for (_, child) in self.children.iter() {
      child.dump(depth + 1);
    }
  }
}

impl<T> Display for Node<T> {
  fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
    write!(f, "{}", self.format(false))
  }
}

#[cfg(feature = "rusb")]
impl<T: rusb::UsbContext> Node<rusb::Device<T>> {
  fn new(
    device: rusb::Device<T>,
    bus_address: BusAddress,
    children: BTreeMap<PortNumber, Rc<Node<rusb::Device<T>>>>,
  ) -> rusb::Result<Self> {
    let descriptor = device.device_descriptor()?;
    let class_code = descriptor.class_code();
    let node_type = if class_code == LIBUSB_CLASS_HUB {
      NodeType::Hub
    } else {
      NodeType::Device
    };

    // Get basic device information from descriptor
    let vendor_id = descriptor.vendor_id();
    let product_id = descriptor.product_id();

    // Try to get string descriptors (these may fail if device can't be opened)
    let (manufacturer, product, serial_number) = if let Ok(handle) = device.open() {
      let manufacturer = handle.read_manufacturer_string_ascii(&descriptor).ok();
      let product = handle.read_product_string_ascii(&descriptor).ok();
      let serial_number = handle.read_serial_number_string_ascii(&descriptor).ok();
      (manufacturer, product, serial_number)
    } else {
      (None, None, None)
    };

    Ok(Node {
      node_type,
      children,
      bus_address,
      vendor_id,
      product_id,
      serial_number,
      manufacturer,
      product,
      userdata: Some(device),
    })
  }

  fn from_raw(
    context: T,
    raw: *mut libusb_device,
    bus_address: BusAddress,
    all_children: &HashMap<*mut libusb_device, BTreeMap<PortNumber, *mut libusb_device>>,
  ) -> rusb::Result<Node<rusb::Device<T>>> {
    let device = unsafe { rusb::Device::from_libusb(context.clone(), NonNull::new(raw).unwrap()) };

    let mut node_children = BTreeMap::new();
    if let Some(children) = all_children.get(&raw) {
      for (port, child) in children {
        let mut bus_address = bus_address.clone();
        bus_address.push(*port);
        let child_node = Node::from_raw(context.clone(), *child, bus_address, all_children)?;
        node_children.insert(*port, Rc::new(child_node));
      }
    };

    Node::new(device, bus_address, node_children)
  }

  pub fn from_devices(context: &T, it: rusb::Devices<T>) -> BTreeMap<PortNumber, Rc<Node<rusb::Device<T>>>> {
    let mut roots = BTreeMap::new();
    let mut children: HashMap<*mut libusb_device, BTreeMap<PortNumber, *mut libusb_device>> = HashMap::new();

    for device in it {
      let raw = device.as_raw();
      let port_number = PortNumber(device.port_number());
      match device.get_parent() {
        Some(parent) => {
          let parent_raw = parent.as_raw();
          if let Some(children) = children.get_mut(&parent_raw) {
            children.insert(port_number, raw);
          } else {
            let mut map = BTreeMap::new();
            map.insert(port_number, raw);
            children.insert(parent_raw, map);
          }
        }

        None => {
          roots.insert(PortNumber(device.bus_number()), device.as_raw());
        }
      };
    }

    let mut result = BTreeMap::new();
    for (port, root) in roots {
      match Node::from_raw(context.clone(), root, vec![port], &children) {
        Ok(node) => {
          result.insert(port, Rc::new(node));
        }
        Err(_) => {
          // Skip nodes that can't be created
        }
      }
    }
    result
  }
}
