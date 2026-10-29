//! Networking: custom payloads between client and server (Fabric `fabric-networking-api-v1`).
//!
//! A payload is a channel id (`namespace:path`) plus bytes; build and read the bytes with
//! [`PacketBuf`] and [`PacketReader`] or your own codec. Declare every type first:
//!
//! ```ignore
//! PayloadTypeRegistry::play_s2c().register("mymod:hello", 1024)?;
//! PayloadTypeRegistry::play_c2s().register("mymod:reply", 1024)?;
//! ClientPlayNetworking::register_global_receiver("mymod:hello", |ctx| {
//!     let name = PacketReader::new(ctx.data()).read_string().unwrap_or_default();
//!     let _ = ClientPlayNetworking::send("mymod:reply", PacketBuf::new().string(&name).as_bytes());
//! })?;
//! ServerPlayNetworking::register_global_receiver("mymod:reply", |ctx| {
//!     log::info!("{} answered", ctx.player_name());
//! })?;
//! ```
//!
//! Limits: clientbound payloads are at most 1 MiB, serverbound 32,767 bytes, and never more than the
//! `max_size` you registered. A peer that does not run Fish (or has no receiver for the channel)
//! is skipped: [`can_send`](ServerPlayNetworking::can_send) says so, and a direct send fails with
//! `UNSUPPORTED`.

use std::ffi::c_void;

use fish_abi::networking::{
    C2S_CONFIG, C2S_PLAY, NetPayloadV1, NetworkingServiceV1, S2C_CONFIG, S2C_PLAY,
    SERVICE_NETWORKING,
};
use fish_abi::{FishStatus, FishStr};

use crate::client::{ClientHandle, service};
use crate::error::{Error, check};
use crate::host;

/// Builds payload bytes (big endian, Minecraft style varints).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PacketBuf(Vec<u8>);

impl PacketBuf {
    /// An empty buffer.
    pub fn new() -> PacketBuf {
        PacketBuf::default()
    }

    /// Appends a byte.
    pub fn u8(mut self, v: u8) -> Self {
        self.0.push(v);
        self
    }

    /// Appends a bool as one byte.
    pub fn bool(self, v: bool) -> Self {
        self.u8(u8::from(v))
    }

    /// Appends an `i32`.
    pub fn i32(mut self, v: i32) -> Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }

    /// Appends an `i64`.
    pub fn i64(mut self, v: i64) -> Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }

    /// Appends an `f32`.
    pub fn f32(mut self, v: f32) -> Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }

    /// Appends an `f64`.
    pub fn f64(mut self, v: f64) -> Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }

    /// Appends a varint (1 to 5 bytes).
    pub fn varint(mut self, v: u32) -> Self {
        let mut v = v;
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                self.0.push(byte);
                return self;
            }
            self.0.push(byte | 0x80);
        }
    }

    /// Appends a string: varint length in bytes, then UTF-8.
    pub fn string(self, s: &str) -> Self {
        let mut b = self.varint(s.len() as u32);
        b.0.extend_from_slice(s.as_bytes());
        b
    }

    /// Appends raw bytes.
    pub fn bytes(mut self, data: &[u8]) -> Self {
        self.0.extend_from_slice(data);
        self
    }

    /// The bytes built so far.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The bytes built so far.
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

/// Reads what [`PacketBuf`] wrote.
#[derive(Debug, Clone)]
pub struct PacketReader<'a> {
    data: &'a [u8],
}

impl<'a> PacketReader<'a> {
    /// Reads `data` from the start.
    pub fn new(data: &'a [u8]) -> PacketReader<'a> {
        PacketReader { data }
    }

    /// Bytes not read yet.
    pub fn remaining(&self) -> &'a [u8] {
        self.data
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.data.len() < n {
            return Err(Error::InvalidArgument(format!(
                "the payload ends early: wanted {n} more byte(s), {} left",
                self.data.len()
            )));
        }
        let (head, tail) = self.data.split_at(n);
        self.data = tail;
        Ok(head)
    }

    /// Reads a byte.
    pub fn read_u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    /// Reads a bool.
    pub fn read_bool(&mut self) -> Result<bool, Error> {
        Ok(self.read_u8()? != 0)
    }

    /// Reads an `i32`.
    pub fn read_i32(&mut self) -> Result<i32, Error> {
        Ok(i32::from_be_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    /// Reads an `i64`.
    pub fn read_i64(&mut self) -> Result<i64, Error> {
        Ok(i64::from_be_bytes(
            self.take(8)?.try_into().expect("8 bytes"),
        ))
    }

    /// Reads an `f32`.
    pub fn read_f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_be_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    /// Reads an `f64`.
    pub fn read_f64(&mut self) -> Result<f64, Error> {
        Ok(f64::from_be_bytes(
            self.take(8)?.try_into().expect("8 bytes"),
        ))
    }

    /// Reads a varint.
    pub fn read_varint(&mut self) -> Result<u32, Error> {
        let mut out = 0u32;
        for i in 0..5 {
            let b = self.read_u8()?;
            out |= u32::from(b & 0x7f) << (7 * i);
            if b & 0x80 == 0 {
                return Ok(out);
            }
        }
        Err(Error::InvalidArgument(
            "a varint is longer than 5 bytes".to_string(),
        ))
    }

    /// Reads a string.
    pub fn read_string(&mut self) -> Result<String, Error> {
        let n = self.read_varint()? as usize;
        let bytes = self.take(n)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| Error::InvalidArgument("a string is not valid UTF-8".to_string()))
    }
}

/// A payload that arrived, as a receiver sees it; valid only during the call.
#[repr(transparent)]
pub struct PayloadContext(NetPayloadV1);

impl PayloadContext {
    /// The channel.
    pub fn channel(&self) -> &str {
        // SAFETY: the host passes valid UTF-8 that lives for the call.
        unsafe { self.0.channel.as_str() }.unwrap_or("")
    }

    /// The payload bytes.
    pub fn data(&self) -> &[u8] {
        if self.0.data.is_null() || self.0.len == 0 {
            return &[];
        }
        // SAFETY: the host passes `len` readable bytes for the duration of the call.
        unsafe { std::slice::from_raw_parts(self.0.data, self.0.len) }
    }

    /// On a server: the sending player's name (empty on a client).
    pub fn player_name(&self) -> &str {
        // SAFETY: as for `channel`.
        unsafe { self.0.sender.as_str() }.unwrap_or("")
    }

    /// On a server: the sending player's UUID.
    pub fn player_uuid(&self) -> Option<u128> {
        let n = u128::from_be_bytes(self.0.sender_uuid);
        (n != 0).then_some(n)
    }

    /// The game that received the payload.
    pub fn client(&self) -> ClientHandle {
        ClientHandle::from_raw(self.0.view)
    }
}

type Receiver = Box<dyn Fn(&PayloadContext) + Send + Sync + 'static>;

unsafe extern "C" fn receive(user: *mut c_void, payload: *const NetPayloadV1) -> FishStatus {
    if user.is_null() || payload.is_null() {
        return FishStatus::INVALID_ARGUMENT;
    }
    host::guard("a payload receiver", || {
        // SAFETY: `user` is the boxed receiver of `register_receiver`; `payload` is valid for
        // the call and `PayloadContext` is `repr(transparent)` over it.
        let (f, ctx) = unsafe {
            (
                &*user.cast::<Receiver>(),
                &*payload.cast::<PayloadContext>(),
            )
        };
        f(ctx);
        FishStatus::OK
    })
}

unsafe extern "C" fn drop_receiver(user: *mut c_void) {
    // SAFETY: `user` was created by `Box::into_raw(Box::new(Receiver))` in `register_receiver`.
    drop(unsafe { Box::from_raw(user.cast::<Receiver>()) });
}

fn register_receiver<F>(dir: u8, id: &str, f: F) -> Result<(), Error>
where
    F: Fn(&PayloadContext) + Send + Sync + 'static,
{
    let (svc, host_ptr) = service::<NetworkingServiceV1>(SERVICE_NETWORKING)?;
    let table = host::get()?;
    let boxed: Receiver = Box::new(f);
    let user = Box::into_raw(Box::new(boxed)).cast::<c_void>();
    // SAFETY: valid service table; the id outlives the call; ownership of `user` moves to the host,
    // which calls `drop_receiver` exactly once.
    let status = unsafe {
        (svc.register_receiver)(
            host_ptr,
            table.mod_token,
            dir,
            FishStr::new(id),
            receive,
            user,
            Some(drop_receiver),
        )
    };
    if !status.is_ok() {
        // SAFETY: the host did not take ownership.
        unsafe { drop_receiver(user) };
    }
    check("registering a payload receiver", status)
}

/// Declares payload types: `PayloadTypeRegistry::play_s2c().register("mod:id", max_size)`.
pub struct PayloadTypeRegistry {
    dir: u8,
}

impl PayloadTypeRegistry {
    /// Server to client, play phase.
    pub fn play_s2c() -> PayloadTypeRegistry {
        PayloadTypeRegistry { dir: S2C_PLAY }
    }

    /// Client to server, play phase.
    pub fn play_c2s() -> PayloadTypeRegistry {
        PayloadTypeRegistry { dir: C2S_PLAY }
    }

    /// Server to client, configuration phase.
    pub fn configuration_s2c() -> PayloadTypeRegistry {
        PayloadTypeRegistry { dir: S2C_CONFIG }
    }

    /// Client to server, configuration phase.
    pub fn configuration_c2s() -> PayloadTypeRegistry {
        PayloadTypeRegistry { dir: C2S_CONFIG }
    }

    /// Declares `id` with a maximum payload size (at most 1 MiB clientbound, 32,767 bytes serverbound).
    /// Registering the same id and size twice is fine; another size is an error.
    pub fn register(&self, id: &str, max_size: usize) -> Result<(), Error> {
        let (svc, host_ptr) = service::<NetworkingServiceV1>(SERVICE_NETWORKING)?;
        let table = host::get()?;
        let max = u32::try_from(max_size)
            .map_err(|_| Error::InvalidArgument(format!("max_size {max_size} is too large")))?;
        // SAFETY: valid service table; the id outlives the call.
        check("registering a payload type", unsafe {
            (svc.register_type)(host_ptr, table.mod_token, self.dir, FishStr::new(id), max)
        })
    }
}

macro_rules! server_api {
    ($(#[$m:meta])* $name:ident, receive = $recv:expr, send = $send:expr) => {
        $(#[$m])*
        pub struct $name;

        impl $name {
            /// Registers the one receiver of `id` (the type must be declared first).
            pub fn register_global_receiver<F>(id: &str, f: F) -> Result<(), Error>
            where
                F: Fn(&PayloadContext) + Send + Sync + 'static,
            {
                register_receiver($recv, id, f)
            }

            /// Sends to the named player.
            pub fn send(player: &str, id: &str, data: &[u8]) -> Result<(), Error> {
                send_to_player($send, player, id, data)
            }

            /// Sends to every player that can receive `id`.
            pub fn send_all(id: &str, data: &[u8]) -> Result<(), Error> {
                send_to_player($send, "", id, data)
            }

            /// True when the named player announced a receiver for `id`.
            pub fn can_send(player: &str, id: &str) -> Result<bool, Error> {
                can_send($send, player, id)
            }

            /// The players connected in this phase.
            pub fn players() -> Result<Vec<String>, Error> {
                players($send)
            }
        }
    };
}

macro_rules! client_api {
    ($(#[$m:meta])* $name:ident, receive = $recv:expr, send = $send:expr) => {
        $(#[$m])*
        pub struct $name;

        impl $name {
            /// Registers the one receiver of `id` (the type must be declared first).
            pub fn register_global_receiver<F>(id: &str, f: F) -> Result<(), Error>
            where
                F: Fn(&PayloadContext) + Send + Sync + 'static,
            {
                register_receiver($recv, id, f)
            }

            /// Sends to the server.
            pub fn send(id: &str, data: &[u8]) -> Result<(), Error> {
                send_to_server($send, id, data)
            }

            /// True when the server announced a receiver for `id`.
            pub fn can_send(id: &str) -> Result<bool, Error> {
                can_send($send, "", id)
            }
        }
    };
}

fn send_to_player(dir: u8, player: &str, id: &str, data: &[u8]) -> Result<(), Error> {
    let (svc, host_ptr) = service::<NetworkingServiceV1>(SERVICE_NETWORKING)?;
    // SAFETY: valid service table; the strings and bytes outlive the call.
    check("sending a payload", unsafe {
        (svc.send_to_player)(
            host_ptr,
            dir,
            FishStr::new(player),
            FishStr::new(id),
            data.as_ptr(),
            data.len(),
        )
    })
}

fn send_to_server(dir: u8, id: &str, data: &[u8]) -> Result<(), Error> {
    let (svc, host_ptr) = service::<NetworkingServiceV1>(SERVICE_NETWORKING)?;
    // SAFETY: as in `send_to_player`.
    check("sending a payload", unsafe {
        (svc.send_to_server)(host_ptr, dir, FishStr::new(id), data.as_ptr(), data.len())
    })
}

fn can_send(dir: u8, player: &str, id: &str) -> Result<bool, Error> {
    let (svc, host_ptr) = service::<NetworkingServiceV1>(SERVICE_NETWORKING)?;
    // SAFETY: valid service table; the strings outlive the call.
    Ok(unsafe { (svc.can_send)(host_ptr, dir, FishStr::new(player), FishStr::new(id)) } != 0)
}

unsafe extern "C" fn collect_name(user: *mut c_void, name: FishStr) {
    // SAFETY: `user` is the `Vec<String>` of `players`; the name is valid UTF-8 or skipped.
    unsafe {
        if let Ok(s) = name.as_str() {
            (*user.cast::<Vec<String>>()).push(s.to_string());
        }
    }
}

fn players(dir: u8) -> Result<Vec<String>, Error> {
    let (svc, host_ptr) = service::<NetworkingServiceV1>(SERVICE_NETWORKING)?;
    let mut out: Vec<String> = Vec::new();
    // SAFETY: valid service table; `out` outlives the call and the sink only appends to it.
    unsafe { (svc.players)(host_ptr, dir, collect_name, (&raw mut out).cast()) };
    Ok(out)
}

server_api!(
    /// Play-phase payloads on the server: receives client-to-server, sends server-to-client.
    ServerPlayNetworking, receive = C2S_PLAY, send = S2C_PLAY
);
server_api!(
    /// Configuration-phase payloads on the server.
    ServerConfigurationNetworking, receive = C2S_CONFIG, send = S2C_CONFIG
);
client_api!(
    /// Play-phase payloads on the client: receives server-to-client, sends client-to-server.
    ClientPlayNetworking, receive = S2C_PLAY, send = C2S_PLAY
);
client_api!(
    /// Configuration-phase payloads on the client.
    ClientConfigurationNetworking, receive = S2C_CONFIG, send = C2S_CONFIG
);
