//! Mapper 37 - MMC3 multicart (`Super Mario Bros. / Tetris / Nintendo World Cup`).

use crate::{
    cart::Cart,
    common::ResetKind,
    mapper::{Map, Mapper, MapperOps, Mmc3},
    memory::{Memory, Src},
    ppu::Mirroring,
};
use serde::{Deserialize, Serialize};

/// Mapper 37 (`MMC3 Multicart`) board state.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[must_use]
pub struct Mapper37 {
    /// Shared `MMC3` register file and IRQ counter.
    pub mmc3: Mmc3,
    /// Current nametable mirroring mode.
    pub mirroring: Mirroring,
    /// Selected outer multicart register (`$6000-$7FFF`: bits 0..1 PRG block, bit 2 CHR block).
    pub outer_bank: u8,
}

impl Mapper37 {
    const PRG_8K_WINDOW: usize = 0x2000;
    const CHR_1K_WINDOW: usize = 0x0400;
    const INITIAL_OUTER_BANK: u8 = 0x05;

    /// Loads a Mapper 37 cartridge into a [`Mapper`].
    pub fn load(cart: &mut Cart) -> std::result::Result<Mapper, crate::mapper::Error> {
        let mut board = Self {
            mmc3: Mmc3::default(),
            mirroring: cart.mirroring(),
            outer_bank: Self::INITIAL_OUTER_BANK,
        };
        board.update_banks(&mut cart.memory);
        Ok(board.into())
    }

    /// Returns `(prg_base_8k, prg_inner_mask, chr_base_1k, chr_inner_mask)`.
    /// Para 256 KiB PRG (32 x 8K) e 256 KiB CHR (256 x 1K):
    /// - bits 0..1 selecionam o bloco PRG (0: 0..128K, 1: 128..256K, 2: 128..192K, 3: 192..256K).
    /// - bit 2 seleciona o bloco CHR de 128 KiB (0..127 ou 128..255).
    const fn outer_block_params(&self) -> (usize, usize, usize, usize) {
        let chr_base = if (self.outer_bank & 0x04) != 0 { 128 } else { 0 };
        match self.outer_bank & 0x03 {
            0 => (0, 0x0F, chr_base, 0x7F),
            1 => (16, 0x0F, chr_base, 0x7F),
            2 => (16, 0x07, chr_base, 0x7F),
            _ => (24, 0x07, chr_base, 0x7F),
        }
    }

    fn clamp_prg_bank(memory: &Memory, bank: usize) -> i32 {
        let total_banks = (memory.region_ref(Src::PrgRom).len() / Self::PRG_8K_WINDOW).max(1);
        (bank % total_banks) as i32
    }

    fn clamp_chr_bank(memory: &Memory, bank: usize) -> i32 {
        let chr_len = memory.region_ref(Src::Chr).len();
        if chr_len == 0 {
            return 0;
        }
        let total_banks = (chr_len / Self::CHR_1K_WINDOW).max(1);
        (bank % total_banks) as i32
    }
}

impl Map for Mapper37 {
    fn mapper_ops(&self) -> MapperOps {
        MapperOps::CLOCKED | MapperOps::IRQ | MapperOps::WATCHES_PPU_BUS
    }

    fn irq_pending(&self) -> bool {
        self.mmc3.irq_pending
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn registers(&self, out: &mut Vec<(&'static str, u32)>) {
        out.extend_from_slice(&[
            ("outer_bank", u32::from(self.outer_bank)),
            ("bank_select", u32::from(self.mmc3.bank_select)),
            ("r6", u32::from(self.mmc3.bank_values[6])),
            ("r7", u32::from(self.mmc3.bank_values[7])),
            ("irq_latch", u32::from(self.mmc3.irq_latch)),
            ("irq_counter", u32::from(self.mmc3.irq_counter)),
            ("irq_enabled", u32::from(self.mmc3.irq_enabled)),
        ]);
    }

    fn write_register(&mut self, memory: &mut Memory, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                self.outer_bank = val & 0x07;
                self.update_banks(memory);
            }
            0x8000..=0xFFFF => {
                let even = (addr & 0x0001) == 0;
                match addr {
                    0x8000..=0x9FFF => {
                        if even {
                            self.mmc3.write_bank_select(val);
                        } else {
                            self.mmc3.write_bank_data(val);
                        }
                        self.update_banks(memory);
                    }
                    0xA000..=0xBFFF => {
                        if even {
                            self.mirroring = if (val & 0x01) == 0 {
                                Mirroring::Vertical
                            } else {
                                Mirroring::Horizontal
                            };
                            memory.set_mirroring(self.mirroring);
                        }
                    }
                    0xC000..=0xDFFF => {
                        if even {
                            self.mmc3.write_irq_latch(val);
                        } else {
                            self.mmc3.write_irq_reload();
                        }
                    }
                    0xE000..=0xFFFF => {
                        if even {
                            self.mmc3.write_irq_disable();
                        } else {
                            self.mmc3.write_irq_enable();
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn ppu_bus_addr(&mut self, _memory: &mut Memory, addr: u16) {
        self.mmc3.clock_irq(addr);
    }

    fn update_banks(&mut self, memory: &mut Memory) {
        let (prg_base, prg_mask, chr_base, chr_mask) = self.outer_block_params();
        let prg_mode_swap = (self.mmc3.bank_select & 0x40) != 0;
        let chr_invert = (self.mmc3.bank_select & 0x80) != 0;

        let r6 = (self.mmc3.bank_values[6] as usize) & prg_mask;
        let r7 = (self.mmc3.bank_values[7] as usize) & prg_mask;
        let second_last = prg_mask.saturating_sub(1);
        let last = prg_mask;

        let (b0, b1, b2, b3) = if prg_mode_swap {
            (second_last, r7, r6, last)
        } else {
            (r6, r7, second_last, last)
        };

        memory.map_prg(
            0x8000,
            Self::PRG_8K_WINDOW,
            Self::clamp_prg_bank(memory, prg_base + b0),
            Src::PrgRom,
        );
        memory.map_prg(
            0xA000,
            Self::PRG_8K_WINDOW,
            Self::clamp_prg_bank(memory, prg_base + b1),
            Src::PrgRom,
        );
        memory.map_prg(
            0xC000,
            Self::PRG_8K_WINDOW,
            Self::clamp_prg_bank(memory, prg_base + b2),
            Src::PrgRom,
        );
        memory.map_prg(
            0xE000,
            Self::PRG_8K_WINDOW,
            Self::clamp_prg_bank(memory, prg_base + b3),
            Src::PrgRom,
        );

        let c0 = ((self.mmc3.bank_values[0] & !0x01) as usize) & chr_mask;
        let c1 = ((self.mmc3.bank_values[1] & !0x01) as usize) & chr_mask;
        let c2 = (self.mmc3.bank_values[2] as usize) & chr_mask;
        let c3 = (self.mmc3.bank_values[3] as usize) & chr_mask;
        let c4 = (self.mmc3.bank_values[4] as usize) & chr_mask;
        let c5 = (self.mmc3.bank_values[5] as usize) & chr_mask;

        let chr_banks: [usize; 8] = if chr_invert {
            [c2, c3, c4, c5, c0, c0 + 1, c1, c1 + 1]
        } else {
            [c0, c0 + 1, c1, c1 + 1, c2, c3, c4, c5]
        };

        for (i, bank) in chr_banks.into_iter().enumerate() {
            let addr = (i * Self::CHR_1K_WINDOW) as u16;
            memory.map_chr(
                addr,
                Self::CHR_1K_WINDOW,
                Self::clamp_chr_bank(memory, chr_base + bank),
                Src::Chr,
            );
        }

        memory.set_mirroring(self.mirroring);
    }

    fn clock(&mut self) {
        self.mmc3.clock();
    }

    fn reset(&mut self, kind: ResetKind) {
        self.mmc3.reset(kind);
        self.outer_bank = Self::INITIAL_OUTER_BANK;
    }
}
