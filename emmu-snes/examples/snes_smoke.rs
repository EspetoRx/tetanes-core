use std::{env, fs};

use emmu_snes::SnesCore;

const STARTUP_FRAMES: usize = 600;

fn frame_color_count(frame: &[u8]) -> usize {
    frame
        .chunks_exact(4)
        .map(|pixel| (pixel[0], pixel[1], pixel[2]))
        .collect::<std::collections::HashSet<_>>()
        .len()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths = env::args().skip(1).collect::<Vec<_>>();
    if paths.is_empty() {
        return Err(
            "Usage: cargo run -p emmu-snes --example snes_smoke -- ROM.sfc [ROM.smc ...]".into(),
        );
    }

    for path in paths {
        let rom = fs::read(&path)?;
        let mut core = SnesCore::new(&rom)?;
        let mut frame = Vec::new();
        for _ in 0..STARTUP_FRAMES {
            frame = core.run_frame()?;
        }
        let startup_colors = frame_color_count(&frame);
        let state = core.save_state();
        core.run_frame()?;
        core.load_state(&state)?;
        frame = core.run_frame()?;
        let colors = frame_color_count(&frame);

        println!(
            "{}: {} colors after {STARTUP_FRAMES} startup frames, {} after save/load, save/load OK",
            core.game_title(),
            startup_colors,
            colors,
        );
    }
    Ok(())
}
