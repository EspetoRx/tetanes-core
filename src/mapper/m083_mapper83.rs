use crate::{
    cart::Cart,
    common::ResetKind,
    mapper::{self, Map, Mapper, MapperOps},
    memory::{Memory, PAGE_SIZE, Src},
    ppu::Mirroring,
};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

static M83_LOGS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn push_m83_log(line: String) {
    let lock = M83_LOGS.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut buf) = lock.lock() {
        if buf.len() < 128 {
            buf.push(line);
        }
    }
}

/// Drains buffered Mapper 83 debug log lines for forwarding to the frontend.
pub fn take_m83_logs() -> Vec<String> {
    let Some(lock) = M83_LOGS.get() else {
        return Vec::new();
    };
    let Ok(mut buf) = lock.lock() else {
        return Vec::new();
    };
    std::mem::take(&mut *buf)
}

/// iNES Mapper 083 (Cony / Yoko multicart & single-game boards).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mapper83 {
    /// Current nametable mirroring mode.
    pub mirroring: Mirroring,
    /// Outer bank base register (`$8000` / `$5100..=$5103`).
    pub prg_base: u8,
    /// Board mode register (`$8100`) controlling banking, mirroring, and IRQ enable bit.
    pub mode: u8,
    /// 8 KiB PRG-ROM bank registers (`$8300..=$8303` for `$8000`, `$A000`, `$C000`, `$6000`).
    pub prg_regs: [u8; 4],
    /// 1 KiB CHR-ROM bank registers (`$8310..=$8317`).
    pub chr_regs: [u8; 8],
    /// Current 16-bit IRQ counter value.
    pub irq_counter: u16,
    /// Whether IRQ counting is currently enabled.
    pub irq_enabled: bool,
    /// Whether an IRQ is currently pending acknowledgement.
    pub irq_pending: bool,
    /// Whether the IRQ counter is clocked by PPU A12 rising edges instead of CPU M2 cycles.
    pub irq_ppu_clock: bool,
    /// Previous PPU A12 line state for edge detection.
    pub last_a12: bool,
}

impl Default for Mapper83 {
    /// Creates a `Mapper83` instance with default power-on register values.
    fn default() -> Self {
        Self {
            mirroring: Mirroring::Vertical,
            prg_base: 0,
            mode: 0,
            prg_regs: [0, 1, 2, 0],
            chr_regs: [0, 1, 2, 3, 4, 5, 6, 7],
            irq_counter: 0,
            irq_enabled: false,
            irq_pending: false,
            irq_ppu_clock: false,
            last_a12: false,
        }
    }
}

impl Mapper83 {
    /// Number of 1 KiB memory pages inside an 8 KiB PRG bank.
    const BANK_8K_PAGES: usize = 8 * 1024 / PAGE_SIZE;

    /// Loads a Mapper 083 cartridge and configures its initial memory mappings.
    pub fn load(cart: &mut Cart) -> Result<Mapper, mapper::Error> {
        let mut board = Self {
            mirroring: cart.mirroring(),
            ..Self::default()
        };
        board.update_banks(&mut cart.memory);
        Ok(board.into())
    }

    /// Decodes the nametable mirroring mode from bits 1..0 of `mode`.
    fn decode_mirroring(&self) -> Mirroring {
        match self.mode & 0x03 {
            0 => Mirroring::Vertical,
            1 => Mirroring::Horizontal,
            2 => Mirroring::SingleScreenA,
            _ => Mirroring::SingleScreenB,
        }
    }

    /// Resolves the 256 KiB outer block index (in 8 KiB units -> `block * 32`).
    fn outer_block_256k(&self) -> usize {
        if (self.prg_base & 0x03) != 0 {
            usize::from(self.prg_base & 0x03)
        } else {
            usize::from((self.prg_base >> 5) & 0x03)
        }
    }

    /// Whether IRQ counting is enabled in the mode register (bit 7).
    fn mode_irq_enabled(&self) -> bool {
        (self.mode & 0x80) != 0
    }

    /// Advances the 16-bit IRQ counter when enabled and non-zero, raising IRQ on reaching 0.
    fn clock_irq_counter(&mut self) {
        if !self.irq_enabled || self.irq_counter == 0 {
            return;
        }

        let count_up = (self.mode & 0x08) != 0;
        if count_up {
            self.irq_counter = self.irq_counter.wrapping_add(1);
        } else {
            self.irq_counter -= 1;
        }

        if self.irq_counter == 0 {
            self.irq_enabled = false;
            self.irq_pending = true;
        }
    }

    /// Maps an 8 KiB PRG-ROM bank starting at `cpu_addr`.
    fn map_8k_prg(&self, memory: &mut Memory, cpu_addr: u16, bank_8k: usize) {
        let base_1k = (bank_8k * Self::BANK_8K_PAGES) as i32;
        for i in 0..Self::BANK_8K_PAGES {
            let page_addr = cpu_addr + (i * PAGE_SIZE) as u16;
            memory.map_prg(page_addr, PAGE_SIZE, base_1k + i as i32, Src::PrgRom);
        }
    }

    /// Maps a 1 KiB CHR bank at `ppu_addr`.
    fn map_1k_chr(&self, memory: &mut Memory, ppu_addr: u16, bank_1k: usize) {
        memory.map_chr(ppu_addr, PAGE_SIZE, bank_1k as i32, Src::Chr);
    }
}

impl Map for Mapper83 {
    /// Returns the optional hooks used by this mapper.
    fn mapper_ops(&self) -> MapperOps {
        MapperOps::CLOCKED
            | MapperOps::IRQ
            | MapperOps::WATCHES_PPU_BUS
            | MapperOps::SERVES_PRG_READS
    }

    /// Returns the active nametable mirroring mode.
    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    /// Returns whether an IRQ is currently pending.
    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    /// Appends current register values to `out` for debugging.
    fn registers(&self, out: &mut Vec<(&'static str, u32)>) {
        out.push(("prg_base", u32::from(self.prg_base)));
        out.push(("mode", u32::from(self.mode)));
        out.push(("prg0", u32::from(self.prg_regs[0])));
        out.push(("prg1", u32::from(self.prg_regs[1])));
        out.push(("prg2", u32::from(self.prg_regs[2])));
        out.push(("prg3", u32::from(self.prg_regs[3])));
        out.push(("irq_cnt", u32::from(self.irq_counter)));
        out.push(("irq_en", u32::from(self.irq_enabled)));
        out.push(("irq_pend", u32::from(self.irq_pending)));
        out.push(("irq_ppu", u32::from(self.irq_ppu_clock)));
    }

    /// Handles CPU reads from mapper-mapped/expansion addresses (`$5100..=$5103`).
    fn prg_read(&mut self, addr: u16) -> Option<u8> {
        if (0x5100..=0x5103).contains(&addr) {
            let val = 0x60 | (self.prg_base & 0x03);
            push_m83_log(format!(
                "[M83::read] addr=${addr:04X} -> ${val:02X} (base=${:02X})",
                self.prg_base
            ));
            return Some(val);
        }
        None
    }

    /// Handles a CPU register write and updates banking.
    fn write_register(&mut self, memory: &mut Memory, addr: u16, val: u8) {
        push_m83_log(format!(
            "[M83::write] addr=${addr:04X} val=${val:02X} base=${:02X} mode=${:02X} p0=${:02X} p1=${:02X} p2=${:02X} irq_cnt=${:04X} en={} pend={}",
            self.prg_base,
            self.mode,
            self.prg_regs[0],
            self.prg_regs[1],
            self.prg_regs[2],
            self.irq_counter,
            u8::from(self.irq_enabled),
            u8::from(self.irq_pending),
        ));

        if (0x5000..=0x5FFF).contains(&addr) {
            self.prg_base = val;
            self.update_banks(memory);
            return;
        }

        if addr < 0x8000 {
            return;
        }

        match addr & 0x8300 {
            0x8000 => {
                self.prg_base = val;
            }
            0x8100 => {
                self.mode = val;
                self.mirroring = self.decode_mirroring();
                if !self.mode_irq_enabled() {
                    self.irq_enabled = false;
                }
            }
            0x8200 => {
                if (addr & 0x01) == 0 {
                    // LSB write: updates low byte and acknowledges pending IRQ.
                    self.irq_counter = (self.irq_counter & 0xFF00) | u16::from(val);
                    self.irq_pending = false;
                } else {
                    // MSB write: updates high byte and enables counting if mode bit 7 is set.
                    self.irq_counter = (u16::from(val) << 8) | (self.irq_counter & 0x00FF);
                    if self.mode_irq_enabled() {
                        self.irq_enabled = true;
                    }
                }
            }
            0x8300 => {
                match addr & 0x8318 {
                    0x8300 => {
                        let slot = (addr & 0x03) as usize;
                        self.prg_regs[slot] = val;
                    }
                    0x8310 => {
                        let slot = (addr & 0x07) as usize;
                        self.chr_regs[slot] = val;
                    }
                    0x8318 => {
                        self.irq_ppu_clock = (val & 0x01) != 0;
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        self.update_banks(memory);
    }

    /// Observes PPU bus addresses for A12 rising-edge IRQ clocking.
    fn ppu_bus_addr(&mut self, _memory: &mut Memory, addr: u16) {
        let a12 = (addr & 0x1000) != 0;
        if self.irq_ppu_clock && a12 && !self.last_a12 {
            self.clock_irq_counter();
        }
        self.last_a12 = a12;
    }

    /// Clocks the IRQ counter once per CPU cycle when in CPU clock mode.
    fn clock(&mut self) {
        if !self.irq_ppu_clock {
            self.clock_irq_counter();
        }
    }

    /// Resets mapper registers and IRQ state.
    fn reset(&mut self, _kind: ResetKind) {
        self.prg_base = 0;
        self.mode = 0;
        self.prg_regs = [0, 1, 2, 0];
        self.chr_regs = [0, 1, 2, 3, 4, 5, 6, 7];
        self.irq_enabled = false;
        self.irq_pending = false;
        self.irq_counter = 0;
        self.irq_ppu_clock = false;
        self.last_a12 = false;
    }

    /// Rebuilds PRG, CHR, and mirroring page mappings from current registers.
    fn update_banks(&mut self, memory: &mut Memory) {
        let outer_8k = self.outer_block_256k() * 32;
        let use_8k_mode = (self.mode & 0x10) != 0;

        if use_8k_mode {
            self.map_8k_prg(
                memory,
                0x8000,
                outer_8k + usize::from(self.prg_regs[0] & 0x1F),
            );
            self.map_8k_prg(
                memory,
                0xA000,
                outer_8k + usize::from(self.prg_regs[1] & 0x1F),
            );
            self.map_8k_prg(
                memory,
                0xC000,
                outer_8k + usize::from(self.prg_regs[2] & 0x1F),
            );
            self.map_8k_prg(memory, 0xE000, outer_8k + 31);
        } else {
            let bank16_lo = usize::from(self.prg_regs[0] & 0x0F) * 2;
            self.map_8k_prg(memory, 0x8000, outer_8k + bank16_lo);
            self.map_8k_prg(memory, 0xA000, outer_8k + bank16_lo + 1);
            self.map_8k_prg(memory, 0xC000, outer_8k + 30);
            self.map_8k_prg(memory, 0xE000, outer_8k + 31);
        }

        if (self.mode & 0x40) != 0 {
            self.map_8k_prg(
                memory,
                0x6000,
                outer_8k + usize::from(self.prg_regs[3] & 0x1F),
            );
        }

        let chr_outer_1k = self.outer_block_256k() * 256;
        for (i, &reg) in self.chr_regs.iter().enumerate() {
            let ppu_addr = (i * PAGE_SIZE) as u16;
            self.map_1k_chr(memory, ppu_addr, chr_outer_1k + usize::from(reg));
        }

        memory.set_mirroring(self.mirroring);
    }
}
