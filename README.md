# USB Selectors

A Rust library for selecting USB devices using CSS-like selectors.

## Overview

This library allows you to query USB devices using CSS-style selectors, making it easier to find specific devices based on relationships in the USB tree. It supports proposed (but not adopted) syntax in CSS4 for selecting a device that isn't the final element of a compound selector along with parenthesized subexpression support. This lets you do things like select all devices on port 2 of hubs with a Logitech device: `(!hub > device[manufacturer="Logitech"]) > device[port=2]`.

## Selector Syntax

### Node Types

- `hub` - USB hubs
- `device` - USB devices (non-hub)
- `*` - Any node type (wildcard)

### Attributes

- `port=N` - Port number (0-255)
- `vid=XXXX` - Vendor ID (hex, with or without 0x prefix)
- `pid=XXXX` - Product ID (hex, with or without 0x prefix)
- `id=XXXX:XXXX` - Vendor + Product ID (hex, with or without 0x prefix)
- `manufacturer="string"` - Manufacturer string
- `product="string"` - Product string
- `serial="string"` - Serial number string

(Note that string values must be quoted with double quotes, but escaping for quotes in strings is currently unimplemented.)

### Combinators

- `>` - Direct child relationship
- ` ` (space) - Descendant relationship
- `%>` - Current element and direct children
- `%` - Current element and all descendants

### Subject Selectors

Subject selectors allow you to select ancestor elements based on their descendants by prefixing them with `!`.

```rust
// Regular selector - returns the device
"hub > device[vid=046d]"

// Subject selector - returns the hub that contains the device
"!hub > device[vid=046d]"
```

### Examples

```rust
// Find all Logitech devices
device[vid=046d]

// Find devices on port 3 of any hub
hub[port=3] > device

// Find any descendant devices of hub on port 1
hub[port=1] device

// Find devices with specific vendor and product ID
device[vid=0x1234, pid=0x5678]

// Find devices by manufacturer
device[manufacturer="Logitech"]

// Find hubs that directly contain Logitech devices
!hub > device[vid=046d]

// Find any hub that has a Corsair device as a descendant
!hub device[vid=1b1c]

// Find intermediate hubs in a chain
hub > !hub > device

// Find hubs with any USB device connected
!hub > device

// Find the specific hub containing a device with vendor/product ID
!hub > device[id=1235:5678]
```
