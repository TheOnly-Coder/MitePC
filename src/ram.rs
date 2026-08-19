/// Virtual RAM simulator using a page-based sparse memory model.
///
/// Physical RAM is not fully allocated; pages are created on demand.
/// This allows simulating large amounts of RAM (e.g. 16 GB) without
/// actually consuming that much host memory.

use std::collections::HashMap;

const PAGE_SIZE: u64 = 4096;

/// Represents the simulated RAM of the virtual PC.
pub struct Ram {
    /// Total RAM size in bytes.
    total_size: u64,
    /// Total number of pages.
    total_pages: u64,
    /// Sparse page storage. Key = page index, Value = page data.
    pages: HashMap<u64, Vec<u8>>,
    /// Track number of allocated (used) pages.
    used_pages: usize,
}

impl Ram {
    /// Create a new RAM module with the given size in megabytes.
    pub fn new(mb: u64) -> Self {
        let total_size = mb * 1024 * 1024;
        let total_pages = (total_size + PAGE_SIZE - 1) / PAGE_SIZE;
        Self {
            total_size,
            total_pages,
            pages: HashMap::new(),
            used_pages: 0,
        }
    }

    /// Returns the total size of the RAM in bytes.
    pub fn size(&self) -> u64 {
        self.total_size
    }

    /// Returns the number of used (allocated) pages.
    pub fn used_pages(&self) -> usize {
        self.used_pages
    }

    /// Returns total page count.
    pub fn total_pages(&self) -> u64 {
        self.total_pages
    }

    /// Returns RAM usage as a percentage (0.0 - 100.0).
    pub fn usage_percent(&self) -> f64 {
        if self.total_pages == 0 {
            return 0.0;
        }
        (self.used_pages as f64 / self.total_pages as f64) * 100.0
    }

    /// Read a single byte from the given address.
    /// Returns 0 if the page hasn't been written yet (zero-fill on read).
    pub fn read_byte(&self, addr: u64) -> u8 {
        if addr >= self.total_size {
            return 0;
        }
        let page_idx = addr / PAGE_SIZE;
        let offset = (addr % PAGE_SIZE) as usize;
        if let Some(page) = self.pages.get(&page_idx) {
            page[offset]
        } else {
            0 // Uninitialized memory reads as zero
        }
    }

    /// Write a single byte to the given address.
    /// Allocates a new page if necessary.
    pub fn write_byte(&mut self, addr: u64, value: u8) {
        if addr >= self.total_size {
            return;
        }
        let page_idx = addr / PAGE_SIZE;
        let offset = (addr % PAGE_SIZE) as usize;
        let page = self.pages.entry(page_idx).or_insert_with(|| {
            self.used_pages += 1;
            vec![0u8; PAGE_SIZE as usize]
        });
        page[offset] = value;
    }

    /// Read a 32-bit little-endian word from the given address.
    pub fn read_u32(&self, addr: u64) -> u32 {
        let b0 = self.read_byte(addr) as u32;
        let b1 = self.read_byte(addr + 1) as u32;
        let b2 = self.read_byte(addr + 2) as u32;
        let b3 = self.read_byte(addr + 3) as u32;
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }

    /// Write a 32-bit little-endian word to the given address.
    pub fn write_u32(&mut self, addr: u64, value: u32) {
        self.write_byte(addr, (value & 0xFF) as u8);
        self.write_byte(addr + 1, ((value >> 8) & 0xFF) as u8);
        self.write_byte(addr + 2, ((value >> 16) & 0xFF) as u8);
        self.write_byte(addr + 3, ((value >> 24) & 0xFF) as u8);
    }

    /// Read a 64-bit little-endian word from the given address.
    pub fn read_u64(&self, addr: u64) -> u64 {
        let lo = self.read_u32(addr) as u64;
        let hi = self.read_u32(addr + 4) as u64;
        lo | (hi << 32)
    }

    /// Write a 64-bit little-endian word to the given address.
    pub fn write_u64(&mut self, addr: u64, value: u64) {
        self.write_u32(addr, (value & 0xFFFFFFFF) as u32);
        self.write_u32(addr + 4, ((value >> 32) & 0xFFFFFFFF) as u32);
    }

    /// Read a slice of bytes into a buffer.
    pub fn read_bytes(&self, addr: u64, buf: &mut [u8]) {
        for (i, byte) in buf.iter_mut().enumerate() {
            *byte = self.read_byte(addr + i as u64);
        }
    }

    /// Write a slice of bytes from a buffer.
    pub fn write_bytes(&mut self, addr: u64, data: &[u8]) {
        for (i, &byte) in data.iter().enumerate() {
            self.write_byte(addr + i as u64, byte);
        }
    }

    /// Write a null-terminated string to memory.
    pub fn write_string(&mut self, addr: u64, s: &str) {
        self.write_bytes(addr, s.as_bytes());
        self.write_byte(addr + s.len() as u64, 0);
    }

    /// Read a null-terminated string from memory.
    pub fn read_string(&self, addr: u64, max_len: usize) -> String {
        let mut buf = Vec::with_capacity(max_len);
        for i in 0..max_len {
            let b = self.read_byte(addr + i as u64);
            if b == 0 {
                break;
            }
            buf.push(b);
        }
        String::from_utf8_lossy(&buf).to_string()
    }

    /// Zero out a region of memory.
    pub fn zero(&mut self, addr: u64, len: u64) {
        for i in 0..len {
            self.write_byte(addr + i, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ram_basic() {
        let mut ram = Ram::new(1); // 1 MB
        ram.write_byte(0x1000, 0x42);
        assert_eq!(ram.read_byte(0x1000), 0x42);
        assert_eq!(ram.read_byte(0x1001), 0); // unwritten reads as 0
    }

    #[test]
    fn test_ram_u32() {
        let mut ram = Ram::new(1);
        ram.write_u32(0x100, 0xDEADBEEF);
        assert_eq!(ram.read_u32(0x100), 0xDEADBEEF);
    }

    #[test]
    fn test_ram_string() {
        let mut ram = Ram::new(1);
        ram.write_string(0x200, "hello");
        assert_eq!(ram.read_string(0x200, 32), "hello");
    }

    #[test]
    fn test_ram_out_of_bounds() {
        let ram = Ram::new(1); // 1 MB
        assert_eq!(ram.read_byte(0x100000), 0); // out of bounds
    }

    #[test]
    fn test_sparse_pages() {
        let mut ram = Ram::new(4); // 4 MB
        assert_eq!(ram.used_pages(), 0);
        ram.write_byte(0, 1);
        assert_eq!(ram.used_pages(), 1);
        ram.write_byte(PAGE_SIZE, 2); // different page
        assert_eq!(ram.used_pages(), 2);
    }
}
