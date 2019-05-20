# vm-device

The `vm-device` crate is the rust-vmm device model and manager. It provides
high level APIs for VMMs to register devices into a device topology,
manages the VMM address spaces and resources and handles IO VM exits.

## Objectives

A Virtual Machine Manager (VMM) needs to handle and sometimes emulate different
kind of devices, from legacy PIO based ones to modern PCI MMIO endpoints.

The `vm-device` crate aims at providing top-level APIs for managing and tracking
devices on behalf of the VMM.

## Principles

- The main `vm-device` object is the `DeviceManager`. It provides high level
  APIs for the VMM to

  - Allocate a new `DeviceManager` instance.

  - Register and unregister devices.

  - Handle IO VM exits.

- Any registered device must implement the `Device` trait.

- Any device needs a set of resources, mostly IO ranges (PIO or MMIO)
  and interrupts.

- Except for physically backed MMIO operations, IO ranges access will trigger
  VM exits that will be handled through device specific callbacks by the device
  manager

- Both buses and devices are treated as device objects.

## Architecture

The `vm-device` crate is derived from two upstream projects:

- [crosvm](https://chromium.googlesource.com/chromiumos/platform/crosvm/)
  commit b1de6323

- [firecracker](https://firecracker-microvm.github.io/) commit 1fdde199

Both crosvm and firecracker have some kinds of high level management
for their devices. The rust-vmm `vm-device` crate implementation mostly
refactors those project implementations as follows:

### `DeviceManager`

The `DeviceManager` keeps track of all registered devices, with `Arc`
reference-counting pointer. Each registered device is responsible for
protecting shared data inside its own implementation. The registered
device is optionally linked to a parent bus and will typically register
a set of IO related resources and IRQ resource.
All devices are added to an internal hash map indexed by a unique instance id.

As the `DeviceManager` keeps track of devices relations between each others,
it provides an overall view of the platform device model.

By resolving adresses into their registered device, the `DeviceManager`
handles all IO related VM exits on behalf of the VMM.

Both buses and devices objects are implementation of the `Device` trait.

### `Device`

The `Device` trait is the top level device abstraction. Any registered device
must implement the `Device` trait interface:

- `name` should return a name string for this device.

- `read` and `write` are IO callbacks for the device related VM exits. They
   handle both PIO and MMIO exits.

- `set_resources` is being called by the `DeviceManager` to notify the device
  about the final resources that got allocated for it. Typically devices will
  ask for IO ranges and a set of interrupts. The `DeviceManager` will allocate
  those and eventually let the device know about them.

## Example

Let's create a `DeviceManager` and register a `Device` against it:

```Rust

// First we need to allocate a system allocator
let mut allocator = SystemAllocator::new(
    None,
    None,
    GuestAddress(0),
    1 << 36 as GuestUsize,
    X86_64_IRQ_BASE,
    X86_64_IRQ_END,
    INSTANCE_ID_BASE,
)
.ok_or(Error::CreateSystemAllocator)?;

let mut device_manager = DeviceManager::new(&mut sys_res);

/// DummyDevice returns config_address on a read and sets
/// config_address on a write. As dummy as it can get.
struct DummyDevice {
    config_address: Mutex<u32>,
}

impl Device for DummyDevice {
    fn name(&self) -> String {
        "dummy_device".to_string()
    }

    fn read(&self, addr: GuestAddress, data: &mut [u8], io_type: IoType) {
        if data.len() > 4 {
            for d in data {
                *d = 0xff;
            }
            return;
        }

        for i in 0..data.len() {
            let config = self.config_address.lock().expect("failed to acquire lock");
            *iter = (*config >> (idx * 8) & 0xff) as u8;
        }
    }

    fn write(&self, addr: GuestAddress, data: &[u8], io_type: IoType) {
        let mut config = self.config_address.lock().expect("failed to acquire lock");
        *config = data[0] as u32 & 0xff;
    }

    fn set_resources(&self, _res: &[IoResource], _irq: Option<IrqResource>) {}
}

/// Now we can register a DummyDevice against the DeviceManager

/// First we need a PIO resource for it.
let mut resources_vec = Vec::new();
let res = IoResource::new(Some(GuestAddress(0xcf8)), 8 as GuestUsize, IoType::Pio);
resources.push(res);

/// Register with the request of IO resource and IRQ resource.
let dummy = DummyDevice{config_address: 0x1000,};
device_manager.register_device(Arc::new(dummy), None, &mut resources, Some(IrqResource(None)));
```

The VMM will then call the `DeviceManager` instance to handle VM exits:

```Rust
struct Vmm {
    fd: VcpuFd,
    devices: DeviceManager,
}

/// Create a vCPU fd
[...]

/// Create a Vmm
let vmm = Vmm{
    fd: vcpu_fd,
    devices: device_manager,
};

/// Run the vCPU and only handle PIO exits
match vmm.fd.run() {
    Ok(run) => match run {
        VcpuExit::IoIn(addr, data) => {
            vmm.devices.read(GuestAddress(u64::from(addr)), data, IoType::Pio);
            continue;
        }
        VcpuExit::IoOut(addr, data) => {
            vmm.devices.write(GuestAddress(u64::from(addr)), data, IoType::Pio);
            continue;
        }
    }
    Err(_) => {libc::_exit(0);}
}
```
