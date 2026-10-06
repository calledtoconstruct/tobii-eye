//! libusb transport for the Eye Tracker 5 runtime interface.
//!
//! Gaze is vendor bulk on interface 0: OUT 0x05, IN 0x83. A transfer larger
//! than 8192 bytes is split, and each chunk's envelope length describes only
//! that chunk. The TTP header in the first chunk still carries the full size.

use std::time::Duration;

use rusb::{Context, DeviceHandle, Direction, Recipient, RequestType, UsbContext};

const VID: u16 = 0x2104;
const PID: u16 = 0x0313;
const EP_IN: u8 = 0x83;
const EP_OUT: u8 = 0x05;
const INTERFACE: u8 = 0;
const CHUNK: usize = 8192;

#[derive(Debug)]
pub enum UsbError {
    Init(rusb::Error),
    NotFound,
    Claim(rusb::Error),
    Session(rusb::Error),
}

pub struct Usb {
    handle: Option<DeviceHandle<Context>>,
    lost: bool,
}

impl Usb {
    pub fn new() -> Self {
        Self {
            handle: None,
            lost: false,
        }
    }

    pub fn lost(&self) -> bool {
        self.lost
    }

    pub fn open(&mut self) -> Result<(), UsbError> {
        self.close_handle();
        let context = Context::new().map_err(UsbError::Init)?;
        let handle = context
            .open_device_with_vid_pid(VID, PID)
            .ok_or(UsbError::NotFound)?;
        let _ = handle.set_auto_detach_kernel_driver(true);
        if handle.kernel_driver_active(INTERFACE).ok() == Some(true) {
            let _ = handle.detach_kernel_driver(INTERFACE);
        }
        handle
            .claim_interface(INTERFACE)
            .map_err(UsbError::Claim)?;
        let request_type = rusb::request_type(Direction::Out, RequestType::Vendor, Recipient::Interface);
        handle
            .write_control(request_type, 0x41, 0, 0, &[], Duration::from_secs(1))
            .map_err(UsbError::Session)?;
        self.lost = false;
        self.handle = Some(handle);
        eprintln!("tobiifreed: opened device {VID:04x}:{PID:04x}");
        Ok(())
    }

    pub fn close_device(&mut self) {
        self.lost = true;
        self.close_handle();
    }

    pub fn send(&mut self, data: &[u8]) -> bool {
        if self.handle.is_none() || self.lost {
            return false;
        }
        if data.len() <= CHUNK {
            return self.bulk_out(data);
        }
        let mut first = data[..CHUNK].to_vec();
        let cont_data = (CHUNK - 8) as u32;
        first[4..8].copy_from_slice(&cont_data.to_le_bytes());
        if !self.bulk_out(&first) {
            return false;
        }
        let mut offset = CHUNK;
        while offset < data.len() {
            let end = (offset + (CHUNK - 8)).min(data.len());
            let piece = &data[offset..end];
            let mut chunk = Vec::with_capacity(8 + piece.len());
            chunk.extend_from_slice(&[0, 0, 0, 0]);
            chunk.extend_from_slice(&(piece.len() as u32).to_le_bytes());
            chunk.extend_from_slice(piece);
            if !self.bulk_out(&chunk) {
                return false;
            }
            offset = end;
        }
        true
    }

    pub fn recv(&mut self, timeout: Duration) -> Option<Vec<u8>> {
        if self.handle.is_none() || self.lost {
            return None;
        }
        let mut buf = vec![0u8; 16384];
        let result = {
            let handle = self.handle.as_ref()?;
            handle.read_bulk(EP_IN, &mut buf, timeout)
        };
        match result {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some(buf)
            }
            Err(rusb::Error::Timeout) => None,
            Err(err) => {
                self.note_lost(err);
                None
            }
        }
    }

    fn bulk_out(&mut self, data: &[u8]) -> bool {
        if self.handle.is_none() || self.lost {
            return false;
        }
        let result = if let Some(handle) = self.handle.as_ref() {
            handle.write_bulk(EP_OUT, data, Duration::from_millis(2000))
        } else {
            return false;
        };
        match result {
            Ok(n) if n == data.len() => true,
            Ok(_) => false,
            Err(err) => {
                self.note_lost(err);
                false
            }
        }
    }

    fn note_lost(&mut self, err: rusb::Error) {
        if self.lost {
            return;
        }
        self.lost = true;
        eprintln!("tobiifreed: tracker link lost ({err})");
    }

    fn close_handle(&mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        let request_type = rusb::request_type(Direction::Out, RequestType::Vendor, Recipient::Interface);
        let _ = handle.write_control(request_type, 0x42, 0, 0, &[], Duration::from_millis(500));
        let _ = handle.release_interface(INTERFACE);
    }
}

impl Default for Usb {
    fn default() -> Self {
        Self::new()
    }
}
