use std::borrow::Cow;

use meru_interface::{EmulatorCore, InputData};
use super_sabicom::Snes;

const BUTTONS: [&str; 12] = [
    "A", "B", "X", "Y", "L", "R", "Start", "Select", "Up", "Down", "Left", "Right",
];
const FRAME_WIDTH: usize = 256;
const FRAME_HEIGHT: usize = 224;

fn repair_overlong_lorom_title(rom: &[u8]) -> Cow<'_, [u8]> {
    let copier_header_len = match rom.len() & 0x3FF {
        0 => 0,
        0x200 => 0x200,
        _ => return Cow::Borrowed(rom),
    };
    let header = copier_header_len + 0x7F00;
    let map_mode_index = header + 0xD5;
    if header + 0xE0 > rom.len() {
        return Cow::Borrowed(rom);
    }

    let map_mode = rom[map_mode_index];
    if map_mode & 0xE0 == 0x20 && matches!(map_mode & 0x0F, 0 | 1 | 2 | 3 | 5 | 0x0A) {
        return Cow::Borrowed(rom);
    }
    if !map_mode.is_ascii_alphabetic()
        || !rom[header + 0xC0..=map_mode_index]
            .iter()
            .all(|byte| (0x20..=0x7E).contains(byte))
    {
        return Cow::Borrowed(rom);
    }

    let chipset = rom[header + 0xD6];
    let rom_size_code = rom[header + 0xD7];
    let sram_size_code = rom[header + 0xD8];
    let chipset_is_valid = chipset <= 2 || matches!(chipset & 0x0F, 3 | 4 | 5 | 6 | 9);
    if !chipset_is_valid || rom_size_code > 0x0D || sram_size_code > 9 {
        return Cow::Borrowed(rom);
    }

    let logical_rom_len = rom.len() - copier_header_len;
    let declared_rom_len = 1024usize << rom_size_code;
    if declared_rom_len != logical_rom_len.next_power_of_two() {
        return Cow::Borrowed(rom);
    }

    let checksum_complement = u16::from_le_bytes([rom[header + 0xDC], rom[header + 0xDD]]);
    let checksum = u16::from_le_bytes([rom[header + 0xDE], rom[header + 0xDF]]);
    if checksum_complement != !checksum {
        return Cow::Borrowed(rom);
    }

    // A few dumps let the 22nd title character overwrite the LoROM map-mode byte.
    let mut repaired = rom.to_vec();
    repaired[map_mode_index] = 0x20;
    Cow::Owned(repaired)
}

fn button_index(button: &str) -> Result<usize, String> {
    BUTTONS
        .iter()
        .position(|candidate| *candidate == button)
        .ok_or_else(|| format!("Botão SNES desconhecido: {button}"))
}

pub struct SnesCore {
    snes: Snes,
    buttons: [bool; BUTTONS.len()],
}

impl SnesCore {
    pub fn new(rom: &[u8]) -> Result<Self, super_sabicom::Error> {
        let rom = repair_overlong_lorom_title(rom);
        let snes = Snes::try_from_file(rom.as_ref(), None, &Default::default())?;
        Ok(Self {
            snes,
            buttons: [false; BUTTONS.len()],
        })
    }

    pub fn game_title(&self) -> String {
        self.snes
            .game_info()
            .into_iter()
            .find_map(|(key, value)| (key == "Title").then_some(value))
            .unwrap_or_else(|| "SNES".to_string())
    }

    pub fn set_button(&mut self, button: &str, pressed: bool) -> Result<(), String> {
        let index = button_index(button)?;
        self.buttons[index] = pressed;
        Ok(())
    }

    fn apply_input(&mut self) {
        let controller = BUTTONS
            .iter()
            .zip(self.buttons)
            .map(|(name, pressed)| ((*name).to_string(), pressed))
            .collect();
        self.snes.set_input(&InputData {
            controllers: vec![controller, Vec::new(), Vec::new(), Vec::new()],
        });
    }

    pub fn run_frame(&mut self) -> Result<Vec<u8>, String> {
        self.apply_input();
        self.snes.exec_frame(true);

        let source = self.snes.frame_buffer();
        if source.width < FRAME_WIDTH * 2 || source.height < FRAME_HEIGHT * 2 {
            return Err(format!(
                "O núcleo retornou um framebuffer inválido: {}x{}.",
                source.width, source.height
            ));
        }
        let mut frame = Vec::with_capacity(FRAME_WIDTH * FRAME_HEIGHT * 4);
        for y in 0..FRAME_HEIGHT {
            let source_row = y * 2 * source.width;
            for x in 0..FRAME_WIDTH {
                let pixel = &source.buffer[source_row + x * 2];
                frame.extend_from_slice(&[pixel.r, pixel.g, pixel.b, 255]);
            }
        }
        Ok(frame)
    }

    pub fn run_frame_without_rendering(&mut self) {
        self.apply_input();
        self.snes.exec_frame(false);
    }

    pub fn audio_samples(&self) -> Vec<i16> {
        self.snes
            .audio_buffer()
            .samples
            .iter()
            .flat_map(|sample| [sample.left, sample.right])
            .collect()
    }

    pub fn audio_sample_rate(&self) -> u32 {
        self.snes.audio_buffer().sample_rate
    }

    pub fn save_state(&self) -> Vec<u8> {
        self.snes.save_state()
    }

    pub fn load_state(&mut self, state: &[u8]) -> Result<(), super_sabicom::Error> {
        self.snes.load_state(state)
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;

    use super::SnesCore;

    #[wasm_bindgen]
    pub struct SnesEmulator {
        core: SnesCore,
    }

    #[wasm_bindgen]
    impl SnesEmulator {
        #[wasm_bindgen(constructor)]
        pub fn new(rom: &[u8]) -> Result<SnesEmulator, JsValue> {
            let core = SnesCore::new(rom).map_err(|error| JsValue::from_str(&error.to_string()))?;
            Ok(Self { core })
        }

        #[wasm_bindgen(js_name = gameTitle)]
        pub fn game_title(&self) -> String {
            self.core.game_title()
        }

        #[wasm_bindgen(js_name = setButton)]
        pub fn set_button(&mut self, button: &str, pressed: bool) -> Result<(), JsValue> {
            self.core
                .set_button(button, pressed)
                .map_err(|error| JsValue::from_str(&error))
        }

        #[wasm_bindgen(js_name = runFrame)]
        pub fn run_frame(&mut self, render_graphics: bool) -> Result<Vec<u8>, JsValue> {
            if render_graphics {
                self.core
                    .run_frame()
                    .map_err(|error| JsValue::from_str(&error))
            } else {
                self.core.run_frame_without_rendering();
                Ok(Vec::new())
            }
        }

        #[wasm_bindgen(js_name = audioSamples)]
        pub fn audio_samples(&self) -> Vec<i16> {
            self.core.audio_samples()
        }

        #[wasm_bindgen(js_name = audioSampleRate)]
        pub fn audio_sample_rate(&self) -> u32 {
            self.core.audio_sample_rate()
        }

        #[wasm_bindgen(js_name = saveState)]
        pub fn save_state(&self) -> Vec<u8> {
            self.core.save_state()
        }

        #[wasm_bindgen(js_name = loadState)]
        pub fn load_state(&mut self, state: &[u8]) -> Result<(), JsValue> {
            self.core
                .load_state(state)
                .map_err(|error| JsValue::from_str(&error.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rom(map_mode: u8) -> Vec<u8> {
        let mut rom = vec![0; 1 << 20];
        let header = 0x7F00;
        rom[header + 0xC0..header + 0xD5].copy_from_slice(b"CONTRA3 THE ALIEN WAR");
        rom[header + 0xD5] = map_mode;
        rom[header + 0xD6] = 0;
        rom[header + 0xD7] = 0x0A;
        rom[header + 0xD8] = 0;
        let checksum = 0x1234_u16;
        rom[header + 0xDC..header + 0xDE].copy_from_slice(&(!checksum).to_le_bytes());
        rom[header + 0xDE..header + 0xE0].copy_from_slice(&checksum.to_le_bytes());
        rom
    }

    #[test]
    fn repairs_overlong_lorom_title_without_changing_original_bytes() {
        let rom = test_rom(b'S');
        let repaired = repair_overlong_lorom_title(&rom);

        assert!(matches!(&repaired, Cow::Owned(_)));
        assert_eq!(repaired[0x7FD5], 0x20);
        assert_eq!(rom[0x7FD5], b'S');
    }

    #[test]
    fn leaves_valid_or_inconsistent_headers_unchanged() {
        let valid_rom = test_rom(0x20);
        assert!(matches!(
            repair_overlong_lorom_title(&valid_rom),
            Cow::Borrowed(_)
        ));

        let mut inconsistent_rom = test_rom(b'S');
        inconsistent_rom[0x7FD7] = 0xFF;
        assert!(matches!(
            repair_overlong_lorom_title(&inconsistent_rom),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn rejects_unknown_controller_buttons() {
        assert!(button_index("Turbo").is_err());
        assert_eq!(button_index("Start"), Ok(6));
    }

    #[test]
    fn rejects_invalid_rom_data() {
        assert!(SnesCore::new(&[]).is_err());
    }
}
