//! Mapper 184 (Sunsoft-1).

#![allow(missing_docs)]

use crate::{
    cart::Cart,
    mapper::{self, Map, Mapper},
    memory::{Memory, Src},
    ppu::Mirroring,
};
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[must_use]
pub struct Mapper184 {
    pub mirroring: Mirroring,
    pub chr_low_4k: u8,
    pub chr_high_4k: u8,
    pub prg_16k_count: usize,
    pub chr_4k_count: usize,
    #[serde(skip)]
    pub write_count: u32,
}

impl Mapper184 {
    const PRG_16K: usize = 16 * 1024;
    const CHR_4K: usize = 4 * 1024;

    pub fn load(cart: &mut Cart) -> Result<Mapper, mapper::Error> {
        let prg_16k_count = (cart.prg_rom_size / Self::PRG_16K).max(1);
        let chr_4k_count = (cart.chr_rom().len() / Self::CHR_4K).max(1);
        let board = Self {
            mirroring: cart.mirroring(),
            chr_low_4k: 0,
            chr_high_4k: 4,
            prg_16k_count,
            chr_4k_count,
            write_count: 0,
        };
        info!(
            "[m184 load] prg16k={} chr4k={} init_mir={:?}",
            board.prg_16k_count, board.chr_4k_count, board.mirroring
        );
        Ok(board.into())
    }
}

impl Map for Mapper184 {
    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn registers(&self, out: &mut Vec<(&'static str, u32)>) {
        out.push(("CHR 4K Low", u32::from(self.chr_low_4k)));
        out.push(("CHR 4K High", u32::from(self.chr_high_4k)));
    }

    fn write_register(&mut self, memory: &mut Memory, addr: u16, val: u8) {
        // Registrador mapeado na faixa $6000-$7FFF
        if !(0x6000..=0x7FFF).contains(&addr) {
            return;
        }

        self.write_count += 1;
        let old_low = self.chr_low_4k;
        let old_high = self.chr_high_4k;

        // Bits 2..0 -> banco CHR 4K em $0000-$0FFF
        // Bits 6..4 -> banco CHR 4K em $1000-$1FFF (na placa Sunsoft-1, bancos 4..7)
        self.chr_low_4k = val & 0x07;
        self.chr_high_4k = (val >> 4) & 0x07;

        if self.write_count <= 32 || old_low != self.chr_low_4k || old_high != self.chr_high_4k {
            info!(
                "[m184 write #{}] addr={:#06X} val={:#04X} -> chr_lo={} chr_hi={}",
                self.write_count, addr, val, self.chr_low_4k, self.chr_high_4k
            );
        }

        self.update_banks(memory);
    }

    fn update_banks(&mut self, memory: &mut Memory) {
        let chr_mask = (self.chr_4k_count.saturating_sub(1)) as u8;
        let low_bank = i32::from(self.chr_low_4k & chr_mask);
        let high_bank = i32::from(self.chr_high_4k & chr_mask);

        // PRG-ROM: $8000-$BFFF no banco 0 (ou último se houver apenas 1) e $C000-$FFFF no último banco de 16K
        let last_16k = i32::try_from(self.prg_16k_count.saturating_sub(1)).unwrap_or(0);
        let first_16k = if self.prg_16k_count >= 2 { 0 } else { last_16k };
        memory.map_prg(0x8000, Self::PRG_16K, first_16k, Src::PrgRom);
        memory.map_prg(0xC000, Self::PRG_16K, last_16k, Src::PrgRom);

        // CHR-ROM: duas janelas de 4 KiB
        memory.map_chr(0x0000, Self::CHR_4K, low_bank, Src::Chr);
        memory.map_chr(0x1000, Self::CHR_4K, high_bank, Src::Chr);

        memory.set_mirroring(self.mirroring);
    }
}
