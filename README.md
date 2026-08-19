# MitePC

A PC simulator written in Rust that simulates its own RAM, CPU, and Storage. Runs **MiteOS**, a minimal GUI-based operating system stored in the custom `.mite` file format.

**MiteOS is NOT based on Linux.** It is a completely custom operating system with its own filesystem (MiteFS), CPU architecture (Mite-16), and kernel (MiteMicro).

## Features

- **Simulated Hardware:**
  - **CPU:** Mite-16 architecture with 16 registers, custom ISA (40+ opcodes), flags, and syscall support
  - **RAM:** Sparse page-based memory simulation (4 KB pages, allocated on demand)
  - **Storage:** Custom MiteFS filesystem in `.mite` disk images

- **MiteOS (built-in operating system):**
  - Windowed GUI with desktop, taskbar, and window management
  - Built-in applications: File Manager, System Info, Terminal, Settings, About
  - Command-line terminal with commands: `ls`, `cd`, `cat`, `stat`, `mkdir`, `touch`, `rm`, `echo`, `clear`, `sysinfo`
  - Boot sequence with hardware detection

- **MiteFS Filesystem:**
  - Custom on-disk format (NOT ext, FAT, NTFS, or any existing FS)
  - Superblock, inode table, free block bitmap, data blocks with chaining
  - 256-byte inodes, 4096-byte blocks, up to 4096 files/directories

## Configuration

Edit `setup.conf` to customize the virtual hardware:

```ini
# RAM amount in MB (64 - 16384)
ram_mb = 1024

# Number of CPU cores (1 - 16)
cpu_cores = 1

# CPU clock speed in MHz (100 - 4000)
cpu_mhz = 800

# Storage capacity in MB (256 - 131072)
storage_mb = 4096

# Path to the .mite OS image (created automatically if missing)
os_image = ./miteos.mite

# Host directory for persistent storage
storage_dir = ./mite
```

## Building

```bash
cargo build --release
```

## Running

```bash
cargo run --release
```

On first run, MitePC will:
1. Create a `.mite` disk image (size from `setup.conf`)
2. Format it with MiteFS
3. Install default MiteOS files
4. Boot MiteOS with a splash screen

### Controls

| Key | Action |
|-----|--------|
| Enter | Select / Open / Confirm |
| Arrow keys | Navigate |
| Tab | Switch window focus |
| Esc | Close window / Go back |
| Ctrl+C | Exit simulator |
| Alt+F4 | Close focused window |
| Ctrl+W | Close focused window |

### MiteOS Desktop

- **Files** — Browse the MiteFS filesystem
- **System** — View hardware info (CPU, RAM, Storage usage)
- **Terminal** — Command-line interface with built-in commands
- **Settings** — System preferences (display)
- **About** — About MiteOS

## Architecture

```
MitePC Simulator
├── config.rs      — setup.conf parser
├── cpu.rs         — Mite-16 CPU simulator (registers, ALU, ISA)
├── ram.rs         — Sparse page-based RAM simulator
├── display.rs     — Text-mode display buffer (crossterm)
├── mitefs.rs      — MiteFS driver (.mite on-disk format)
├── miteos.rs      — MiteOS kernel, GUI, window manager, apps
├── simulator.rs   — Main simulation loop
└── main.rs        — Entry point
```

### Mite-16 CPU Architecture

- 16 general-purpose 32-bit registers (R0-R15)
- R0 hardwired to zero
- R13 = Stack Pointer, R14 = Link Register, R15 = Program Counter
- Flags: Zero, Carry, Negative, Overflow
- 8-byte fixed-width instruction encoding
- 40+ opcodes: data movement, arithmetic, bitwise, compare/branch, subroutine, I/O, syscall

### MiteFS Disk Layout

```
Block 0:       Superblock (4096 bytes)
Block 1..N:    Inode table (256 bytes per inode, 16 per block)
Block N..M:    Free block bitmap
Block M..End:  Data blocks (4096 bytes, linked list chains)
```

## License

MIT
