# vm-device

The `vm-device` crate provides:
* device traits defining read and write operations on specialized buses
* device manager (bus-specific traits and a concrete implementation) for
operating devices and dispatching I/O
* abstractions for defining resources and their constraints (e.g. a specific bus
address range, IRQ number, etc)

## Design

### I/O

The virtual device model is built around four traits, `DevicePio` and
`MutDevicePio` for
[Programmed I/O](https://en.wikipedia.org/wiki/Programmed_input%E2%80%93output)
(PIO), and `DeviceMmio` and `MutDeviceMmio` for
[Memory-mapped I/O](https://en.wikipedia.org/wiki/Memory-mapped_I/O)
(MMIO). The traits define the same methods for handling read and write
operations. The difference is that `DevicePio` and `DeviceMmio` only require
immutable self borrows, whereas `MutDevicePio` and `MutDeviceMmio` require
mutable borrows.

The device manager abstraction is implemented by the `IoManager` struct. It
defines two buses, one for PIO and one for MMIO. For each bus, with the help of
the `PioManager` and `MmioManager` traits, the manager provides methods for
device registration, as well as for dispatching read and write requests.
The manager will determine which device is responsible for handling the I/O
request based on the accessed address range, and will route the request to that
device.

`PioManager` and `MmioManager` traits are useful as interfaces for a couple of
reasons. First, to allow for alternative implementations when the provided
`IoManager` is not sufficient. Second, to allow other crates depend on the
traits without depending on the actual implementation.

The interaction between a guest kernel, a host kernel and a VMM using
`IoManager` is depicted at the diagram below. A driver in the guest kernel
issues an I/O request. That request gets turned by the host kernel’s hypervisor
(KVM) into a trigger for the VMM to handle. The trigger is either a vCPU exit or
an eventfd notification. The VMM extracts address information and determines the
target bus. Then it dispatches a new request to the `IoManager`, which checks
that there’s a virtual device registered on the bus for the requested address,
and finally sends the request to that device.
![vm-device](https://user-images.githubusercontent.com/241037/143853115-b1526028-6836-4845-a311-71cf989c60ef.png)

### Interrupts

Interrupt configuration is built around the `Interrupt` and `InterruptSourceGroup`
traits. These traits allow device control code that is developed in separate
crates from the VMM (which typically implements the interrupt mechanisms) to 
configure interrupt delivery to the guest VM without having a dependency on the
implementation of the interrupt mechanism.
The `EdgeInterrupt` and `LevelInterrupt` are traits that provide interrupt
assertion mechanisms and can be used by device implementations directly in order
to inject interrupts in the guest VM.

The `Interrupt` trait provides methods that are used by devices to enable and
disable interrupt generation by registering the interrupt with the VMM. Access
to additional interrupt properties is defined in new super-traits.
`ConfigurableInterrupt` allows for devices to send or receive interrupt
configuration parameters to/from the implementation inside the VMM. This is useful
when devices need to specify custom data that the VMM will use when delivering the
interrupt (e.g. MSI device id, PCI INTx pin etc).
`MaskableInterrupt` is also defined as a super-trait for use with interrupts that
can be masked/unmasked.

An `InterruptSourceGroup` stores a collection of interrupts of the same type. It
is the interface through which a device may request or release interrupts and
perform group related actions like enabling or disabling all interrupts at once.
Each device that generates interrupts can be assigned one or more
`InterruptSourceGroup`s (depending on the types of interrupts it uses or logical
grouping). The following diagram depicts the interaction between the components
that use the interrupt interface:

![vm-device-interrupts](https://user-images.githubusercontent.com/86006646/148783015-fea49a7c-cff8-4ec7-8766-00b0baed41c5.png)

## Usage

### I/O
A device is usually attached to a particular bus and thus needs to implement a
trait of only one type. For example, serial port on x86 is a PIO device, while
VirtIO devices use MMIO. It’s also possible for a device to implement both. Once
the type of I/O is determined, the next step is to choose between mutable and
immutable trait variant. If read or write method needs to mutate the device’s
internal state, then the mutable variant must be used.

Before dispatching any I/O requests to the new device, it needs to be registered
with an `IoManager` instance within the specified address range on the bus.
Creating a new `IoManager` is easy by calling `IoManager::new()` without any
configuration. Internally the manager stores devices as trait objects wrapped
in `Arc`’s, therefore if the device implements `MutDevicePio` or
`MutDeviceMmio`, then it must be wrapped in a `Mutex`. The crate contains
automatic implementation of `DevicePio for Mutex<T> where T: MutDevicePio`
and `DeviceMmio for Mutex<T> where T: MutDeviceMmio` but only for the Mutex
type in the standard library. For any other `Mutex` type from 3rd party crates
the blanket implementation must be done by the user.

From now on the IoManager will be routing I/O requests for the registered
address range to the device. The requests are dispatched by the client code, for
example when handling VM exits, using `IoManager`'s methods `pio_read`,
`pio_write`, `mmio_read` and `mmio_write`.

### Interrupts

To allow configuration of interrupts, a VMM must implement the `Interrupt` and
`InterruptSourceGroup` traits for the interrupt mechanisms that the device
requires. Implementation is machine or VMM specific and may depend on the types
and number of IRQ chips that the machine has or interrupt delivery mechanisms
(e.g. `EventFd`s).
The device interrupt configuration code generally does not concern itself with
the actual implementation of the interrupts and will be initialized with one or
more `InterruptSourceGroup`s by the VMM.

In order to allow devices to inject interrupt lines in the guest, a VMM must
also implement either the `EdgeInterrupt` or `LevelInterrupt` traits, depending
on the type of interrupt assertion mechanism that is supported. Devices use
objects that have these traits to signal an interrupt to the guest.

The device configuration code may define constraints for the types of interrupts
that it needs by combining the supertraits defined into trait bounds (e.g. if it
needs a `ConfigurableInterrupt` that can receive the `LegacyIrqConfig`
configuration struct). MSI and legacy interrupt traits are added in this crate
for ease of use. Configuration of these interrupt types are standardised.
`MsiInterrupt` can also be used for MSI-X interrupts. These traits only define
the configuration bounds for these interrupt types. `EdgeInterrupt` or
`LevelInterrupt` will still need to be implemented in order to allow devices
to use these interrupts. MSI interrupts are considered edge-triggered while
`LegacyInterrupt` can be either edge-triggered (typically in the case of ISA
interrupts) or level triggered (in the case of INTx interrupts).

In order to have access to the underlying notification mechanisms used by the
hypervisor, the device configuration may use the `AsRefTriggerNotifier` and the 
`AsRefResampleNotifier` conversion traits and specify the `NotifierType` associated
type. This type defines the interrupt delivery mechanism and is specific to the
Hypervisor (e.g. KVM irqfd, Xen evtchn etc).

One example of this requirement is the development of a VFIO device. Since VFIO
can trigger a KVM irqfd directly, the VFIO device would need to get access to the
underlying irqfd in order to register it with VFIO.

## Examples

### Implementing a simple log PIO device

```rust
use std::sync::{Arc, Mutex};
use vm_device::bus::{PioAddress, PioAddressOffset, PioRange};
use vm_device::device_manager::{IoManager, PioManager};
use vm_device::MutDevicePio;

struct LogDevice {}

impl MutDevicePio for LogDevice {
    fn pio_read(&mut self, base: PioAddress, offset: PioAddressOffset, _data: &mut [u8]) {
        println!("mut pio_read: base {:?}, offset {}", base, offset);
    }

    fn pio_write(&mut self, base: PioAddress, offset: PioAddressOffset, data: &[u8]) {
        println!(
            "mut pio_write: base {:?}, offset {}, data {:?}",
            base, offset, data
        );
    }
}
```

### Registering the device with IoManager and performing I/O

```rust
fn main() {
    let mut manager = IoManager::new();
    let device = LogDevice {};
    let bus_range = PioRange::new(PioAddress(0), 10).unwrap();
    manager
        .register_pio(bus_range, Arc::new(Mutex::new(device)))
        .unwrap();
    manager.pio_write(PioAddress(0), &vec![b'o', b'k']).unwrap();
}
```

## Testing

The `vm-device` is tested using unit tests and integration tests.
It leverages [`rust-vmm-ci`](https://github.com/rust-vmm/rust-vmm-ci)
for continuous testing. All tests are ran in the `rustvmm/dev` container.

## License

This project is licensed under either of:

- [Apache License](http://www.apache.org/licenses/LICENSE-2.0), Version 2.0
- [BSD-3-Clause License](https://opensource.org/licenses/BSD-3-Clause)

