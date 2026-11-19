//! `BMC 1200-in-1` (Mapper 227), an address-latch multicart.

#![allow(missing_docs)]

use crate::{
    cart::Cart,
    mapper::{self, Map, Mapper},
    memory::{Memory, Src},
    ppu::Mirroring,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[must_use]
pub struct Mapper227 {
    register: u16,
}

impl Mapper227 {
    const PRG_WINDOW: usize = 16 * 1024;
    const CHR_WINDOW: usize = 8 * 1024;

    pub fn load(cart: &mut Cart) -> Result<Mapper, mapper::Error> {
        let mut board = Self { register: 0x8000 };
        board.update_banks(&mut cart.memory);
        Ok(board.into())
    }

    fn prg_banks(&self) -> (u16, u16) {
        let address = self.register;
        let bank = ((address >> 2) & 0x1F) | ((address & 0x0100) >> 3);
        let single_bank = address & 0x0001 != 0;
        let last_bank = address & 0x0200 != 0;
        let paired_mode = address & 0x0080 != 0;

        if paired_mode {
            if single_bank {
                let first = bank & 0x00FE;
                (first, first | 1)
            } else {
                (bank, bank)
            }
        } else if single_bank {
            let second = if last_bank {
                bank | 0x0007
            } else {
                bank & 0x0038
            };
            (bank & 0x003E, second)
        } else {
            let second = if last_bank {
                bank | 0x0007
            } else {
                bank & 0x0038
            };
            (bank, second)
        }
    }
}

impl Map for Mapper227 {
    fn mirroring(&self) -> Mirroring {
        if self.register & 0x0002 != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn write_register(&mut self, memory: &mut Memory, address: u16, _value: u8) {
        if address < 0x8000 {
            return;
        }
        self.register = address;
        self.update_banks(memory);
    }

    fn update_banks(&mut self, memory: &mut Memory) {
        let (first, second) = self.prg_banks();
        memory.map_prg(0x6000, 8 * 1024, 0, Src::PrgRam);
        memory.map_prg(0x8000, Self::PRG_WINDOW, i32::from(first), Src::PrgRom);
        memory.map_prg(0xC000, Self::PRG_WINDOW, i32::from(second), Src::PrgRom);
        memory.map_chr(0x0000, Self::CHR_WINDOW, 0, Src::Chr);
        memory.set_mirroring(self.mirroring());
    }
}
