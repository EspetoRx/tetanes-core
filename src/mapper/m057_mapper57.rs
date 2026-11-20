//! Mapper 057 (Multicart).

#![allow(missing_docs)]

use crate::{
    cart::Cart,
    common::ResetKind,
    mapper::{self, Map, Mapper},
    memory::{Memory, Src},
    ppu::Mirroring,
};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tracing::info;

static M057_WRITE_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn take_m057_logs() -> Vec<String> {
    match M057_WRITE_LOG.lock() {
        Ok(mut guard) => std::mem::take(&mut *guard),
        Err(_) => Vec::new(),
    }
}

fn push_m057_log(line: String) {
    info!("{line}");
    if let Ok(mut guard) = M057_WRITE_LOG.lock()
        && guard.len() < 512
    {
        guard.push(line);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[must_use]
pub struct Mapper57 {
    pub mirroring: Mirroring,
    pub prg_bank: u8,
    pub prg_32k: bool,
    pub chr_bank: u8,
    pub prg_16k_count: usize,
    pub chr_8k_count: usize,
    pub dip_switch: u8,
    #[serde(skip)]
    pub write_count: u32,
}

impl Mapper57 {
    const PRG_16K: usize = 16 * 1024;
    const PRG_32K: usize = 32 * 1024;
    const CHR_8K: usize = 8 * 1024;

    fn log_mem_sample(&self, memory: &Memory, tag: &str) {
        let prg_8000 = [
            memory.prg_peek(0x8000),
            memory.prg_peek(0x8001),
            memory.prg_peek(0x8002),
            memory.prg_peek(0x8003),
            memory.prg_peek(0x8004),
            memory.prg_peek(0x8005),
        ];
        let prg_c000 = [
            memory.prg_peek(0xC000),
            memory.prg_peek(0xC001),
            memory.prg_peek(0xC002),
            memory.prg_peek(0xFFFA),
            memory.prg_peek(0xFFFB),
            memory.prg_peek(0xFFFC),
            memory.prg_peek(0xFFFD),
        ];
        let chr_0000 = [
            memory.chr_peek(0x0000),
            memory.chr_peek(0x0001),
            memory.chr_peek(0x0002),
            memory.chr_peek(0x0003),
            memory.chr_peek(0x0010),
            memory.chr_peek(0x1000),
        ];
        push_m057_log(format!(
            "[m057 {tag}] prg={} 32k={} chr={} mir={:?} dip={} | $8000={prg_8000:02X?} | $C000/vec={prg_c000:02X?} | chr={chr_0000:02X?}",
            self.prg_bank, self.prg_32k, self.chr_bank, self.mirroring, self.dip_switch
        ));
    }

    pub fn load(cart: &mut Cart) -> Result<Mapper, mapper::Error> {
        let prg_16k_count = (cart.prg_rom_size / Self::PRG_16K).max(1);
        let chr_8k_count = (cart.chr_rom().len() / Self::CHR_8K).max(1);
        let init_bank = (prg_16k_count.saturating_sub(1)) as u8;
        let mut board = Self {
            mirroring: cart.mirroring(),
            prg_bank: init_bank,
            prg_32k: false,
            chr_bank: 0,
            prg_16k_count,
            chr_8k_count,
            dip_switch: 0,
            write_count: 0,
        };
        push_m057_log(format!(
            "[m057 load] prg_rom_size={} chr_rom_len={} chr_ram_size={} prg16k={} chr8k={} init_prg={} init_mir={:?}",
            cart.prg_rom_size,
            cart.chr_rom().len(),
            cart.chr_ram_size,
            board.prg_16k_count,
            board.chr_8k_count,
            board.prg_bank,
            board.mirroring
        ));
        board.update_banks(&mut cart.memory);
        board.log_mem_sample(&cart.memory, "after-load");
        Ok(board.into())
    }
}

impl Map for Mapper57 {
    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn reset(&mut self, kind: ResetKind) {
        if kind == ResetKind::Soft {
            self.dip_switch = self.dip_switch.wrapping_add(1) & 0x07;
        } else {
            self.dip_switch = 0;
        }
        self.prg_bank = (self.prg_16k_count.saturating_sub(1)) as u8;
        self.prg_32k = false;
        self.chr_bank = 0;
        self.write_count = 0;
        push_m057_log(format!(
            "[m057 reset] kind={kind:?} -> prg={} 32k={} chr={} mir={:?} dip={}",
            self.prg_bank, self.prg_32k, self.chr_bank, self.mirroring, self.dip_switch
        ));
    }

    fn registers(&self, out: &mut Vec<(&'static str, u32)>) {
        out.push(("PRG Bank", u32::from(self.prg_bank)));
        out.push(("PRG 32K", u32::from(self.prg_32k)));
        out.push(("CHR Bank", u32::from(self.chr_bank)));
        out.push(("DIP Switch", u32::from(self.dip_switch)));
    }

    fn write_register(&mut self, memory: &mut Memory, addr: u16, val: u8) {
        if addr < 0x8000 {
            if (0x4020..0x8000).contains(&addr) && self.write_count < 64 {
                push_m057_log(format!(
                    "[m057 low-write] addr={addr:#06X} val={val:#04X}"
                ));
            }
            return;
        }

        self.write_count += 1;
        let old_prg = self.prg_bank;
        let old_32k = self.prg_32k;
        let old_chr = self.chr_bank;
        let old_mir = self.mirroring;
        let a11 = (addr & 0x0800) != 0;

        if a11 {
            self.chr_bank = (self.chr_bank & 0x08) | (val & 0x07);
            self.mirroring = if (val & 0x08) != 0 {
                Mirroring::Horizontal
            } else {
                Mirroring::Vertical
            };
        } else {
            self.prg_bank = (val >> 5) & 0x07;
            self.prg_32k = (val & 0x10) != 0;
            self.chr_bank = (self.chr_bank & 0x07) | ((val & 0x01) << 3);
        }

        let changed = old_prg != self.prg_bank
            || old_32k != self.prg_32k
            || old_chr != self.chr_bank
            || old_mir != self.mirroring;

        if self.write_count <= 96 || changed {
            let a11_bit = u8::from(a11);
            push_m057_log(format!(
                "[m057 write #{}] addr={addr:#06X} val={val:#04X} A11={a11_bit} changed={changed} -> prg={} (era {old_prg}) 32k={} (era {old_32k}) chr={} (era {old_chr}) mir={:?} (era {old_mir:?})",
                self.write_count,
                self.prg_bank,
                self.prg_32k,
                self.chr_bank,
                self.mirroring,
            ));
        }

        self.update_banks(memory);

        if self.write_count <= 32 || changed {
            self.log_mem_sample(memory, "after-write");
        }
    }

    fn update_banks(&mut self, memory: &mut Memory) {
        memory.map_prg(0x6000, 8 * 1024, 0, Src::PrgRam);

        let prg_mask = (self.prg_16k_count.saturating_sub(1)) as u8;
        let chr_mask = (self.chr_8k_count.saturating_sub(1)) as u8;
        let raw_prg = self.prg_bank;
        let chr8 = i32::from(self.chr_bank & chr_mask);

        if self.prg_32k {
            let bank32_count = (self.prg_16k_count / 2).max(1) as u8;
            let bank32_mask = bank32_count.saturating_sub(1);
            let bank32 = i32::from(((raw_prg & prg_mask) >> 1) & bank32_mask);
            memory.map_prg(0x8000, Self::PRG_32K, bank32, Src::PrgRom);
            if self.write_count <= 24 {
                push_m057_log(format!(
                    "[m057 map] mode=32K bank32={bank32} (raw_prg={raw_prg}, mask={prg_mask:#04X}, bank32_mask={bank32_mask:#04X}) chr8={chr8}"
                ));
            }
        } else {
            let bank16 = i32::from(raw_prg & prg_mask);
            let last_16k = i32::try_from(self.prg_16k_count.saturating_sub(1)).unwrap_or(0);
            memory.map_prg(0x8000, Self::PRG_16K, bank16, Src::PrgRom);
            memory.map_prg(0xC000, Self::PRG_16K, last_16k, Src::PrgRom);
            if self.write_count <= 24 {
                push_m057_log(format!(
                    "[m057 map] mode=16K $8000={bank16} $C000={last_16k} (raw_prg={raw_prg}, mask={prg_mask:#04X}) chr8={chr8}"
                ));
            }
        }

        memory.map_chr(0x0000, Self::CHR_8K, chr8, Src::Chr);
        memory.set_mirroring(self.mirroring);
    }
}
