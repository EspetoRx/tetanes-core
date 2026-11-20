use crate::{
    cart::Cart,
    common::ResetKind,
    mapper::{self, Map, Mapper},
    memory::{Memory, Src},
    ppu::Mirroring,
};
use serde::{Deserialize, Serialize};

/// Operating mode selected by soft-reset toggling on Mapper 230 boards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    /// Standard multicart menu and game selection mode.
    Multicart,
    /// Dedicated `Contra` mode selected after a soft reset.
    Contra,
}


/// Mapper 230 (`22-in-1` / multicart with `Contra` mode) board state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct M230 {
    /// Initial cart nametable mirroring configuration.
    pub mirroring: Mirroring,
    /// Active operating mode (`Multicart` or `Contra`).
    pub mode: Mode,
    /// Latched register value written to `$8000-$FFFF`.
    pub reg: u8,
    /// Number of 16 KiB PRG-ROM banks available on the cartridge.
    pub prg_16k_count: usize,
}


impl M230 {
    const PRG_16K: usize = 0x4000;
    const CHR_8K: usize = 0x2000;

    /// Loads a Mapper 230 cartridge into a [`Mapper`].
    pub fn load(cart: &mut Cart) -> Result<Mapper, mapper::Error> {
        let prg_16k_count = (cart.prg_rom().len() / Self::PRG_16K).max(1);

        let mut board = Self {
            mirroring: cart.mirroring(),
            mode: Mode::Multicart,
            reg: 0,
            prg_16k_count,
        };
        board.update_banks(&mut cart.memory);
        Ok(board.into())
    }

    fn wrap_16k(&self, bank: usize) -> i32 {
        (bank % self.prg_16k_count) as i32
    }

    fn multicart_base_16k(&self) -> usize {
        // Em dumps 22-in-1 de 640 KiB (40 x 16 KiB):
        // bit 7 == 0 -> janela final de 128 KiB (bancos 32..=39, onde costuma estar o menu)
        // bit 7 == 1 -> janela inicial de 512 KiB (bancos 0..=31)
        if self.prg_16k_count > 32 && (self.reg & 0x80) == 0 {
            32
        } else {
            0
        }
    }
}

impl Map for M230 {
    fn mirroring(&self) -> Mirroring {
        match self.mode {
            Mode::Contra => Mirroring::Vertical,
            Mode::Multicart => {
                if (self.reg & 0x40) != 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                }
            }
        }
    }

    fn write_register(&mut self, memory: &mut Memory, addr: u16, val: u8) {
        if addr >= 0x8000 {
            self.reg = val;
            self.update_banks(memory);
        }
    }

    fn reset(&mut self, kind: ResetKind) {
        if matches!(kind, ResetKind::Soft) {
            self.mode = match self.mode {
                Mode::Multicart => Mode::Contra,
                Mode::Contra => Mode::Multicart,
            };
        }
        self.reg = 0;
    }

    fn registers(&self, out: &mut Vec<(&'static str, u32)>) {
        out.push((
            "Mode",
            match self.mode {
                Mode::Multicart => 0,
                Mode::Contra => 1,
            },
        ));
        out.push(("Reg", u32::from(self.reg)));
    }

    fn update_banks(&mut self, memory: &mut Memory) {
        let active_mirroring = self.mirroring();
        memory.set_mirroring(active_mirroring);
        memory.map_chr(0x0000, Self::CHR_8K, 0, Src::Chr);

        match self.mode {
            Mode::Multicart => {
                let base = self.multicart_base_16k();
                let is_16k_mode = (self.reg & 0x20) != 0;
                let raw_bank = usize::from(self.reg & 0x1F);

                if is_16k_mode {
                    let b = self.wrap_16k(base + raw_bank);
                    memory.map_prg(0x8000, Self::PRG_16K, b, Src::PrgRom);
                    memory.map_prg(0xC000, Self::PRG_16K, b, Src::PrgRom);
                } else {
                    let base_32k = base + (raw_bank & !1);
                    let b0 = self.wrap_16k(base_32k);
                    let b1 = self.wrap_16k(base_32k + 1);
                    memory.map_prg(0x8000, Self::PRG_16K, b0, Src::PrgRom);
                    memory.map_prg(0xC000, Self::PRG_16K, b1, Src::PrgRom);
                }
            }
            Mode::Contra => {
                let b_low = self.wrap_16k(usize::from(self.reg & 0x07));
                let b_high = self.wrap_16k(7);
                memory.map_prg(0x8000, Self::PRG_16K, b_low, Src::PrgRom);
                memory.map_prg(0xC000, Self::PRG_16K, b_high, Src::PrgRom);
            }
        }
    }
}
