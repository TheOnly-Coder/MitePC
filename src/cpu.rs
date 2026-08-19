/// Virtual CPU simulator with a custom instruction set architecture.
///
/// Architecture: Mite-16
/// - 16 general-purpose 32-bit registers (R0-R15)
/// - 32-bit addressing
/// - R0 is hardwired to zero (reads as 0, writes are ignored)
/// - R13 = Stack Pointer (SP)
/// - R14 = Link Register (LR, return address for calls)
/// - R15 = Program Counter (PC)
/// - Flags: Zero (Z), Carry (C), Negative (N), Overflow (V)
///
/// Instruction encoding: Each instruction is 8 bytes
///   [0:2]  Opcode (u16)
///   [2:4]  Destination register / immediate low (u16)
///   [4:6]  Source register / immediate high (u16)
///   [6:8]  Extra data / address high bits (u16)

use crate::ram::Ram;

/// Number of general-purpose registers.
const NUM_REGISTERS: usize = 16;

/// Instruction opcodes for the Mite-16 ISA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Opcode {
    Nop    = 0x00,
    Halt   = 0x01,
    // Data movement
    Mov    = 0x10,
    Load   = 0x11,
    Store  = 0x12,
    LoadI  = 0x13,  // Load immediate (32-bit)
    // Arithmetic
    Add    = 0x20,
    Sub    = 0x21,
    Mul    = 0x22,
    Div    = 0x23,
    Mod    = 0x24,
    Inc    = 0x25,
    Dec    = 0x26,
    // Bitwise
    And    = 0x30,
    Or     = 0x31,
    Xor    = 0x32,
    Not    = 0x33,
    Shl    = 0x34,
    Shr    = 0x35,
    // Comparison & Branch
    Cmp    = 0x40,
    Jmp    = 0x41,
    Jz     = 0x42,
    Jnz    = 0x43,
    Jc     = 0x44,
    Jnc    = 0x45,
    Jn     = 0x46,
    Jnn    = 0x47,
    Jgt    = 0x48,
    Jlt    = 0x49,
    Jge    = 0x4A,
    Jle    = 0x4B,
    // Subroutine
    Call   = 0x50,
    Ret    = 0x51,
    Push   = 0x52,
    Pop    = 0x53,
    // System
    Int    = 0x60,  // System call (interrupt)
    // I/O
    InB    = 0x70,  // Read byte from I/O port
    OutB   = 0x71,  // Write byte to I/O port
}

impl Opcode {
    fn from_u16(val: u16) -> Option<Self> {
        match val {
            0x00 => Some(Self::Nop),
            0x01 => Some(Self::Halt),
            0x10 => Some(Self::Mov),
            0x11 => Some(Self::Load),
            0x12 => Some(Self::Store),
            0x13 => Some(Self::LoadI),
            0x20 => Some(Self::Add),
            0x21 => Some(Self::Sub),
            0x22 => Some(Self::Mul),
            0x23 => Some(Self::Div),
            0x24 => Some(Self::Mod),
            0x25 => Some(Self::Inc),
            0x26 => Some(Self::Dec),
            0x30 => Some(Self::And),
            0x31 => Some(Self::Or),
            0x32 => Some(Self::Xor),
            0x33 => Some(Self::Not),
            0x34 => Some(Self::Shl),
            0x35 => Some(Self::Shr),
            0x40 => Some(Self::Cmp),
            0x41 => Some(Self::Jmp),
            0x42 => Some(Self::Jz),
            0x43 => Some(Self::Jnz),
            0x44 => Some(Self::Jc),
            0x45 => Some(Self::Jnc),
            0x46 => Some(Self::Jn),
            0x47 => Some(Self::Jnn),
            0x48 => Some(Self::Jgt),
            0x49 => Some(Self::Jlt),
            0x4A => Some(Self::Jge),
            0x4B => Some(Self::Jle),
            0x50 => Some(Self::Call),
            0x51 => Some(Self::Ret),
            0x52 => Some(Self::Push),
            0x53 => Some(Self::Pop),
            0x60 => Some(Self::Int),
            0x70 => Some(Self::InB),
            0x71 => Some(Self::OutB),
            _ => None,
        }
    }
}

/// CPU flags register.
#[derive(Debug, Clone, Copy, Default)]
pub struct Flags {
    pub zero: bool,
    pub carry: bool,
    pub negative: bool,
    pub overflow: bool,
}

/// A decoded instruction.
#[derive(Debug, Clone)]
struct Instruction {
    opcode: Opcode,
    rd: u16,   // destination register or immediate low
    rs: u16,   // source register or immediate high
    extra: u16, // extra data
}

/// The virtual CPU.
pub struct Cpu {
    /// General-purpose registers. R0 is always 0.
    registers: [u32; NUM_REGISTERS],
    /// Flags.
    flags: Flags,
    /// Whether the CPU is halted.
    halted: bool,
    /// Total instructions executed.
    instruction_count: u64,
    /// Clock speed in MHz (for display purposes).
    clock_mhz: u32,
    /// Pending system call number (set by INT instruction).
    pending_syscall: Option<u32>,
}

impl Cpu {
    /// Create a new CPU with the given clock speed in MHz.
    pub fn new(clock_mhz: u32) -> Self {
        let mut cpu = Self {
            registers: [0; NUM_REGISTERS],
            flags: Flags::default(),
            halted: false,
            instruction_count: 0,
            clock_mhz,
            pending_syscall: None,
        };
        // Set up initial stack pointer (high address)
        cpu.set_reg(13, 0xFFFE0000);
        cpu
    }

    /// Get the program counter.
    pub fn pc(&self) -> u32 {
        self.reg(15)
    }

    /// Set the program counter.
    pub fn set_pc(&mut self, val: u32) {
        self.set_reg(15, val);
    }

    /// Get the stack pointer.
    pub fn sp(&self) -> u32 {
        self.reg(13)
    }

    /// Set the stack pointer.
    pub fn set_sp(&mut self, val: u32) {
        self.set_reg(13, val);
    }

    /// Read a register value. R0 always returns 0.
    pub fn reg(&self, idx: u16) -> u32 {
        if idx == 0 {
            0
        } else if (idx as usize) < NUM_REGISTERS {
            self.registers[idx as usize]
        } else {
            0
        }
    }

    /// Write a register value. Writes to R0 are silently ignored.
    pub fn set_reg(&mut self, idx: u16, val: u32) {
        if idx != 0 && (idx as usize) < NUM_REGISTERS {
            self.registers[idx as usize] = val;
        }
    }

    /// Check if the CPU is halted.
    pub fn is_halted(&self) -> bool {
        self.halted
    }

    /// Halt the CPU.
    pub fn halt(&mut self) {
        self.halted = true;
    }

    /// Get total instructions executed.
    pub fn instruction_count(&self) -> u64 {
        self.instruction_count
    }

    /// Get clock speed in MHz.
    pub fn clock_mhz(&self) -> u32 {
        self.clock_mhz
    }

    /// Take the pending system call (if any).
    pub fn take_syscall(&mut self) -> Option<u32> {
        self.pending_syscall.take()
    }

    /// Set a register used for syscall arguments/results.
    /// Convention: R1 = syscall number (already taken), R2-R5 = args, R1 = result on return.
    pub fn syscall_arg(&self, idx: usize) -> u32 {
        self.reg((idx as u16) + 2)
    }

    pub fn set_syscall_result(&mut self, val: u32) {
        self.set_reg(1, val);
    }

    /// Fetch and decode one instruction from RAM at the current PC.
    fn fetch_decode(&self, ram: &Ram) -> Option<Instruction> {
        let pc = self.pc() as u64;
        let opcode = ram.read_u16(pc);
        let rd = ram.read_u16(pc + 2);
        let rs = ram.read_u16(pc + 4);
        let extra = ram.read_u16(pc + 6);
        Opcode::from_u16(opcode).map(|op| Instruction {
            opcode: op,
            rd,
            rs,
            extra,
        })
    }

    /// Update flags after an arithmetic result.
    fn update_flags(&mut self, result: u32) {
        self.flags.zero = result == 0;
        self.flags.negative = (result as i32) < 0;
        self.flags.carry = false; // simplified
        self.flags.overflow = false; // simplified
    }

    /// Execute a single instruction cycle.
    /// Returns true if a syscall was raised.
    pub fn step(&mut self, ram: &mut Ram) -> bool {
        if self.halted {
            return false;
        }

        let instr = match self.fetch_decode(ram) {
            Some(i) => i,
            None => {
                // Unknown opcode — halt
                self.halted = true;
                return false;
            }
        };

        // Advance PC by 8 bytes (instruction size)
        self.set_pc(self.pc().wrapping_add(8));
        self.instruction_count += 1;

        let mut raised_syscall = false;

        match instr.opcode {
            Opcode::Nop => {}

            Opcode::Halt => {
                self.halted = true;
            }

            Opcode::Mov => {
                let val = self.reg(instr.rs);
                self.set_reg(instr.rd, val);
            }

            Opcode::LoadI => {
                // Load 32-bit immediate: rd = (extra << 16) | rs
                let imm = ((instr.extra as u32) << 16) | (instr.rs as u32);
                self.set_reg(instr.rd, imm);
            }

            Opcode::Load => {
                // Load from memory: rd = mem[rs + extra*4]
                let addr = self.reg(instr.rs).wrapping_add((instr.extra as u32) * 4);
                let val = ram.read_u32(addr as u64);
                self.set_reg(instr.rd, val);
            }

            Opcode::Store => {
                // Store to memory: mem[rd + extra*4] = rs
                let addr = self.reg(instr.rd).wrapping_add((instr.extra as u32) * 4);
                let val = self.reg(instr.rs);
                ram.write_u32(addr as u64, val);
            }

            Opcode::Add => {
                let a = self.reg(instr.rd);
                let b = self.reg(instr.rs);
                let result = a.wrapping_add(b);
                self.set_reg(instr.rd, result);
                self.update_flags(result);
            }

            Opcode::Sub => {
                let a = self.reg(instr.rd);
                let b = self.reg(instr.rs);
                let result = a.wrapping_sub(b);
                self.set_reg(instr.rd, result);
                self.update_flags(result);
            }

            Opcode::Mul => {
                let a = self.reg(instr.rd);
                let b = self.reg(instr.rs);
                let result = a.wrapping_mul(b);
                self.set_reg(instr.rd, result);
                self.update_flags(result);
            }

            Opcode::Div => {
                let a = self.reg(instr.rd);
                let b = self.reg(instr.rs);
                if b != 0 {
                    let result = a / b;
                    self.set_reg(instr.rd, result);
                    self.update_flags(result);
                } else {
                    // Division by zero — halt
                    self.halted = true;
                }
            }

            Opcode::Mod => {
                let a = self.reg(instr.rd);
                let b = self.reg(instr.rs);
                if b != 0 {
                    let result = a % b;
                    self.set_reg(instr.rd, result);
                    self.update_flags(result);
                } else {
                    self.halted = true;
                }
            }

            Opcode::Inc => {
                let val = self.reg(instr.rd).wrapping_add(1);
                self.set_reg(instr.rd, val);
                self.update_flags(val);
            }

            Opcode::Dec => {
                let val = self.reg(instr.rd).wrapping_sub(1);
                self.set_reg(instr.rd, val);
                self.update_flags(val);
            }

            Opcode::And => {
                let result = self.reg(instr.rd) & self.reg(instr.rs);
                self.set_reg(instr.rd, result);
                self.update_flags(result);
            }

            Opcode::Or => {
                let result = self.reg(instr.rd) | self.reg(instr.rs);
                self.set_reg(instr.rd, result);
                self.update_flags(result);
            }

            Opcode::Xor => {
                let result = self.reg(instr.rd) ^ self.reg(instr.rs);
                self.set_reg(instr.rd, result);
                self.update_flags(result);
            }

            Opcode::Not => {
                let result = !self.reg(instr.rd);
                self.set_reg(instr.rd, result);
                self.update_flags(result);
            }

            Opcode::Shl => {
                let result = self.reg(instr.rd) << self.reg(instr.rs);
                self.set_reg(instr.rd, result);
                self.update_flags(result);
            }

            Opcode::Shr => {
                let result = self.reg(instr.rd) >> self.reg(instr.rs);
                self.set_reg(instr.rd, result);
                self.update_flags(result);
            }

            Opcode::Cmp => {
                let a = self.reg(instr.rd) as i32;
                let b = self.reg(instr.rs) as i32;
                let result = a.wrapping_sub(b);
                self.flags.zero = result == 0;
                self.flags.negative = result < 0;
                self.flags.carry = (self.reg(instr.rd) as u64) < (self.reg(instr.rs) as u64);
            }

            Opcode::Jmp => {
                let target = ((instr.extra as u32) << 16) | (instr.rd as u32);
                self.set_pc(target);
            }

            Opcode::Jz => {
                if self.flags.zero {
                    let target = ((instr.extra as u32) << 16) | (instr.rd as u32);
                    self.set_pc(target);
                }
            }

            Opcode::Jnz => {
                if !self.flags.zero {
                    let target = ((instr.extra as u32) << 16) | (instr.rd as u32);
                    self.set_pc(target);
                }
            }

            Opcode::Jc => {
                if self.flags.carry {
                    let target = ((instr.extra as u32) << 16) | (instr.rd as u32);
                    self.set_pc(target);
                }
            }

            Opcode::Jnc => {
                if !self.flags.carry {
                    let target = ((instr.extra as u32) << 16) | (instr.rd as u32);
                    self.set_pc(target);
                }
            }

            Opcode::Jn => {
                if self.flags.negative {
                    let target = ((instr.extra as u32) << 16) | (instr.rd as u32);
                    self.set_pc(target);
                }
            }

            Opcode::Jnn => {
                if !self.flags.negative {
                    let target = ((instr.extra as u32) << 16) | (instr.rd as u32);
                    self.set_pc(target);
                }
            }

            Opcode::Jgt => {
                // A > B (signed, after CMP rd, rs)
                if !self.flags.zero && !self.flags.negative {
                    let target = ((instr.extra as u32) << 16) | (instr.rd as u32);
                    self.set_pc(target);
                }
            }

            Opcode::Jlt => {
                if self.flags.negative {
                    let target = ((instr.extra as u32) << 16) | (instr.rd as u32);
                    self.set_pc(target);
                }
            }

            Opcode::Jge => {
                if !self.flags.negative || self.flags.zero {
                    let target = ((instr.extra as u32) << 16) | (instr.rd as u32);
                    self.set_pc(target);
                }
            }

            Opcode::Jle => {
                if self.flags.zero || self.flags.negative {
                    let target = ((instr.extra as u32) << 16) | (instr.rd as u32);
                    self.set_pc(target);
                }
            }

            Opcode::Call => {
                // Save return address to LR (R14)
                self.set_reg(14, self.pc());
                let target = ((instr.extra as u32) << 16) | (instr.rd as u32);
                self.set_pc(target);
            }

            Opcode::Ret => {
                self.set_pc(self.reg(14));
            }

            Opcode::Push => {
                let mut sp = self.sp();
                sp = sp.wrapping_sub(4);
                self.set_sp(sp);
                ram.write_u32(sp as u64, self.reg(instr.rd));
            }

            Opcode::Pop => {
                let sp = self.sp();
                let val = ram.read_u32(sp as u64);
                self.set_reg(instr.rd, val);
                self.set_sp(sp.wrapping_add(4));
            }

            Opcode::Int => {
                // System call: rd contains the syscall number
                let syscall_num = self.reg(instr.rd);
                self.pending_syscall = Some(syscall_num);
                raised_syscall = true;
            }

            Opcode::InB => {
                // Read from I/O port (simplified: just return 0)
                // In a full implementation, this would read from device registers
                self.set_reg(instr.rd, 0);
            }

            Opcode::OutB => {
                // Write to I/O port (simplified: no-op)
                // In a full implementation, this would write to device registers
            }
        }

        raised_syscall
    }

    /// Execute multiple cycles. Returns the number of syscalls raised.
    pub fn run_cycles(&mut self, ram: &mut Ram, cycles: u64) -> u32 {
        let mut syscalls = 0u32;
        for _ in 0..cycles {
            if self.halted {
                break;
            }
            if self.step(ram) {
                syscalls += 1;
            }
        }
        syscalls
    }

    /// Reset the CPU to initial state.
    pub fn reset(&mut self) {
        self.registers = [0; NUM_REGISTERS];
        self.flags = Flags::default();
        self.halted = false;
        self.instruction_count = 0;
        self.pending_syscall = None;
        self.set_reg(13, 0xFFFE0000); // Reset SP
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_basic() {
        let mut cpu = Cpu::new(800);
        assert_eq!(cpu.reg(0), 0); // R0 is always 0
        cpu.set_reg(1, 42);
        assert_eq!(cpu.reg(1), 42);
        cpu.set_reg(0, 99); // Writing to R0 is ignored
        assert_eq!(cpu.reg(0), 0);
    }

    #[test]
    fn test_cpu_halt() {
        let mut cpu = Cpu::new(800);
        let mut ram = Ram::new(1);
        // Write HALT instruction at address 0
        ram.write_u16(0, 0x0001); // Halt opcode
        cpu.set_pc(0);
        cpu.step(&mut ram);
        assert!(cpu.is_halted());
    }

    #[test]
    fn test_cpu_mov() {
        let mut cpu = Cpu::new(800);
        let mut ram = Ram::new(1);
        // MOV R1, R2
        cpu.set_reg(2, 123);
        ram.write_u16(0, 0x0010); // MOV
        ram.write_u16(2, 1);     // rd = R1
        ram.write_u16(4, 2);     // rs = R2
        cpu.set_pc(0);
        cpu.step(&mut ram);
        assert_eq!(cpu.reg(1), 123);
        assert_eq!(cpu.pc(), 8); // PC advanced
    }

    #[test]
    fn test_cpu_add() {
        let mut cpu = Cpu::new(800);
        let mut ram = Ram::new(1);
        // ADD R1, R2
        cpu.set_reg(1, 100);
        cpu.set_reg(2, 50);
        ram.write_u16(0, 0x0020); // ADD
        ram.write_u16(2, 1);     // rd = R1
        ram.write_u16(4, 2);     // rs = R2
        cpu.set_pc(0);
        cpu.step(&mut ram);
        assert_eq!(cpu.reg(1), 150);
    }

    #[test]
    fn test_cpu_loadi() {
        let mut cpu = Cpu::new(800);
        let mut ram = Ram::new(1);
        // LOADI R1, 0xBEEF
        ram.write_u16(0, 0x0013); // LOADI
        ram.write_u16(2, 1);      // rd = R1
        ram.write_u16(4, 0xBEEF); // imm low
        ram.write_u16(6, 0x0000); // imm high
        cpu.set_pc(0);
        cpu.step(&mut ram);
        assert_eq!(cpu.reg(1), 0xBEEF);
    }
}

// Add u16 read/write to Ram
impl Ram {
    pub fn read_u16(&self, addr: u64) -> u16 {
        let b0 = self.read_byte(addr) as u16;
        let b1 = self.read_byte(addr + 1) as u16;
        b0 | (b1 << 8)
    }

    pub fn write_u16(&mut self, addr: u64, value: u16) {
        self.write_byte(addr, (value & 0xFF) as u8);
        self.write_byte(addr + 1, ((value >> 8) & 0xFF) as u8);
    }
}
