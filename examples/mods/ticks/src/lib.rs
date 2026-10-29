//! Example mod: logs the client hook events (ticks by role, frames, HUD, keys) and exercises the
//! `fish:client` service, including use of a handle after its callback.

use std::sync::Mutex;

use fish_api::client::{ClientHandle, hud, input};
use fish_api::event::client::FRAME;
use fish_api::event::lifecycle::{client, server};
use fish_api::{ActionResult, Error};

static KEPT: Mutex<Option<ClientHandle>> = Mutex::new(None);

fn init() -> Result<(), Error> {
    client::START_TICK.register(|c| log::info!("client start {} {:?}", c.tick(), c.role()))?;
    client::END_TICK.register(|c| {
        log::info!("client end {}", c.tick());
        if c.tick() == 1 {
            let handle = c.client();
            log::info!("role {:?}", handle.role());
            log::info!("player {:?}", handle.player());
            log::info!(
                "game dir {:?}",
                handle
                    .game_dir()
                    .map(|d| d.is_absolute() || !d.as_os_str().is_empty())
            );
            *KEPT.lock().unwrap() = Some(handle);
        } else if c.tick() == 2 {
            // The handle of tick 1 is dead now.
            let kept = KEPT.lock().unwrap().take();
            if let Some(h) = kept {
                log::info!(
                    "stale: {:?}",
                    h.show_chat("never shown").map_err(|e| e.status())
                );
            }
        }
    })?;
    server::START_TICK.register(|c| log::info!("server start {} {:?}", c.tick(), c.role()))?;
    server::END_TICK.register(|c| log::info!("server end {}", c.tick()))?;
    FRAME.register(|f| log::info!("frame {:.3}", f.elapsed_secs()))?;
    hud::RENDER.register(|h| {
        let d = h.draw();
        log::info!(
            "hud size {:?} width {:?}",
            d.gui_size(),
            d.text_width("abc")
        );
        let _ = d.fill([0.0, 0.0, 10.0, 10.0], 0x7800_0000);
    })?;
    input::KEY.register(|k| {
        log::info!(
            "key {} pressed={} down={:?}",
            k.key(),
            k.pressed(),
            fish_api::client::is_key_down(k.key())
        );
    })?;
    fish_api::client::command::RAW_COMMAND.register(|c| {
        if c.command() == "ticks pass" {
            ActionResult::Pass
        } else {
            ActionResult::Fail
        }
    })?;
    Ok(())
}

fish_api::export_mod! {
    main: [init],
}
