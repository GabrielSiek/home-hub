//! The frames on a TV PC's socket to the server (`/api/agent`), one JSON text message each.
//!
//! A paired TV PC opens the socket with its token (`Authorization: Bearer …`) and says `hello`. One
//! that isn't paired yet opens it without one and asks with `pair`: the TV shows the code it gets
//! back, someone approves it with the admin PIN, and the token arrives on the same socket.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::device::{Caps, Screen};

/// What a TV PC broadcasts to the server's probe port (UDP) to find it. The server answers with
/// JSON: `{"tvl": <protocol version>, "http": port, "https": port or null, "name": its PC's name}`.
pub const PROBE: &[u8] = b"TVL?";

/// A `refused` reason that means the TV PC's token is no good (it was removed from the house, or
/// the server's data was reset): it forgets the token and asks to pair again.
pub fn means_pair_again(reason: &str) -> bool {
    reason.ends_with("pair it again")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Frame {
    /// TV PC → server, without a token: asks to join the house.
    Pair { device: String, name: String, version: u32 },
    /// Server → TV PC: the code the TV shows until someone approves it.
    PairCode { code: String },
    /// Server → TV PC: approved. Its token (kept secret) and the home certificate authority it
    /// trusts from now on, for `wss://`.
    Paired { token: String, ca_pem: String },
    /// TV PC → server: the first frame on a socket opened with a token.
    Hello(Hello),
    /// Server → TV PC: the answer to `hello`.
    Welcome(Welcome),
    /// Server → TV PC: refused (another version, a revoked token, pairing denied). The socket
    /// closes after it.
    Refused { reason: String },
    /// Either way: run `cmd`. `from` names the screen that asked, when it wasn't the sender.
    Call {
        id: u64,
        cmd: String,
        #[serde(default)]
        args: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
    },
    /// Either way: what a call returned, or why it failed. No `err` means it worked; a missing
    /// `ok` is a null result.
    Reply {
        id: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ok: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        err: Option<String>,
    },
    /// Either way: something happened (`topics` says who hears it).
    Event {
        name: String,
        #[serde(default)]
        payload: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub version: u32,
    pub device: String,
    pub name: String,
    pub caps: Caps,
    /// For Wake-on-LAN.
    #[serde(default)]
    pub mac: Option<String>,
    /// The desktop app's own version, for the devices list.
    #[serde(default)]
    pub app_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Welcome {
    pub version: u32,
    /// This TV PC as the house lists it (its room, its name).
    pub screen: Screen,
    /// Shown in the TV's Settings, for unlocking admin on a phone or tablet.
    pub admin_pin: String,
    /// The server's HTTPS port: the TV PC connects over `wss://` from then on, trusting only the
    /// home certificate authority it got when it was paired. None: no HTTPS.
    #[serde(default)]
    pub https: Option<u16>,
}

impl Frame {
    pub fn reply(id: u64, result: Result<Value, String>) -> Frame {
        match result {
            Ok(Value::Null) => Frame::Reply { id, ok: None, err: None },
            Ok(v) => Frame::Reply { id, ok: Some(v), err: None },
            Err(e) => Frame::Reply { id, ok: None, err: Some(e) },
        }
    }

    /// A reply's outcome, the other way round from `reply`.
    pub fn outcome(ok: Option<Value>, err: Option<String>) -> Result<Value, String> {
        match err {
            Some(e) => Err(e),
            None => Ok(ok.unwrap_or(Value::Null)),
        }
    }

    pub fn to_text(&self) -> String {
        serde_json::to_string(self).expect("frames always serialize")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn frames_round_trip_as_tagged_json() {
        let call = Frame::Call { id: 7, cmd: "remote_key".into(), args: json!({"key": "Up"}), from: Some("s1".into()) };
        let text = call.to_text();
        assert!(text.contains(r#""t":"call""#), "{text}");
        assert_eq!(serde_json::from_str::<Frame>(&text).unwrap(), call);

        // A null result (a command returning nothing) is a reply with neither field.
        let done = Frame::reply(7, Ok(json!(null)));
        assert_eq!(done.to_text(), r#"{"t":"reply","id":7}"#);
        let Frame::Reply { ok, err, .. } = serde_json::from_str::<Frame>(&done.to_text()).unwrap() else { panic!() };
        assert_eq!(Frame::outcome(ok, err), Ok(Value::Null));
        let some = Frame::reply(9, Ok(json!({"volume": 40})));
        let Frame::Reply { ok, err, .. } = serde_json::from_str::<Frame>(&some.to_text()).unwrap() else { panic!() };
        assert_eq!(Frame::outcome(ok, err), Ok(json!({"volume": 40})));
        let err: Frame = serde_json::from_str(r#"{"t":"reply","id":8,"err":"The TV PC went offline"}"#).unwrap();
        assert_eq!(err, Frame::Reply { id: 8, ok: None, err: Some("The TV PC went offline".into()) });

        // An event with no payload reads as null.
        let ev: Frame = serde_json::from_str(r#"{"t":"event","name":"mic-ready"}"#).unwrap();
        assert_eq!(ev, Frame::Event { name: "mic-ready".into(), payload: Value::Null });
    }

    #[test]
    fn hello_names_the_tv_and_what_it_can_do() {
        let hello = Frame::Hello(Hello {
            version: crate::PROTOCOL_VERSION,
            device: "pc-quarto".into(),
            name: "Quarto TV".into(),
            caps: Caps { remote: true, volume: true, ring: true, ..Default::default() },
            mac: Some("00:11:22:33:44:55".into()),
            app_version: "0.1.0".into(),
        });
        let back: Frame = serde_json::from_str(&hello.to_text()).unwrap();
        assert_eq!(back, hello);
    }
}
