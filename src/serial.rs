// Copyright 2018 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
//
// Portions Copyright 2017 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the THIRD-PARTY file.

use std::collections::VecDeque;
use std::io::{self, Result};

use vmm_sys_util::eventfd::EventFd;

use crate::{DeviceIo, IoAddress};

const LOOP_SIZE: usize = 0x40;

// Depending on the operation, this represents either the Receiver Buffer Register (RBR)
// or the Transmitter Holding Register (THR).
const DATA: u8 = 0;
// Interrupt Enable Register.
const IER: u8 = 1;
// Interrupt Identification Register.
const IIR: u8 = 2;
// Line Control Register.
const LCR: u8 = 3;
// Modem Control Register.
const MCR: u8 = 4;
// Line Status Register.
const LSR: u8 = 5;
// Modem Status Register.
const MSR: u8 = 6;
// Scratch Register.
const SCR: u8 = 7;

const DLAB_LOW: u8 = 0;
const DLAB_HIGH: u8 = 1;

const IER_RECV_BIT: u8 = 0x1;
const IER_THR_BIT: u8 = 0x2;
const IER_FIFO_BITS: u8 = 0x0f;

const IIR_FIFO_BITS: u8 = 0xc0;
const IIR_NONE_BIT: u8 = 0x1;
const IIR_THR_BIT: u8 = 0x2;
const IIR_RECV_BIT: u8 = 0x4;

const LCR_DLAB_BIT: u8 = 0x80;

const LSR_DATA_BIT: u8 = 0x1;
const LSR_EMPTY_BIT: u8 = 0x20;
const LSR_IDLE_BIT: u8 = 0x40;

const MCR_LOOP_BIT: u8 = 0x10;

const DEFAULT_INTERRUPT_IDENTIFICATION: u8 = IIR_NONE_BIT; // no pending interrupt
const DEFAULT_LINE_STATUS: u8 = LSR_EMPTY_BIT | LSR_IDLE_BIT; // THR empty and line is idle
const DEFAULT_LINE_CONTROL: u8 = 0x3; // 8-bits per character
const DEFAULT_MODEM_CONTROL: u8 = 0x8; // Auxiliary output 2
const DEFAULT_MODEM_STATUS: u8 = 0x20 | 0x10 | 0x80; // data ready, clear to send, carrier detect
const DEFAULT_BAUD_DIVISOR: u16 = 12; // 9600 bps

/// Emulates serial COM ports commonly seen on x86 I/O ports 0x3f8/0x2f8/0x3e8/0x2e8.
///
/// This can optionally write the guest's output to a Write trait object. To send input to the
/// guest, use `queue_input_bytes`.
pub struct Serial {
    interrupt_enable: u8,
    interrupt_identification: u8,
    interrupt_evt: EventFd,
    line_control: u8,
    line_status: u8,
    modem_control: u8,
    modem_status: u8,
    scratch: u8,
    baud_divisor: u16,
    in_buffer: VecDeque<u8>,
    out: Option<Box<dyn io::Write + Send>>,
}

impl Serial {
    fn new(interrupt_evt: EventFd, out: Option<Box<dyn io::Write + Send>>) -> Serial {
        Serial {
            interrupt_enable: 0,
            interrupt_identification: DEFAULT_INTERRUPT_IDENTIFICATION,
            interrupt_evt,
            line_control: DEFAULT_LINE_CONTROL,
            line_status: DEFAULT_LINE_STATUS,
            modem_control: DEFAULT_MODEM_CONTROL,
            modem_status: DEFAULT_MODEM_STATUS,
            scratch: 0,
            baud_divisor: DEFAULT_BAUD_DIVISOR,
            in_buffer: VecDeque::new(),
            out,
        }
    }

    /// Constructs a Serial port ready for output.
    pub fn new_out(interrupt_evt: EventFd, out: Box<dyn io::Write + Send>) -> Serial {
        Self::new(interrupt_evt, Some(out))
    }

    /// Constructs a Serial port with no connected output.
    pub fn new_sink(interrupt_evt: EventFd) -> Serial {
        Self::new(interrupt_evt, None)
    }

    /// Queues raw bytes for the guest to read and signals the interrupt if the line status would
    /// change.
    pub fn queue_input_bytes(&mut self, c: &[u8]) -> Result<()> {
        if !self.is_loop() {
            self.in_buffer.extend(c);
            self.recv_data()?;
        }
        Ok(())
    }

    fn is_dlab_set(&self) -> bool {
        (self.line_control & LCR_DLAB_BIT) != 0
    }

    fn is_recv_intr_enabled(&self) -> bool {
        (self.interrupt_enable & IER_RECV_BIT) != 0
    }

    fn is_thr_intr_enabled(&self) -> bool {
        (self.interrupt_enable & IER_THR_BIT) != 0
    }

    fn is_loop(&self) -> bool {
        (self.modem_control & MCR_LOOP_BIT) != 0
    }

    fn add_intr_bit(&mut self, bit: u8) {
        self.interrupt_identification &= !IIR_NONE_BIT;
        self.interrupt_identification |= bit;
    }

    fn del_intr_bit(&mut self, bit: u8) {
        self.interrupt_identification &= !bit;
        if self.interrupt_identification == 0x0 {
            self.interrupt_identification = IIR_NONE_BIT;
        }
    }

    fn thr_empty(&mut self) -> io::Result<()> {
        if self.is_thr_intr_enabled() {
            self.add_intr_bit(IIR_THR_BIT);
            self.trigger_interrupt()?
        }
        Ok(())
    }

    fn recv_data(&mut self) -> io::Result<()> {
        if self.is_recv_intr_enabled() {
            self.add_intr_bit(IIR_RECV_BIT);
            self.trigger_interrupt()?
        }
        self.line_status |= LSR_DATA_BIT;
        Ok(())
    }

    fn trigger_interrupt(&mut self) -> io::Result<()> {
        self.interrupt_evt.write(1)
    }

    fn iir_reset(&mut self) {
        self.interrupt_identification = DEFAULT_INTERRUPT_IDENTIFICATION;
    }

    fn handle_write(&mut self, offset: u8, v: u8) -> io::Result<()> {
        match offset {
            DLAB_LOW if self.is_dlab_set() => {
                self.baud_divisor = (self.baud_divisor & 0xff00) | u16::from(v)
            }
            DLAB_HIGH if self.is_dlab_set() => {
                self.baud_divisor = (self.baud_divisor & 0x00ff) | (u16::from(v) << 8)
            }
            DATA => {
                if self.is_loop() {
                    if self.in_buffer.len() < LOOP_SIZE {
                        self.in_buffer.push_back(v);
                        self.recv_data()?;
                    }
                } else {
                    if let Some(out) = self.out.as_mut() {
                        out.write_all(&[v])?;
                        out.flush()?;
                    }
                    self.thr_empty()?;
                }
            }
            IER => self.interrupt_enable = v & IER_FIFO_BITS,
            LCR => self.line_control = v,
            MCR => self.modem_control = v,
            SCR => self.scratch = v,
            _ => {}
        }
        Ok(())
    }

    fn handle_read(&mut self, addr: u8) -> u8 {
        match addr {
            DLAB_LOW if self.is_dlab_set() => self.baud_divisor as u8,
            DLAB_HIGH if self.is_dlab_set() => (self.baud_divisor >> 8) as u8,
            DATA => {
                self.del_intr_bit(IIR_RECV_BIT);
                if self.in_buffer.len() <= 1 {
                    self.line_status &= !LSR_DATA_BIT;
                }
                self.in_buffer.pop_front().unwrap_or_default()
            }
            IER => self.interrupt_enable,
            IIR => {
                let v = self.interrupt_identification | IIR_FIFO_BITS;
                self.iir_reset();
                v
            }
            LCR => self.line_control,
            MCR => self.modem_control,
            LSR => self.line_status,
            MSR => self.modem_status,
            SCR => self.scratch,
            _ => 0,
        }
    }
}

impl DeviceIo for Serial {
    fn read(&mut self, addr: IoAddress, data: &mut [u8]) {
        if data.len() != 1 {
            return;
        }

        match addr {
            IoAddress::Pio(port) => {
                data[0] = self.handle_read(port as u8);
            }
            _ => {}
        }
    }

    fn write(&mut self, addr: IoAddress, data: &[u8]) {
        if data.len() != 1 {
            return;
        }

        match addr {
            IoAddress::Pio(port) => {
                if let Err(e) = self.handle_write(port as u8, data[0]) {
                    error!("Failed the write to serial: {:?}", e);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct SharedBuffer {
        buf: Arc<Mutex<Vec<u8>>>,
    }

    impl SharedBuffer {
        fn new() -> SharedBuffer {
            SharedBuffer {
                buf: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl io::Write for SharedBuffer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.buf.lock().unwrap().write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.buf.lock().unwrap().flush()
        }
    }

    #[test]
    fn serial_output() {
        let intr_evt = EventFd::new(0).unwrap();
        let serial_out = SharedBuffer::new();

        let mut serial = Serial::new_out(intr_evt, Box::new(serial_out.clone()));

        serial.write(IoAddress::Pio(DATA as u16), &[b'x', b'y']);
        serial.write(IoAddress::Pio(DATA as u16), &[b'a']);
        serial.write(IoAddress::Pio(DATA as u16), &[b'b']);
        serial.write(IoAddress::Pio(DATA as u16), &[b'c']);
        assert_eq!(
            serial_out.buf.lock().unwrap().as_slice(),
            &[b'a', b'b', b'c']
        );
    }

    #[test]
    fn serial_input() {
        let intr_evt = EventFd::new(0).unwrap();
        let serial_out = SharedBuffer::new();

        let mut serial =
            Serial::new_out(intr_evt.try_clone().unwrap(), Box::new(serial_out.clone()));

        // write 1 to the interrupt event fd, so that read doesn't block in case the event fd
        // counter doesn't change (for 0 it blocks)
        assert!(intr_evt.write(1).is_ok());
        serial.write(IoAddress::Pio(IER as u16), &[IER_RECV_BIT]);
        serial.queue_input_bytes(&[b'a', b'b', b'c']).unwrap();

        assert_eq!(intr_evt.read().unwrap(), 2);

        // check if reading in a 2-length array doesn't have side effects
        let mut data = [0u8, 0u8];
        serial.read(IoAddress::Pio(DATA as u16), &mut data[..]);
        assert_eq!(data, [0u8, 0u8]);

        let mut data = [0u8];
        serial.read(IoAddress::Pio(LSR as u16), &mut data[..]);
        assert_ne!(data[0] & LSR_DATA_BIT, 0);
        serial.read(IoAddress::Pio(DATA as u16), &mut data[..]);
        assert_eq!(data[0], b'a');
        serial.read(IoAddress::Pio(DATA as u16), &mut data[..]);
        assert_eq!(data[0], b'b');
        serial.read(IoAddress::Pio(DATA as u16), &mut data[..]);
        assert_eq!(data[0], b'c');

        // check if reading from the largest u8 offset returns 0
        serial.read(IoAddress::Pio(0xff), &mut data[..]);
        assert_eq!(data[0], 0);
    }

    #[test]
    fn serial_thr() {
        let intr_evt = EventFd::new(0).unwrap();
        let mut serial = Serial::new_sink(intr_evt.try_clone().unwrap());

        // write 1 to the interrupt event fd, so that read doesn't block in case the event fd
        // counter doesn't change (for 0 it blocks)
        assert!(intr_evt.write(1).is_ok());
        serial.write(IoAddress::Pio(IER as u16), &[IER_THR_BIT]);
        serial.write(IoAddress::Pio(DATA as u16), &[b'a']);

        assert_eq!(intr_evt.read().unwrap(), 2);
        let mut data = [0u8];
        serial.read(IoAddress::Pio(IER as u16), &mut data[..]);
        assert_eq!(data[0] & IER_FIFO_BITS, IER_THR_BIT);
        serial.read(IoAddress::Pio(IIR as u16), &mut data[..]);
        assert_ne!(data[0] & IIR_THR_BIT, 0);
    }

    #[test]
    fn serial_dlab() {
        let mut serial = Serial::new_sink(EventFd::new(0).unwrap());

        serial.write(IoAddress::Pio(LCR as u16), &[LCR_DLAB_BIT as u8]);
        serial.write(IoAddress::Pio(DLAB_LOW as u16), &[0x12 as u8]);
        serial.write(IoAddress::Pio(DLAB_HIGH as u16), &[0x34 as u8]);

        let mut data = [0u8];
        serial.read(IoAddress::Pio(LCR as u16), &mut data[..]);
        assert_eq!(data[0], LCR_DLAB_BIT as u8);
        serial.read(IoAddress::Pio(DLAB_LOW as u16), &mut data[..]);
        assert_eq!(data[0], 0x12);
        serial.read(IoAddress::Pio(DLAB_HIGH as u16), &mut data[..]);
        assert_eq!(data[0], 0x34);
    }

    #[test]
    fn serial_modem() {
        let mut serial = Serial::new_sink(EventFd::new(0).unwrap());

        serial.write(IoAddress::Pio(MCR as u16), &[MCR_LOOP_BIT as u8]);
        serial.write(IoAddress::Pio(DATA as u16), &[b'a']);
        serial.write(IoAddress::Pio(DATA as u16), &[b'b']);
        serial.write(IoAddress::Pio(DATA as u16), &[b'c']);

        let mut data = [0u8];
        serial.read(IoAddress::Pio(MSR as u16), &mut data[..]);
        assert_eq!(data[0], DEFAULT_MODEM_STATUS as u8);
        serial.read(IoAddress::Pio(MCR as u16), &mut data[..]);
        assert_eq!(data[0], MCR_LOOP_BIT as u8);
        serial.read(IoAddress::Pio(DATA as u16), &mut data[..]);
        assert_eq!(data[0], b'a');
        serial.read(IoAddress::Pio(DATA as u16), &mut data[..]);
        assert_eq!(data[0], b'b');
        serial.read(IoAddress::Pio(DATA as u16), &mut data[..]);
        assert_eq!(data[0], b'c');
    }

    #[test]
    fn serial_scratch() {
        let mut serial = Serial::new_sink(EventFd::new(0).unwrap());

        serial.write(IoAddress::Pio(SCR as u16), &[0x12 as u8]);

        let mut data = [0u8];
        serial.read(IoAddress::Pio(SCR as u16), &mut data[..]);
        assert_eq!(data[0], 0x12 as u8);
    }
}