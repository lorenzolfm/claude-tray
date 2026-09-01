mod agents;
mod icon;
mod jump;
mod mark;
mod state;
mod tray;

use ksni::blocking::TrayMethods;
use std::sync::Arc;
use std::time::Duration;

const TICK: Duration = Duration::from_millis(100);

const TICKS_PER_POLL: u64 = 50;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let renderer = match icon::Renderer::load() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("claude-tray: {e}");
            std::process::exit(1);
        }
    };

    let anim = Arc::new(tray::Animation::default());
    let handle = tray::ClaudeTray::new(renderer, Arc::clone(&anim))
        .assume_sni_available(true)
        .spawn()?;

    let mut ticks: u64 = 0;
    loop {
        std::thread::sleep(TICK);
        ticks += 1;
        anim.advance();

        let poll = ticks.is_multiple_of(TICKS_PER_POLL);
        if !poll && !anim.is_spinning() {
            continue;
        }

        if handle
            .update(|t: &mut tray::ClaudeTray| {
                if poll {
                    t.refresh();
                }
            })
            .is_none()
        {
            eprintln!("claude-tray: tray service stopped unexpectedly");
            std::process::exit(1);
        }
    }
}
