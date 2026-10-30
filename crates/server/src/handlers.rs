//! The commands the server runs itself, with their arguments named as the screens send them
//! (camelCase, as the TV's `invoke()` always did).

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use tvl_kit::Error;
use tvl_protocol::ScreenKind;

use crate::admin::UnlockError;
use crate::calendar;
use crate::clock;
use crate::hub;
use crate::library;
use crate::smarthome::{self, rooms, scenes};
use crate::ctx::Ctx;
use crate::router::{Caller, Failure};

type Answer = Result<Value, Failure>;

/// Argument `key` of `args`; a missing one reads as null (None for options).
fn arg<T: DeserializeOwned>(args: &Value, key: &str) -> Result<T, Failure> {
    serde_json::from_value(args.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|e| Failure::Failed(format!("argument {key}: {e}")))
}

fn json(v: impl Serialize) -> Answer {
    Ok(serde_json::to_value(v)?)
}

pub async fn run(ctx: &Ctx, caller: &Caller, cmd: &str, a: Value) -> Answer {
    match cmd {
        // ---------- The house's screens ----------
        // A phone or tablet says who it is when the page opens: it's one of the house's screens.
        "companion_hello" => json(ctx.devices.hello(&arg::<String>(&a, "id")?, arg(&a, "kind")?, &arg::<String>(&a, "name")?)?),
        "devices_get" => json(ctx.devices.list()),
        "smarthome_put_screen" => {
            let room: Option<String> = arg(&a, "room")?;
            json(ctx.devices.put(&arg::<String>(&a, "id")?, &arg::<String>(&a, "name")?, room.as_deref())?)
        }
        "smarthome_forget_screen" => {
            let gone = ctx.devices.forget(&arg::<String>(&a, "id")?)?;
            if gone.kind == ScreenKind::Tv {
                ctx.admin.revoke(&gone.id)?;
                ctx.agents.disconnect(&gone.id);
            }
            json(())
        }
        "screen_follow" => {
            let screen = caller.screen.as_deref().ok_or_else(|| Failure::Failed("Open the companion again".into()))?;
            let device: Option<String> = arg(&a, "device")?;
            json(ctx.devices.pick(screen, device.as_deref())?)
        }
        "device_wake" => {
            let id: String = arg(&a, "id")?;
            let mac = ctx.agent_mac(&id).ok_or_else(|| Error::msg("That TV PC never said its network address"))?;
            json(crate::discovery::wake(&mac).await?)
        }
        // The page couldn't start or crashed (the companion's index.html): into the log.
        "client_log" => {
            let message: String = arg(&a, "message")?;
            let agent: Option<String> = arg(&a, "agent")?;
            tracing::warn!(
                "companion page error: {} [{}]",
                tvl_kit::log::clip(&message, 2000),
                tvl_kit::log::clip(agent.as_deref().unwrap_or("?"), 200)
            );
            json(())
        }

        // ---------- The server, admin ----------
        "server_status" => json(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "protocol": tvl_protocol::PROTOCOL_VERSION,
            "uptimeS": ctx.started.elapsed().as_secs(),
            "shadow": ctx.config.shadow,
            "https": ctx.ca.is_some() && ctx.config.https_port.is_some(),
            "caFingerprint": ctx.ca.as_ref().map(|ca| ca.fingerprint()),
        })),
        "admin_unlock" => match ctx.admin.unlock(&arg::<String>(&a, "pin")?, caller.screen.as_deref()) {
            Ok((token, expires)) => json(json!({ "token": token, "expires": expires })),
            Err(UnlockError::WrongPin) => Err(Failure::Failed("That's not the PIN".into())),
            Err(UnlockError::TooManyTries) => Err(Failure::TooManyTries),
        },
        "admin_check" => json(json!({ "admin": caller.is_admin() })),
        "admin_pin_set" => json(ctx.admin.set_pin(&arg::<String>(&a, "pin")?)?),
        // The household's settings, keys masked; saving a key still masked keeps it.
        "server_settings_get" => json(ctx.settings.get().masked()),
        "server_settings_save" => {
            ctx.settings.save(arg(&a, "settings")?)?;
            let house = ctx.settings.get();
            ctx.bus.emit("settings", &crate::settings::for_tvs(&house));
            ctx.bus.emit("calendar", &crate::calendar::snapshot(ctx));
            json(house.masked())
        }
        "pair_list" => json(ctx.admin.pairings()),
        "pair_approve" => {
            let p = ctx.admin.approve(&arg::<String>(&a, "code")?)?;
            ctx.bus.emit("pairing", &ctx.admin.pairings().len());
            json(p)
        }
        "pair_deny" => {
            ctx.admin.deny(&arg::<String>(&a, "code")?)?;
            ctx.bus.emit("pairing", &ctx.admin.pairings().len());
            json(())
        }

        // ---------- Smart home ----------
        "smarthome_get" => json(smarthome::snapshot(ctx)),
        "smarthome_control" => json(smarthome::control(ctx, &arg::<String>(&a, "id")?, arg(&a, "command")?).await?),
        // A TV PC's Smart Home screen opened (true) or closed: the devices are polled often then.
        "smarthome_watch" => {
            let screen = caller.screen.as_deref().unwrap_or_default();
            smarthome::watch(ctx, screen, arg(&a, "active")?);
            json(())
        }
        "smarthome_refresh" => {
            smarthome::refresh(ctx);
            json(())
        }
        "smarthome_run_scene" => json(scenes::run(ctx, &arg::<String>(&a, "id")?).await?),
        "smarthome_save_scene" => {
            let room: Option<String> = arg(&a, "room")?;
            let id = scenes::save(ctx, &arg::<String>(&a, "id")?, &arg::<String>(&a, "name")?, &arg::<String>(&a, "icon")?, room.as_deref())?;
            smarthome::emit(ctx);
            json(id)
        }
        "smarthome_control_room" => json(smarthome::control_room(ctx, &arg::<String>(&a, "room")?, arg(&a, "command")?).await?),
        "smarthome_add_room" => json(rooms::add_room(ctx, &arg::<String>(&a, "name")?, &arg::<String>(&a, "icon")?)?),
        "smarthome_rename_room" => json(rooms::rename_room(ctx, &arg::<String>(&a, "from")?, &arg::<String>(&a, "to")?)?),
        "smarthome_delete_room" => json(rooms::delete_room(ctx, &arg::<String>(&a, "name")?)?),
        "smarthome_put_task" => {
            let (id, room): (Option<String>, Option<String>) = (arg(&a, "id")?, arg(&a, "room")?);
            json(rooms::put_task(ctx, id.as_deref(), room.as_deref(), &arg::<String>(&a, "title")?)?)
        }
        "smarthome_task_done" => json(rooms::task_done(ctx, &arg::<String>(&a, "id")?, arg(&a, "done")?)?),
        "smarthome_delete_task" => json(rooms::delete_task(ctx, &arg::<String>(&a, "id")?)?),
        "smarthome_put_scene" => {
            let id = scenes::put(ctx, arg(&a, "scene")?)?;
            smarthome::emit(ctx);
            json(id)
        }
        "smarthome_delete_scene" => {
            scenes::delete(ctx, &arg::<String>(&a, "id")?)?;
            smarthome::emit(ctx);
            json(())
        }
        "smarthome_put_automation" => {
            scenes::put_automation(ctx, arg(&a, "automation")?)?;
            smarthome::emit(ctx);
            json(())
        }
        "smarthome_delete_automation" => {
            scenes::delete_automation(ctx, &arg::<String>(&a, "id")?)?;
            smarthome::emit(ctx);
            json(())
        }
        "smarthome_set_layout" => {
            ctx.smarthome.layout.set(arg(&a, "layout")?)?;
            smarthome::emit(ctx);
            json(())
        }
        "ewelink_sign_in" | "ewelink_sign_out" => Err(Failure::Failed("The eWeLink cloud account isn't set up on the server: Sonoff devices are driven on the home network".into())),

        // ---------- Timers and alarms ----------
        // Each answers the whole clock, as the old launcher did. What's set rings where it was set
        // from (the screen's room), unless it says otherwise.
        "clock_get" => json(clock::snapshot(ctx)),
        "clock_timer_start" => {
            let label: Option<String> = arg(&a, "label")?;
            let target = clock::target_for(ctx, caller.screen.as_deref());
            clock::start_timer(ctx, arg(&a, "ms")?, label.as_deref(), target)?;
            json(clock::snapshot(ctx))
        }
        "clock_timer" => {
            clock::change_timer(ctx, &arg::<String>(&a, "id")?, arg(&a, "change")?)?;
            json(clock::snapshot(ctx))
        }
        "clock_alarm_save" => {
            clock::save_alarm(ctx, arg(&a, "alarm")?, clock::target_for(ctx, caller.screen.as_deref()))?;
            json(clock::snapshot(ctx))
        }
        "clock_alarm_on" => {
            clock::set_alarm_on(ctx, &arg::<String>(&a, "id")?, arg(&a, "on")?)?;
            json(clock::snapshot(ctx))
        }
        "clock_alarm_delete" => {
            clock::delete_alarm(ctx, &arg::<String>(&a, "id")?)?;
            json(clock::snapshot(ctx))
        }
        "clock_respond" => {
            clock::respond(ctx, &arg::<String>(&a, "id")?, arg(&a, "answer")?)?;
            json(clock::snapshot(ctx))
        }
        "clock_rang_locally" => json(clock::rang_locally(ctx, &arg::<String>(&a, "id")?, arg(&a, "at")?)?),

        // ---------- Calendar ----------
        "calendar_get" => json(calendar::agenda(ctx, arg(&a, "from")?, arg(&a, "to")?).await?),
        "calendar_next" => json(calendar::next(ctx).await),
        "calendar_link" => json(calendar::snapshot(ctx)),
        "calendar_sign_in" => {
            let device = crate::oauth::sign_in_device(ctx, caller)?;
            json(calendar::sign_in(ctx, &device).await?)
        }
        "calendar_sign_out" => json(calendar::sign_out(ctx)?),
        "calendar_show" => json(calendar::show(ctx, &arg::<String>(&a, "id")?, arg(&a, "on")?).await?),
        "calendar_remind" => json(calendar::remind(ctx, &arg::<String>(&a, "id")?, arg(&a, "end")?, arg(&a, "on")?)?),
        "calendar_travel" => json(calendar::travel(ctx, &arg::<String>(&a, "place")?).await),
        "calendar_qr" => json(qr_svg(&arg::<String>(&a, "text")?)?),

        // ---------- Assistant ----------
        "assistant_ask" => {
            let asked = crate::assistant::Asked { screen: arg(&a, "screen")?, room: arg(&a, "room")? };
            let now: String = arg(&a, "now")?;
            json(crate::assistant::ask(ctx, caller, &arg::<String>(&a, "text")?, &now, asked).await?)
        }
        // A TV PC's microphone, as base64 WAV: the words, or null when nothing was said.
        "assistant_transcribe" => {
            use base64::Engine as _;
            let wav = base64::engine::general_purpose::STANDARD
                .decode(arg::<String>(&a, "wav")?)
                .map_err(|_| Failure::Failed("Not a recording".into()))?;
            json(crate::assistant::transcribe::transcribe_wav(ctx, wav).await?)
        }

        // ---------- The house's library ----------
        "library_get" => json(library::list(ctx).await),
        "library_title" => json(library::title(ctx, &arg::<String>(&a, "mediaType")?, arg(&a, "id")?).await?),
        "library_request" => {
            json(library::request(ctx, &arg::<String>(&a, "mediaType")?, arg(&a, "id")?, arg(&a, "season")?, arg(&a, "episode")?).await?)
        }
        "library_remove" => json(library::remove(ctx, &arg::<String>(&a, "mediaType")?, arg(&a, "id")?).await?),
        "library_downloads" => json(library::downloads(ctx).await),
        "library_download_control" => json(library::control(ctx, &arg::<String>(&a, "hash")?, arg(&a, "action")?).await?),
        "library_stream" => {
            json(library::stream(ctx, &arg::<String>(&a, "mediaType")?, arg(&a, "id")?, arg(&a, "season")?, arg(&a, "episode")?).await?)
        }

        // ---------- The companion's Hub ----------
        "companion_get" => json(hub::snapshot(ctx)),
        "companion_set_hub" => json(hub::set_hub(ctx, arg(&a, "hub")?)?),

        // Known commands the server doesn't run yet (each moves here in its milestone).
        _ => Err(Failure::Failed(format!("{cmd} isn't on the server yet"))),
    }
}

/// `text` as a QR code (an SVG for the page), for opening something on a phone.
fn qr_svg(text: &str) -> tvl_kit::Result<String> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::new(text.as_bytes()).map_err(|e| Error::msg(format!("QR code: {e}")))?;
    let svg = code
        .render::<svg::Color>()
        .min_dimensions(360, 360)
        .dark_color(svg::Color("#0c0c14"))
        .light_color(svg::Color("#ffffff"))
        .build();
    // Drop the XML declaration: the SVG goes inside the page's HTML.
    Ok(svg.find("<svg").map(|i| svg[i..].to_string()).unwrap_or(svg))
}
