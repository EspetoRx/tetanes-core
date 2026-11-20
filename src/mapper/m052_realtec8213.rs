use crate::{
    cart::Cart,
    common::ResetKind,
    mapper::{self, Map, Mapper, MapperOps, mmc3::Mmc3},
    memory::{Memory, Src},
    ppu::Mirroring,
};
use serde::{Deserialize, Serialize};

/// Realtec 8213 / MMC3 multicart (Mapper 052).
///
/// Register at `$6000-$7FFF` (when WRAM is enabled and not write-protected via `$A001`):
/// - Bit 7 (`0x80`): lock bit (`1` prevents further writes to this register until reset/power-on)
/// - Bit 6 (`0x40`): CHR bank size / mask select (`0` = 256 KiB outer block, `1` = 128 KiB outer block)
/// - Bits `5..=3`: CHR outer bank bits
/// - Bit 2 (`0x04`): PRG bank size / mask select (`0` = 256 KiB outer block, `1` = 128 KiB outer block)
/// - Bits `1..=0`: PRG outer bank bits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mapper52 {
    /// Underlying MMC3 state.
    pub mmc3: Mmc3,
    /// Current nametable mirroring.
    pub mirroring: Mirroring,
    /// Whether cartridge WRAM is enabled.
    pub wram_enabled: bool,
    /// Whether cartridge WRAM accepts writes.
    pub wram_writable: bool,
    /// Multicart outer-bank selection register.
    pub outer_reg: u8,
    /// Whether the outer-bank register is locked until reset.
    pub locked: bool,
}

impl Mapper52 {
    /// Loads a Mapper 052 board from the given cartridge.
    pub fn load(cart: &mut Cart) -> Result<Mapper, mapper::Error> {
        let mut board = Self {
            mmc3: Mmc3::default(),
            mirroring: cart.mirroring(),
            wram_enabled: true,
            wram_writable: true,
            outer_reg: 0,
            locked: false,
        };
        board.update_banks(&mut cart.memory);
        Ok(board.into())
    }

    const fn prg_swap_mode(&self) -> bool {
        (self.mmc3.bank_select & 0x40) != 0
    }

    const fn chr_swap_mode(&self) -> bool {
        (self.mmc3.bank_select & 0x80) != 0
    }

    /// Returns `(base_bank_8k, bank_mask_8k)` for PRG banking.
    const fn prg_outer_window(&self) -> (i32, u8) {
        let small_block = (self.outer_reg & 0x04) != 0;
        let low_bits = (self.outer_reg & 0x03) as i32;
        if small_block {
            // 128 KiB block (16 banks of 8 KiB); bit 2 selects half of the 256 KiB pair.
            let bit2 = ((self.outer_reg >> 2) & 0x01) as i32;
            let base = (low_bits << 5) | (bit2 << 4);
            (base, 0x0F)
        } else {
            // 256 KiB block (32 banks of 8 KiB).
            let base = low_bits << 5;
            (base, 0x1F)
        }
    }

    /// Returns `(base_bank_1k, bank_mask_1k)` for CHR banking.
    const fn chr_outer_window(&self) -> (i32, u8) {
        let small_block = (self.outer_reg & 0x40) != 0;
        let chr_bits = ((self.outer_reg >> 3) & 0x07) as i32;
        if small_block {
            // 128 KiB CHR block (128 banks of 1 KiB); bit 6 selects half of the 256 KiB block.
            let bit6 = ((self.outer_reg >> 6) & 0x01) as i32;
            let base = (chr_bits << 8) | (bit6 << 7);
            (base, 0x7F)
        } else {
            // 256 KiB CHR block (256 banks of 1 KiB).
            let base = chr_bits << 8;
            (base, 0xFF)
        }
    }

    fn write_outer_reg(&mut self, memory: &mut Memory, val: u8) {
        if self.locked {
            return;
        }
        self.outer_reg = val;
        if (val & 0x80) != 0 {
            self.locked = true;
        }
        self.update_banks(memory);
    }
}

impl Map for Mapper52 {
    fn mapper_ops(&self) -> MapperOps {
        MapperOps::IRQ | MapperOps::WATCHES_PPU_BUS
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        self.mmc3.irq_pending
    }

    fn ppu_bus_addr(&mut self, _memory: &mut Memory, addr: u16) {
        self.mmc3.clock_irq(addr);
    }

    fn write_register(&mut self, memory: &mut Memory, addr: u16, val: u8) {
        if (0x6000..=0x7FFF).contains(&addr) {
            if self.wram_enabled && self.wram_writable {
                self.write_outer_reg(memory, val);
            }
            return;
        }

        match addr & 0xE001 {
            0x8000 => {
                self.mmc3.write_bank_select(val);
                self.update_banks(memory);
            }
            0x8001 => {
                self.mmc3.write_bank_data(val);
                self.update_banks(memory);
            }
            0xA000 => {
                self.mirroring = if (val & 0x01) != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
                memory.set_mirroring(self.mirroring);
            }
            0xA001 => {
                self.wram_enabled = (val & 0x80) != 0;
                self.wram_writable = (val & 0x40) == 0;
                self.update_banks(memory);
            }
            0xC000 => self.mmc3.write_irq_latch(val),
            0xC001 => self.mmc3.write_irq_reload(),
            0xE000 => self.mmc3.write_irq_disable(),
            0xE001 => self.mmc3.write_irq_enable(),
            _ => {}
        }
    }

    fn reset(&mut self, kind: ResetKind) {
        self.mmc3.reset(kind);
        self.outer_reg = 0;
        self.locked = false;
        self.wram_enabled = true;
        self.wram_writable = true;
    }

    fn update_banks(&mut self, memory: &mut Memory) {
        const K1: usize = 1024;
        const K2: usize = 2 * 1024;
        const K8: usize = 8 * 1024;

        // PRG-RAM at $6000-$7FFF
        if self.wram_enabled {
            memory.map_prg(0x6000, K8, 0, Src::PrgRam);
            memory.set_prg_writable(0x6000, K8, self.wram_writable);
        } else {
            memory.unmap_prg(0x6000, K8);
        }

        let (prg_base, prg_mask) = self.prg_outer_window();
        let r6 = i32::from(self.mmc3.bank_values[6] & prg_mask);
        let r7 = i32::from(self.mmc3.bank_values[7] & prg_mask);
        let fixed_second_last = i32::from(0xFE & prg_mask);
        let fixed_last = i32::from(0xFF & prg_mask);

        if self.prg_swap_mode() {
            // $8000 = fixed second-to-last, $A000 = R7, $C000 = R6, $E000 = fixed last
            memory.map_prg(0x8000, K8, prg_base | fixed_second_last, Src::PrgRom);
            memory.map_prg(0xA000, K8, prg_base | r7, Src::PrgRom);
            memory.map_prg(0xC000, K8, prg_base | r6, Src::PrgRom);
            memory.map_prg(0xE000, K8, prg_base | fixed_last, Src::PrgRom);
        } else {
            // $8000 = R6, $A000 = R7, $C000 = fixed second-to-last, $E000 = fixed last
            memory.map_prg(0x8000, K8, prg_base | r6, Src::PrgRom);
            memory.map_prg(0xA000, K8, prg_base | r7, Src::PrgRom);
            memory.map_prg(0xC000, K8, prg_base | fixed_second_last, Src::PrgRom);
            memory.map_prg(0xE000, K8, prg_base | fixed_last, Src::PrgRom);
        }

        let (chr_base, chr_mask) = self.chr_outer_window();
        let r0 = i32::from((self.mmc3.bank_values[0] & !0x01) & chr_mask);
        let r1 = i32::from((self.mmc3.bank_values[1] & !0x01) & chr_mask);
        let r2 = i32::from(self.mmc3.bank_values[2] & chr_mask);
        let r3 = i32::from(self.mmc3.bank_values[3] & chr_mask);
        let r4 = i32::from(self.mmc3.bank_values[4] & chr_mask);
        let r5 = i32::from(self.mmc3.bank_values[5] & chr_mask);

        if self.chr_swap_mode() {
            // CHR A12 inverted: R2..R5 at $0000-$0FFF, R0..R1 at $1000-$1FFF
            memory.map_chr(0x0000, K1, chr_base | r2, Src::Chr);
            memory.map_chr(0x0400, K1, chr_base | r3, Src::Chr);
            memory.map_chr(0x0800, K1, chr_base | r4, Src::Chr);
            memory.map_chr(0x0C00, K1, chr_base | r5, Src::Chr);
            memory.map_chr(0x1000, K2, (chr_base >> 1) | (r0 >> 1), Src::Chr);
            memory.map_chr(0x1800, K2, (chr_base >> 1) | (r1 >> 1), Src::Chr);
        } else {
            // Standard CHR: R0..R1 at $0000-$0FFF, R2..R5 at $1000-$1FFF
            memory.map_chr(0x0000, K2, (chr_base >> 1) | (r0 >> 1), Src::Chr);
            memory.map_chr(0x0800, K2, (chr_base >> 1) | (r1 >> 1), Src::Chr);
            memory.map_chr(0x1000, K1, chr_base | r2, Src::Chr);
            memory.map_chr(0x1400, K1, chr_base | r3, Src::Chr);
            memory.map_chr(0x1800, K1, chr_base | r4, Src::Chr);
            memory.map_chr(0x1C00, K1, chr_base | r5, Src::Chr);
        }

        memory.set_mirroring(self.mirroring);
    }
}
