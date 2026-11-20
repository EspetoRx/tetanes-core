//! Mapper 051 (11-in-1 Ball Games multicart).

#![allow(missing_docs)]

use crate::{
    cart::Cart,
    common::ResetKind,
    mapper::{self, Map, Mapper},
    memory::{Memory, Src},
    ppu::Mirroring,
};
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub enum BankMode {
    Mode16K,
    Mode32K,
    ModeSpecial,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[must_use]
pub struct Mapper51 {
    pub mirroring: Mirroring,
    pub mode: BankMode,
    pub bank_reg: u8,
    pub prg_16k_count: usize,
    pub chr_8k_count: usize,
    #[serde(skip)]
    pub write_count: u32,
}

impl Mapper51 {
    const PRG_16K: usize = 16 * 1024;
    const PRG_32K: usize = 32 * 1024;
    const CHR_8K: usize = 8 * 1024;

    pub fn load(cart: &mut Cart) -> Result<Mapper, mapper::Error> {
        let prg_16k_count = (cart.prg_rom_size / Self::PRG_16K).max(1);
        let chr_8k_count = (cart.chr_rom().len() / Self::CHR_8K).max(1);
        let mut board = Self {
            mirroring: cart.mirroring(),
            mode: BankMode::Mode16K,
            bank_reg: 0,
            prg_16k_count,
            chr_8k_count,
            write_count: 0,
        };
        info!(
            "[m051 load] prg_rom_size={} prg16k={} chr8k={} init_mir={:?}",
            cart.prg_rom_size, board.prg_16k_count, board.chr_8k_count, board.mirroring
        );
        board.update_banks(&mut cart.memory);
        Ok(board.into())
    }

    fn decode_write(&mut self, addr: u16, val: u8) -> bool {
        let old_reg = self.bank_reg;
        let old_mode = self.mode;
        let old_mir = self.mirroring;

        // Ajustar conforme o mapeamento exato de bits para a ROM/placa:
        // - bits de seleção de banco / bloco de PRG
        // - seleção entre modo 16K / 32K / modo especial
        // - bit de mirroring (Horizontal / Vertical) quando aplicável
        self.bank_reg = val;
        self.mode = if (val & 0x10) != 0 {
            BankMode::Mode32K
        } else {
            BankMode::Mode16K
        };

        let changed =
            old_reg != self.bank_reg || old_mode != self.mode || old_mir != self.mirroring;

        if self.write_count <= 32 || changed {
            info!(
                "[m051 write #{}] addr={addr:#06X} val={val:#04X} -> bank_reg={:#04X} mode={:?} mir={:?}",
                self.write_count, self.bank_reg, self.mode, self.mirroring
            );
        }

        changed
    }
}

impl Map for Mapper51 {
    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn reset(&mut self, kind: ResetKind) {
        self.mode = BankMode::Mode16K;
        self.bank_reg = 0;
        self.write_count = 0;
        info!(
            "[m051 reset] kind={kind:?} -> bank_reg={:#04X} mode={:?}",
            self.bank_reg, self.mode
        );
    }

    fn registers(&self, out: &mut Vec<(&'static str, u32)>) {
        out.push(("Bank Reg", u32::from(self.bank_reg)));
        out.push((
            "Mode",
            match self.mode {
                BankMode::Mode16K => 0,
                BankMode::Mode32K => 1,
                BankMode::ModeSpecial => 2,
            },
        ));
    }

    fn write_register(&mut self, memory: &mut Memory, addr: u16, val: u8) {
        // Mappers multicart dessa família costumam responder em $6000-$7FFF e/ou $8000-$FFFF.
        if (0x6000..=0xFFFF).contains(&addr) {
            self.write_count += 1;
            let _ = self.decode_write(addr, val);
            self.update_banks(memory);
        }
    }

    fn update_banks(&mut self, memory: &mut Memory) {
        memory.map_prg(0x6000, 8 * 1024, 0, Src::PrgRam);

        let prg_mask = (self.prg_16k_count.saturating_sub(1)) as u8;
        let chr_mask = (self.chr_8k_count.saturating_sub(1)) as u8;

        match self.mode {
            BankMode::Mode16K => {
                let bank16 = i32::from(self.bank_reg & prg_mask);
                let high16 = bank16 | 0x03; // ajuste conforme o bloco/página do PRG-ROM
                memory.map_prg(0x8000, Self::PRG_16K, bank16, Src::PrgRom);
                memory.map_prg(0xC000, Self::PRG_16K, high16, Src::PrgRom);
            }
            BankMode::Mode32K => {
                let bank32 = i32::from((self.bank_reg & prg_mask) >> 1);
                memory.map_prg(0x8000, Self::PRG_32K, bank32, Src::PrgRom);
            }
            BankMode::ModeSpecial => {
                let bank16 = i32::from(self.bank_reg & prg_mask);
                memory.map_prg(0x8000, Self::PRG_16K, bank16, Src::PrgRom);
                memory.map_prg(0xC000, Self::PRG_16K, bank16, Src::PrgRom);
            }
        }

        let chr_bank = i32::from(0u8 & chr_mask);
        memory.map_chr(0x0000, Self::CHR_8K, chr_bank, Src::Chr);
        memory.set_mirroring(self.mirroring);
    }
}
