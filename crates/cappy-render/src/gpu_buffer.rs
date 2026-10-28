//! A GPU buffer that grows to fit whatever is written into it.

use wgpu::{Buffer, BufferDescriptor, BufferUsages, Device, Queue};

const MIN_CAPACITY_BYTES: u64 = 4096;

pub(crate) struct GrowableBuffer {
    label: &'static str,
    usage: BufferUsages,
    buffer: Buffer,
}

impl GrowableBuffer {
    pub fn new(device: &Device, label: &'static str, usage: BufferUsages) -> Self {
        let usage = usage | BufferUsages::COPY_DST;
        Self {
            label,
            usage,
            buffer: create(device, label, usage, MIN_CAPACITY_BYTES),
        }
    }

    pub fn write(&mut self, device: &Device, queue: &Queue, bytes: &[u8]) {
        let needed = bytes.len() as u64;
        if needed > self.buffer.size() {
            let capacity = needed.next_power_of_two().max(MIN_CAPACITY_BYTES);
            self.buffer = create(device, self.label, self.usage, capacity);
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytes);
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }
}

fn create(device: &Device, label: &'static str, usage: BufferUsages, size: u64) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some(label),
        size,
        usage,
        mapped_at_creation: false,
    })
}
