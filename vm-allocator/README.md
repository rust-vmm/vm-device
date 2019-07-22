#vm-allocator

The `vm-allocator` crate is the rust-vmm resource allocator and manager. It provides
high level APIs for each kind of resources that would be used by VMMs. Meanwhile,
it provides a system level allocator that manages all resources instances for VMMs.

## Objectives

A Virtual Machine Manager (VMM) needs to manager the system resources like port IO
address space resource, memory-mapped I/O space resource, etc. And it also needs
to have a mechnism allocating such resources for devices in case of no firmware case.

## Architecture

### `Resource` and `ResourceSize` traits 

The `Resource` and `ResourceSize` traits are designed for each resource type like
port I/O address space resource, memory-mapped I/O space resource, legacy IRQ, etc,
which should implement the traits for allocation usage.

### `ResourceAllocator` trait

This is an abstration of each resource allocator mechanism, having `fn allocate()`
and `fn free()` methods with parameters implementing `Resource` and `ResourceSize`
traits describing the allocation and free requests.

### `IdAllocator`

This is an allocator structure implementing `ResourceAllocator` trait, and responsible
for any unsigned integer 32-bit resource type. For VMMs, legacy IRQ and device instance
id might be such resource.

### `AddressAllocator`

This is an allocator structure implementing `ResourceAllocator` trait, and responsible
for allocating and freeing a range of guest addresses. For VMMs, port I/O address space
resource, memory-mapped I/O space resource would be such resource.

### `SystemAllocator` trait

This trait is a system level allocator management for some VMMs, and can provides resource
allocation callbacks to dynamically allocate or free some resources on demand. This
helps caller decouple with VMM's system allocator.

Every VMMs can choose to implement this trait or not, according to whether the VMM wants to
have a lightweight allocation mechanism and resources lifetime management e.g. serverless.

A `Option<>` of callback could be a good way to make things alternatively. If a VMM wants
itself responsible for resource allocation, it can simply register a None callback inside
`vm-device` crate. If a VMM wants `vm-device` crate do the resources allocation management,
it can simply register the callback inside `vm-device` crate.

## Example

Before giving out different use cases, let's see the DeviceManager members.

```Rust
pub struct DeviceManager {
    device_id_cb: Option<IdAllocateFunc>,
    /// IRQ allocator callback.
    irq_cb: Option<IdAllocateFunc>,
    /// Port IO address resource allocator callback.
    pio_addr_cb: Option<AddrAllocateFunc>,
    /// Mmio address resource allocator callback.
    mmio_addr_cb: Option<AddrAllocateFunc>,

    /// Range mapping for VM exit mmio operations.
    mmio_bus: BTreeMap<Range, Arc<dyn Device>>,
    /// Range mapping for VM exit pio operations.
    pio_bus: BTreeMap<Range, Arc<dyn Device>>,
}

```
Case 1:
Let's create a lightweight VMM and make it allocate resources byself.

```Rust
// First we have a very simple system allocator.
struct SystemAllocator {
    irq: IdAllocator,
    device_id: IdAllocator,
}

let sys_alloc = SystemAllocator::new();

// VMM has a dummy-device-1 instance.
let dummy = DummyDevice::new();

// VMM wants to allocate each device resources by self.
let instance_id = sys_alloc.allocate_instance_id();
let irq = sys_alloc.allocate_irq();

// Register dummy-device-1 into vm-device::DeviceManager
// by a simple API with a known resources.
let dev_mgr = DeviceManager::new();
dev_mgr.simple_register_device(dummy, instance_id, irq, (0xcf8, 8));

// No unhotplug or unregister use case so no need to free.
```

Case 2:
Let's create a normal VMM which would be used with no firmware. 

```Rust
// First we have a system allocator which needs thread safe.
struct SystemAllocator {
    pio_addr: Arc<Mutex<AddrAllocator>>,
    mmio_addr: Arc<Mutex<AddrAllocator>>,
    irq: Arc<Mutex<IdAllocator>>,
    device_id: Arc<Mutex<IdAllocator>>,
}

let sys_alloc = SystemAllocator::new();

// VMM has a dummy-device-1 instance.
// And it gives out a vector which resources are needed like port IO address, irq.
let dummy = DummyDevice::new();
let resource:Vec<> = dummy.resource_request();

// The VMM wants to register the device.
// full_register_device() would call the allocation callback to allocate
// irq, instance id, port IO resources and then do simple_register_device().
// In such case, each callback would be Some(cb).
let dev_mgr = DeviceManager::new();
let instance_id = dev_mgr.full_register_device(dummy, resource);

// When dummy is unhotplugged, VMM calls device manager to free resources and unregister.
dev_mgr.full_unregister_device(instance_id);
```

Case 3:
A VMM with firmware allocation resources.

Parts of device resources need to be allocated by VMMs, and parts are allocated by firmware.
So `Option<>` type for callbacks are useful in such case. In such case, some callbacks would
be `Some(cb)` like port IO, and some would be `None` like mmio.
