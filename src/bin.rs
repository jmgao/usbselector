use rusb::UsbContext;
use usbselector::Node;
use usbselector::Selector;

fn main() {
  let args = std::env::args().collect::<Vec<_>>();
  let selector = if args.len() == 2 {
    match Selector::parse(&args[1]) {
      Ok(selector) => Some(selector),
      Err(err) => {
        println!("failed to parse selector:\n{}", err);
        std::process::exit(0);
      }
    }
  } else if args.len() == 1 {
    None
  } else {
    println!("usage: {} [SELECTOR]", args.first().unwrap_or(&"usbselector".into()));
    std::process::exit(1);
  };

  let context = rusb::Context::new().unwrap();
  let devices = context.devices().unwrap();

  let nodes = Node::from_devices(&context, devices.iter());

  if let Some(selector) = selector {
    let selected = selector.select(&nodes);

    // Sort by bus address
    let mut selected = selected.iter().collect::<Vec<_>>();
    selected.sort_by_key(|node| &node.bus_address);
    for node in selected {
      println!("{}", node.format(true));
    }
  } else {
    for (_port, node) in nodes {
      node.dump(0);
    }
  }
}
