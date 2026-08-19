/// MiteFS — The MiteOS File System
///
/// A custom on-disk filesystem format with the `.mite` extension.
/// Not based on any existing filesystem (ext, FAT, NTFS, etc.).
///
/// ## Disk Layout
/// ```text
/// Block 0:       Superblock
/// Block 1..N:    Inode table
/// Block N..M:    Free block bitmap
/// Block M..End:  Data blocks
/// ```
///
/// ## Superblock (4096 bytes)
/// ```text
/// Offset  Size   Field
/// 0x000   4      Magic: "MITE" (0x4D495445)
/// 0x004   2      Version (1)
/// 0x006   2      Block size in bytes (4096)
/// 0x008   8      Total image size in bytes
/// 0x010   8      Total number of blocks
/// 0x018   8      Total inodes
/// 0x020   8      First data block index
/// 0x028   8      Free block bitmap start
/// 0x030   8      Root directory inode number
/// 0x038   8      First free inode number
/// 0x040   8      Total free blocks
/// 0x048   8      Total free inodes
/// 0x050   3952   Reserved (zeroes)
/// ```
///
/// ## Inode (256 bytes each, 16 per block)
/// ```text
/// Offset  Size   Field
/// 0x000   4      Inode number
/// 0x004   1      Type: 0=free, 1=directory, 2=file
/// 0x005   1      Permissions (rwxrwxrwx as octal bits)
/// 0x006   2      Parent inode number
/// 0x008   256    Filename (null-terminated UTF-8)
/// 0x108   8      File size in bytes
/// 0x110   8      Created timestamp (unix epoch)
/// 0x118   8      Modified timestamp (unix epoch)
/// 0x120   8      First data block index (0 = no data)
/// 0x128   8      Number of data blocks used
/// 0x130   120    Reserved (zeroes)
/// ```

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

// ---- Constants ----

const MITE_MAGIC: &[u8; 4] = b"MITE";
const MITE_VERSION: u16 = 1;
const BLOCK_SIZE: u32 = 4096;
const INODE_SIZE: u32 = 512;
const INODES_PER_BLOCK: u32 = BLOCK_SIZE / INODE_SIZE; // 8

const INODE_TYPE_FREE: u8 = 0;
const INODE_TYPE_DIR: u8 = 1;
const INODE_TYPE_FILE: u8 = 2;

/// Maximum filename length (255 chars + null terminator).
const MAX_NAME_LEN: usize = 255;

// ---- Superblock ----

#[derive(Debug, Clone)]
pub struct Superblock {
    pub magic: [u8; 4],
    pub version: u16,
    pub block_size: u32,
    pub total_size: u64,
    pub total_blocks: u64,
    pub total_inodes: u64,
    pub first_data_block: u64,
    pub bitmap_start: u64,
    pub root_inode: u64,
    pub first_free_inode: u64,
    pub free_blocks: u64,
    pub free_inodes: u64,
}

impl Superblock {
    fn new(total_size: u64, total_inodes: u64) -> Self {
        let total_blocks = total_size / BLOCK_SIZE as u64;
        // Inode table: 1 superblock + enough blocks for all inodes
        let inode_table_blocks = (total_inodes + INODES_PER_BLOCK as u64 - 1) / INODES_PER_BLOCK as u64;
        let bitmap_start = 1 + inode_table_blocks;
        // Bitmap size: one bit per block
        let bitmap_blocks = (total_blocks + 8 - 1) / 8 / BLOCK_SIZE as u64;
        let first_data_block = bitmap_start + bitmap_blocks.max(1);
        let free_blocks = total_blocks.saturating_sub(first_data_block);

        Self {
            magic: *MITE_MAGIC,
            version: MITE_VERSION,
            block_size: BLOCK_SIZE,
            total_size,
            total_blocks,
            total_inodes,
            first_data_block,
            bitmap_start,
            root_inode: 1, // Inode 1 is always root
            first_free_inode: 2, // Inode 0 = unused, 1 = root, 2 = first free
            free_blocks,
            free_inodes: total_inodes - 2, // Root and inode 0 are used
        }
    }

    fn serialize(&self, buf: &mut [u8; BLOCK_SIZE as usize]) {
        let mut off = 0usize;
        buf[off..off + 4].copy_from_slice(&self.magic); off += 4;
        buf[off..off + 2].copy_from_slice(&self.version.to_le_bytes()); off += 2;
        buf[off..off + 2].copy_from_slice(&(self.block_size as u16).to_le_bytes()); off += 2;
        buf[off..off + 8].copy_from_slice(&self.total_size.to_le_bytes()); off += 8;
        buf[off..off + 8].copy_from_slice(&self.total_blocks.to_le_bytes()); off += 8;
        buf[off..off + 8].copy_from_slice(&self.total_inodes.to_le_bytes()); off += 8;
        buf[off..off + 8].copy_from_slice(&self.first_data_block.to_le_bytes()); off += 8;
        buf[off..off + 8].copy_from_slice(&self.bitmap_start.to_le_bytes()); off += 8;
        buf[off..off + 8].copy_from_slice(&self.root_inode.to_le_bytes()); off += 8;
        buf[off..off + 8].copy_from_slice(&self.first_free_inode.to_le_bytes()); off += 8;
        buf[off..off + 8].copy_from_slice(&self.free_blocks.to_le_bytes()); off += 8;
        buf[off..off + 8].copy_from_slice(&self.free_inodes.to_le_bytes()); off += 8;
        // Rest is zeroes (reserved)
    }

    fn deserialize(buf: &[u8]) -> Result<Self, String> {
        if buf.len() < 80 {
            return Err("Superblock too small".into());
        }
        let mut off = 0usize;
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&buf[off..off + 4]); off += 4;
        if &magic != MITE_MAGIC {
            return Err(format!("Invalid magic: {:?}", magic));
        }

        let read_u16 = |buf: &[u8], o: &mut usize| -> u16 {
            let v = u16::from_le_bytes(buf[*o..*o + 2].try_into().unwrap());
            *o += 2; v
        };
        let read_u32 = |buf: &[u8], o: &mut usize| -> u32 {
            let v = u32::from_le_bytes(buf[*o..*o + 4].try_into().unwrap());
            *o += 4; v
        };
        let read_u64 = |buf: &[u8], o: &mut usize| -> u64 {
            let v = u64::from_le_bytes(buf[*o..*o + 8].try_into().unwrap());
            *o += 8; v
        };

        let version = read_u16(buf, &mut off);
        let block_size = read_u16(buf, &mut off) as u32;
        let total_size = read_u64(buf, &mut off);
        let total_blocks = read_u64(buf, &mut off);
        let total_inodes = read_u64(buf, &mut off);
        let first_data_block = read_u64(buf, &mut off);
        let bitmap_start = read_u64(buf, &mut off);
        let root_inode = read_u64(buf, &mut off);
        let first_free_inode = read_u64(buf, &mut off);
        let free_blocks = read_u64(buf, &mut off);
        let free_inodes = read_u64(buf, &mut off);

        Ok(Self {
            magic, version, block_size, total_size, total_blocks,
            total_inodes, first_data_block, bitmap_start,
            root_inode, first_free_inode, free_blocks, free_inodes,
        })
    }
}

// ---- Inode ----

#[derive(Debug, Clone)]
pub struct Inode {
    pub inode_num: u64,
    pub inode_type: u8,  // 0=free, 1=dir, 2=file
    pub permissions: u8, // rwxrwxrwx
    pub parent: u64,
    pub name: String,
    pub size: u64,
    pub created: u64,
    pub modified: u64,
    pub first_block: u64,
    pub block_count: u64,
}

impl Inode {
    fn new_free(num: u64) -> Self {
        Self {
            inode_num: num,
            inode_type: INODE_TYPE_FREE,
            permissions: 0,
            parent: 0,
            name: String::new(),
            size: 0,
            created: 0,
            modified: 0,
            first_block: 0,
            block_count: 0,
        }
    }

    fn new_dir(num: u64, parent: u64, name: &str) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            inode_num: num,
            inode_type: INODE_TYPE_DIR,
            permissions: 0x07 | (0x05 << 3) | (0x05 << 6), // rwxr-xr-x
            parent,
            name: name.to_string(),
            size: 0,
            created: now,
            modified: now,
            first_block: 0,
            block_count: 0,
        }
    }

    fn new_file(num: u64, parent: u64, name: &str) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            inode_num: num,
            inode_type: INODE_TYPE_FILE,
            permissions: 0x06 | (0x04 << 3) | (0x04 << 6), // rw-r--r--
            parent,
            name: name.to_string(),
            size: 0,
            created: now,
            modified: now,
            first_block: 0,
            block_count: 0,
        }
    }

    fn serialize(&self, buf: &mut [u8; INODE_SIZE as usize]) {
        let mut off = 0usize;
        buf[off..off + 4].copy_from_slice(&(self.inode_num as u32).to_le_bytes()); off += 4;
        buf[off] = self.inode_type; off += 1;
        buf[off] = self.permissions; off += 1;
        buf[off..off + 2].copy_from_slice(&(self.parent as u16).to_le_bytes()); off += 2;
        // Name: 256 bytes
        let name_bytes = self.name.as_bytes();
        let name_len = name_bytes.len().min(MAX_NAME_LEN);
        buf[off..off + name_len].copy_from_slice(&name_bytes[..name_len]);
        off += MAX_NAME_LEN;
        buf[off..off + 8].copy_from_slice(&self.size.to_le_bytes()); off += 8;
        buf[off..off + 8].copy_from_slice(&self.created.to_le_bytes()); off += 8;
        buf[off..off + 8].copy_from_slice(&self.modified.to_le_bytes()); off += 8;
        buf[off..off + 8].copy_from_slice(&self.first_block.to_le_bytes()); off += 8;
        buf[off..off + 8].copy_from_slice(&self.block_count.to_le_bytes()); off += 8;
    }

    fn deserialize(buf: &[u8]) -> Self {
        let mut off = 0usize;
        let read_u64 = |buf: &[u8], o: &mut usize| -> u64 {
            let v = u64::from_le_bytes(buf[*o..*o + 8].try_into().unwrap());
            *o += 8; v
        };
        let inode_num = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap()) as u64; off += 4;
        let inode_type = buf[off]; off += 1;
        let permissions = buf[off]; off += 1;
        let parent = u16::from_le_bytes(buf[off..off + 2].try_into().unwrap()) as u64; off += 2;
        // Name
        let name_end = off + MAX_NAME_LEN;
        let name_bytes = &buf[off..name_end];
        let null_pos = name_bytes.iter().position(|&b| b == 0).unwrap_or(MAX_NAME_LEN);
        let name = String::from_utf8_lossy(&name_bytes[..null_pos]).to_string();
        off = name_end;

        let size = read_u64(buf, &mut off);
        let created = read_u64(buf, &mut off);
        let modified = read_u64(buf, &mut off);
        let first_block = read_u64(buf, &mut off);
        let block_count = read_u64(buf, &mut off);

        Self {
            inode_num, inode_type, permissions, parent,
            name, size, created, modified, first_block, block_count,
        }
    }

    pub fn is_dir(&self) -> bool { self.inode_type == INODE_TYPE_DIR }
    pub fn is_file(&self) -> bool { self.inode_type == INODE_TYPE_FILE }
    pub fn is_free(&self) -> bool { self.inode_type == INODE_TYPE_FREE }
}

// ---- MiteFS ----

/// The MiteOS Filesystem driver.
/// Operates on a `.mite` image file.
pub struct MiteFS {
    image_path: PathBuf,
    file: File,
    superblock: Superblock,
    /// Cached inodes.
    inodes: Vec<Inode>,
}

impl MiteFS {
    /// Open an existing .mite image or create a new one.
    pub fn open_or_create(path: &Path, size_mb: u64) -> Result<Self, String> {
        let path = path.to_path_buf();
        if path.exists() {
            Self::open(&path)
        } else {
            Self::create(&path, size_mb)
        }
    }

    /// Create a new .mite image with the given size in MB.
    pub fn create(path: &Path, size_mb: u64) -> Result<Self, String> {
        let total_size = size_mb * 1024 * 1024;
        let total_inodes = 4096u64; // Support up to 4096 files/dirs

        // Create the image file (sparse)
        let mut file = OpenOptions::new()
            .read(true).write(true).create(true)
            .open(path)
            .map_err(|e| format!("Failed to create mite image: {}", e))?;

        // Set the file size
        file.set_len(total_size)
            .map_err(|e| format!("Failed to set image size: {}", e))?;

        let sb = Superblock::new(total_size, total_inodes);

        // Write superblock (block 0)
        let mut sb_buf = [0u8; BLOCK_SIZE as usize];
        sb.serialize(&mut sb_buf);
        file.seek(SeekFrom::Start(0))
            .map_err(|e| format!("Seek failed: {}", e))?;
        file.write_all(&sb_buf)
            .map_err(|e| format!("Failed to write superblock: {}", e))?;

        // Initialize all inodes as free (skip inode 0 and 1: reserved/root)
        let mut inode_buf = [0u8; BLOCK_SIZE as usize];
        for i in 0..total_inodes {
            if i <= 1 { continue; } // Skip reserved inodes
            let inode = Inode::new_free(i);
            let offset_in_block = (i % INODES_PER_BLOCK as u64) as usize * INODE_SIZE as usize;
            let mut ibuf = [0u8; INODE_SIZE as usize];
            inode.serialize(&mut ibuf);
            inode_buf[offset_in_block..offset_in_block + INODE_SIZE as usize]
                .copy_from_slice(&ibuf);

            // Flush when block is full or last inode
            let next_block_boundary = ((i / INODES_PER_BLOCK as u64) + 1) * INODES_PER_BLOCK as u64;
            if i + 1 == next_block_boundary || i == total_inodes - 1 {
                let block_num = 1 + (i / INODES_PER_BLOCK as u64);
                file.seek(SeekFrom::Start(block_num * BLOCK_SIZE as u64))
                    .map_err(|e| format!("Seek failed: {}", e))?;
                file.write_all(&inode_buf)
                    .map_err(|e| format!("Failed to write inodes: {}", e))?;
                inode_buf = [0u8; BLOCK_SIZE as usize];
            }
        }

        // Create root directory (inode 1) — written AFTER free inode init
        let root = Inode::new_dir(1, 0, "/");
        let root_offset = BLOCK_SIZE as u64 + 1 * INODE_SIZE as u64; // block 1, second slot
        let mut root_buf = [0u8; INODE_SIZE as usize];
        root.serialize(&mut root_buf);
        file.seek(SeekFrom::Start(root_offset))
            .map_err(|e| format!("Seek failed: {}", e))?;
        file.write_all(&root_buf)
            .map_err(|e| format!("Failed to write root inode: {}", e))?;

        // Initialize the free block bitmap
        let bitmap_size = ((sb.total_blocks + 7) / 8) as usize;
        let mut bitmap = vec![0u8; bitmap_size];
        // Mark blocks before first_data_block as used
        for i in 0..sb.first_data_block {
            let byte_idx = (i / 8) as usize;
            let bit_idx = (i % 8) as u8;
            if byte_idx < bitmap.len() {
                bitmap[byte_idx] |= 1 << bit_idx;
            }
        }
        file.seek(SeekFrom::Start(sb.bitmap_start * BLOCK_SIZE as u64))
            .map_err(|e| format!("Seek failed: {}", e))?;
        file.write_all(&bitmap)
            .map_err(|e| format!("Failed to write bitmap: {}", e))?;

        file.flush().map_err(|e| format!("Flush failed: {}", e))?;

        // Now open it
        Self::open(path)
    }

    /// Open an existing .mite image.
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut file = OpenOptions::new()
            .read(true).write(true)
            .open(path)
            .map_err(|e| format!("Failed to open mite image '{}': {}", path.display(), e))?;

        // Read and validate superblock
        let mut sb_buf = [0u8; BLOCK_SIZE as usize];
        file.seek(SeekFrom::Start(0))
            .map_err(|e| format!("Seek failed: {}", e))?;
        file.read_exact(&mut sb_buf)
            .map_err(|e| format!("Failed to read superblock: {}", e))?
        ;

        let sb = Superblock::deserialize(&sb_buf)?;

        // Load all inodes into memory
        let mut inodes = Vec::with_capacity(sb.total_inodes as usize);
        let mut inode_buf = [0u8; BLOCK_SIZE as usize];
        for block in 0..((sb.total_inodes + INODES_PER_BLOCK as u64 - 1) / INODES_PER_BLOCK as u64) {
            let block_offset = (1 + block) * BLOCK_SIZE as u64;
            file.seek(SeekFrom::Start(block_offset))
                .map_err(|e| format!("Seek failed: {}", e))?;
            file.read_exact(&mut inode_buf)
                .map_err(|e| format!("Failed to read inodes: {}", e))?;
            for i in 0..INODES_PER_BLOCK as usize {
                let offset = i * INODE_SIZE as usize;
                let inode = Inode::deserialize(&inode_buf[offset..offset + INODE_SIZE as usize]);
                if inodes.len() < sb.total_inodes as usize {
                    inodes.push(inode);
                }
            }
        }

        Ok(Self {
            image_path: path.to_path_buf(),
            file,
            superblock: sb,
            inodes,
        })
    }

    /// Get a reference to the superblock.
    pub fn superblock(&self) -> &Superblock {
        &self.superblock
    }

    /// Get an inode by number.
    pub fn get_inode(&self, num: u64) -> Option<&Inode> {
        self.inodes.get(num as usize)
    }

    /// Get a mutable inode by number.
    fn get_inode_mut(&mut self, num: u64) -> Option<&mut Inode> {
        self.inodes.get_mut(num as usize)
    }

    /// Allocate a free inode. Returns the inode number.
    fn alloc_inode(&mut self) -> Option<u64> {
        // Start from inode 2 (0 and 1 are reserved)
        for i in 2..self.inodes.len() {
            if self.inodes[i].is_free() {
                self.superblock.free_inodes = self.superblock.free_inodes.saturating_sub(1);
                self.superblock.first_free_inode = (i as u64 + 1).min(self.inodes.len() as u64);
                return Some(i as u64);
            }
        }
        None
    }

    /// Allocate a free data block. Returns the block number.
    fn alloc_block(&mut self) -> Option<u64> {
        let sb = &self.superblock;
        let bitmap_start = sb.bitmap_start * BLOCK_SIZE as u64;
        let bitmap_size = ((sb.total_blocks + 7) / 8) as usize;
        let mut bitmap = vec![0u8; bitmap_size];

        self.file.seek(SeekFrom::Start(bitmap_start))
            .ok()?;
        self.file.read_exact(&mut bitmap).ok()?
        ;

        for i in sb.first_data_block..sb.total_blocks {
            let byte_idx = (i / 8) as usize;
            let bit_idx = (i % 8) as u8;
            if byte_idx < bitmap.len() && (bitmap[byte_idx] & (1 << bit_idx)) == 0 {
                // Mark as used
                bitmap[byte_idx] |= 1 << bit_idx;
                self.file.seek(SeekFrom::Start(bitmap_start))
                    .ok()?;
                self.file.write_all(&bitmap).ok()?
                ;
                self.superblock.free_blocks = self.superblock.free_blocks.saturating_sub(1);
                return Some(i);
            }
        }
        None
    }

    /// Persist an inode to disk.
    fn write_inode(&mut self, inode: &Inode) -> Result<(), String> {
        let block_idx = 1 + inode.inode_num / INODES_PER_BLOCK as u64;
        let offset_in_block = (inode.inode_num % INODES_PER_BLOCK as u64) as usize * INODE_SIZE as usize;
        let block_offset = block_idx * BLOCK_SIZE as u64 + offset_in_block as u64;

        let mut buf = [0u8; INODE_SIZE as usize];
        inode.serialize(&mut buf);
        self.file.seek(SeekFrom::Start(block_offset))
            .map_err(|e| format!("Seek: {}", e))?;
        self.file.write_all(&buf)
            .map_err(|e| format!("Write inode: {}", e))?;
        self.file.flush()
            .map_err(|e| format!("Flush: {}", e))?;
        Ok(())
    }

    /// Write the superblock to disk.
    fn write_superblock(&mut self) -> Result<(), String> {
        let mut buf = [0u8; BLOCK_SIZE as usize];
        self.superblock.serialize(&mut buf);
        self.file.seek(SeekFrom::Start(0))
            .map_err(|e| format!("Seek: {}", e))?;
        self.file.write_all(&buf)
            .map_err(|e| format!("Write sb: {}", e))?;
        self.file.flush()
            .map_err(|e| format!("Flush: {}", e))?;
        Ok(())
    }

    /// Read data from a chain of blocks into a buffer.
    fn read_block_chain(&mut self, first_block: u64, block_count: u64, size: u64) -> Result<Vec<u8>, String> {
        let mut data = Vec::with_capacity(size as usize);
        let mut current_block = first_block;
        let mut bytes_read = 0u64;

        for _ in 0..block_count {
            let block_offset = current_block * BLOCK_SIZE as u64;
            let to_read = (size.saturating_sub(bytes_read)).min(BLOCK_SIZE as u64) as usize;
            let mut buf = vec![0u8; BLOCK_SIZE as usize];
            self.file.seek(SeekFrom::Start(block_offset))
                .map_err(|e| format!("Seek: {}", e))?;
            self.file.read_exact(&mut buf)
                .map_err(|e| format!("Read block: {}", e))?;
            data.extend_from_slice(&buf[..to_read]);
            bytes_read += to_read as u64;
            if bytes_read >= size {
                break;
            }
            // Next block pointer is stored at the end of the current block (last 8 bytes)
            current_block = u64::from_le_bytes(
                buf[BLOCK_SIZE as usize - 8..].try_into().unwrap()
            );
            if current_block == 0 {
                break;
            }
        }

        data.truncate(size as usize);
        Ok(data)
    }

    /// Write data to a chain of blocks.
    fn write_block_chain(&mut self, first_block: u64, block_count: u64, data: &[u8]) -> Result<(), String> {
        let mut current_block = first_block;
        let mut offset = 0usize;

        for _ in 0..block_count {
            let block_offset = current_block * BLOCK_SIZE as u64;
            let chunk_size = (data.len().saturating_sub(offset)).min(BLOCK_SIZE as usize - 8);
            let mut buf = vec![0u8; BLOCK_SIZE as usize];
            buf[..chunk_size].copy_from_slice(&data[offset..offset + chunk_size]);
            offset += chunk_size;

            // If more data, allocate next block
            if offset < data.len() {
                if let Some(next) = self.alloc_block() {
                    buf[BLOCK_SIZE as usize - 8..].copy_from_slice(&next.to_le_bytes());
                    current_block = next;
                } else {
                    return Err("No free blocks for chain".into());
                }
            }

            self.file.seek(SeekFrom::Start(block_offset))
                .map_err(|e| format!("Seek: {}", e))?;
            self.file.write_all(&buf)
                .map_err(|e| format!("Write block: {}", e))?;
        }

        self.file.flush().map_err(|e| format!("Flush: {}", e))?;
        Ok(())
    }

    // ---- Public API ----

    /// Create a directory at the given path.
    /// Parent directories must exist.
    pub fn create_dir(&mut self, path: &str) -> Result<u64, String> {
        let parent_path = Self::parent_path(path);
        let dir_name = Self::basename(path);

        let parent_ino = self.resolve_path(&parent_path)?;
        let inode_num = self.alloc_inode()
            .ok_or("No free inodes")?;

        let mut inode = Inode::new_dir(inode_num, parent_ino, &dir_name);

        // Allocate a block for the directory
        if let Some(block) = self.alloc_block() {
            inode.first_block = block;
            inode.block_count = 1;
            inode.size = BLOCK_SIZE as u64;
        }

        self.inodes[inode_num as usize] = inode.clone();
        self.write_inode(&inode)?;
        self.write_superblock()?;

        Ok(inode_num)
    }

    /// Create a file at the given path and write initial content.
    pub fn create_file(&mut self, path: &str, content: &[u8]) -> Result<u64, String> {
        let parent_path = Self::parent_path(path);
        let file_name = Self::basename(path);

        let parent_ino = self.resolve_path(&parent_path)?;
        let inode_num = self.alloc_inode()
            .ok_or("No free inodes")?;

        let mut inode = Inode::new_file(inode_num, parent_ino, &file_name);
        inode.size = content.len() as u64;

        if !content.is_empty() {
            let blocks_needed = (content.len() as u64 + BLOCK_SIZE as u64 - 9) / (BLOCK_SIZE as u64 - 8);
            let first_block = self.alloc_block()
                .ok_or("No free blocks")?;
            inode.first_block = first_block;
            inode.block_count = blocks_needed;

            // Allocate remaining blocks
            let mut current = first_block;
            for _ in 1..blocks_needed {
                let next = self.alloc_block().ok_or("No free blocks")?;
                // Write next pointer in current block
                let block_offset = current * BLOCK_SIZE as u64 + (BLOCK_SIZE as u64 - 8);
                self.file.seek(SeekFrom::Start(block_offset))
                    .map_err(|e| format!("Seek: {}", e))?;
                self.file.write_all(&next.to_le_bytes())
                    .map_err(|e| format!("Write: {}", e))?;
                current = next;
            }

            self.write_block_chain(first_block, blocks_needed, content)?;
        }

        self.inodes[inode_num as usize] = inode.clone();
        self.write_inode(&inode)?;
        self.write_superblock()?;

        Ok(inode_num)
    }

    /// Read a file's contents.
    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>, String> {
        let inode_num = self.resolve_path(path)?;
        let inode = self.get_inode(inode_num)
            .ok_or("Inode not found")?
            .clone();

        if !inode.is_file() {
            return Err(format!("'{}' is not a file", path));
        }

        if inode.size == 0 || inode.first_block == 0 {
            return Ok(Vec::new());
        }

        self.read_block_chain(inode.first_block, inode.block_count, inode.size)
    }

    /// Read a file as a UTF-8 string.
    pub fn read_file_string(&mut self, path: &str) -> Result<String, String> {
        let data = self.read_file(path)?;
        String::from_utf8(data).map_err(|e| format!("Invalid UTF-8: {}", e))
    }

    /// List entries in a directory. Returns (inode_num, name, type) tuples.
    pub fn list_dir(&self, path: &str) -> Result<Vec<(u64, String, u8)>, String> {
        let inode_num = self.resolve_path(path)?;
        let inode = self.get_inode(inode_num)
            .ok_or("Inode not found")?
            .clone();

        if !inode.is_dir() {
            return Err(format!("'{}' is not a directory", path));
        }

        let mut entries = Vec::new();
        for ino in &self.inodes {
            if !ino.is_free() && ino.parent == inode_num {
                entries.push((ino.inode_num, ino.name.clone(), ino.inode_type));
            }
        }
        entries.sort_by(|a, b| a.1.cmp(&b.1));
        Ok(entries)
    }

    /// Check if a path exists.
    pub fn exists(&self, path: &str) -> bool {
        self.resolve_path(path).is_ok()
    }

    /// Get file/directory metadata at a path.
    pub fn stat(&self, path: &str) -> Result<&Inode, String> {
        let ino = self.resolve_path(path)?;
        self.get_inode(ino).ok_or("Inode not found".into())
    }

    /// Delete a file or empty directory.
    pub fn delete(&mut self, path: &str) -> Result<(), String> {
        let inode_num = self.resolve_path(path)?;
        let inode = self.get_inode(inode_num)
            .ok_or("Inode not found")?
            .clone();

        if inode.inode_num == self.superblock.root_inode {
            return Err("Cannot delete root directory".into());
        }

        // Free data blocks
        if inode.first_block != 0 && inode.block_count > 0 {
            let mut current = inode.first_block;
            let mut freed = 0u64;
            while freed < inode.block_count {
                let block_offset = current * BLOCK_SIZE as u64 + (BLOCK_SIZE as u64 - 8);
                let mut next_buf = [0u8; 8];
                self.file.seek(SeekFrom::Start(block_offset))
                    .map_err(|e| format!("Seek: {}", e))?;
                self.file.read_exact(&mut next_buf)
                    .map_err(|e| format!("Read: {}", e))?;
                let next = u64::from_le_bytes(next_buf);
                // Zero out the block
                let zero_block = vec![0u8; BLOCK_SIZE as usize];
                self.file.seek(SeekFrom::Start(current * BLOCK_SIZE as u64))
                    .map_err(|e| format!("Seek: {}", e))?;
                self.file.write_all(&zero_block)
                    .map_err(|e| format!("Write: {}", e))?;
                current = next;
                freed += 1;
                if next == 0 {
                    break;
                }
            }
            self.superblock.free_blocks += inode.block_count;
        }

        // Free the inode
        let free_inode = Inode::new_free(inode_num);
        self.write_inode(&free_inode)?;
        self.inodes[inode_num as usize] = free_inode;
        self.superblock.free_inodes += 1;
        self.write_superblock()?;

        Ok(())
    }

    /// Resolve a path string to an inode number.
    /// Supports absolute paths starting with '/'
    fn resolve_path(&self, path: &str) -> Result<u64, String> {
        let path = path.trim_start_matches('/');
        if path.is_empty() || path == "/" {
            return Ok(self.superblock.root_inode);
        }

        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current = self.superblock.root_inode;

        for part in parts {
            let inode = self.get_inode(current)
                .ok_or_else(|| format!("Inode {} not found", current))?
                .clone();

            if !inode.is_dir() {
                return Err(format!("'{}' is not a directory", inode.name));
            }

            let mut found = false;
            for ino in &self.inodes {
                if !ino.is_free() && ino.parent == current && ino.name == part {
                    current = ino.inode_num;
                    found = true;
                    break;
                }
            }

            if !found {
                return Err(format!("'{}' not found in '{}'", part, inode.name));
            }
        }

        Ok(current)
    }

    /// Extract the parent directory path.
    fn parent_path(path: &str) -> String {
        let path = path.trim_start_matches('/').trim_end_matches('/');
        if let Some(pos) = path.rfind('/') {
            if pos == 0 {
                return "/".to_string();
            }
            path[..pos].to_string()
        } else {
            "/".to_string()
        }
    }

    /// Extract the basename (final component) of a path.
    fn basename(path: &str) -> String {
        let path = path.trim_start_matches('/').trim_end_matches('/');
        path.rsplit('/').next().unwrap_or(path).to_string()
    }

    /// Get filesystem statistics.
    pub fn stats(&self) -> (u64, u64, u64, u64, u64, u64) {
        (
            self.superblock.total_blocks,
            self.superblock.free_blocks,
            self.superblock.total_inodes,
            self.superblock.free_inodes,
            self.superblock.first_data_block,
            self.superblock.total_size,
        )
    }

    /// Get the image path.
    pub fn image_path(&self) -> &Path {
        &self.image_path
    }

    /// Flush all changes to disk.
    pub fn flush(&mut self) -> Result<(), String> {
        self.file.flush().map_err(|e| format!("Flush: {}", e))
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use std::sync::atomic::{AtomicU64, Ordering};
    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_path() -> PathBuf {
        let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        PathBuf::from(format!("/tmp/test_mitefs_{}_{}.mite", std::process::id(), n))
    }

    #[test]
    fn test_create_and_open() {
        let path = temp_path();
        let _ = std::fs::remove_file(&path);
        {
            let mfs = MiteFS::create(&path, 16).unwrap();
            assert_eq!(mfs.superblock().magic, *MITE_MAGIC);
            assert_eq!(mfs.superblock().version, 1);
        }
        {
            let mfs = MiteFS::open(&path).unwrap();
            assert_eq!(mfs.superblock().root_inode, 1);
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_create_file_and_read() {
        let path = temp_path();
        let _ = std::fs::remove_file(&path);
        {
            let mut mfs = MiteFS::create(&path, 16).unwrap();
            mfs.create_file("/hello.txt", b"Hello, MiteOS!").unwrap();
        }
        {
            let mut mfs = MiteFS::open(&path).unwrap();
            let content = mfs.read_file_string("/hello.txt").unwrap();
            assert_eq!(content, "Hello, MiteOS!");
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_directory_operations() {
        let path = temp_path();
        let _ = std::fs::remove_file(&path);
        {
            let mut mfs = MiteFS::create(&path, 16).unwrap();
            mfs.create_dir("/apps").unwrap();
            mfs.create_file("/apps/test.bin", b"\x00\x01\x02").unwrap();
            let entries = mfs.list_dir("/apps").unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].1, "test.bin");
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_delete() {
        let path = temp_path();
        let _ = std::fs::remove_file(&path);
        {
            let mut mfs = MiteFS::create(&path, 16).unwrap();
            mfs.create_file("/to_delete.txt", b"bye").unwrap();
            assert!(mfs.exists("/to_delete.txt"));
            mfs.delete("/to_delete.txt").unwrap();
            assert!(!mfs.exists("/to_delete.txt"));
        }
        std::fs::remove_file(&path).ok();
    }
}
