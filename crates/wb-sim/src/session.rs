//! The session section of a save (doc 12 §1, ADR-044): which side the screen
//! is on, hot-seat settings, PIN hashes and whether the umpire view was ever
//! used.
//!
//! Client state, not war state. A [`Session`] lives beside the world in
//! [`Sim`](crate::Sim) and is written to its own section of the save; it is
//! never part of the snapshot, so switching sides, handing over or looking at
//! the umpire view cannot change the state hash (G16).

use serde::{Deserialize, Serialize};

use crate::world::{PlayMode, Sides};

/// How a hot-seat game hands the device over (doc 12 §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
// Externally tagged: the session is stored with postcard, which cannot read
// internally tagged enums.
pub enum HandOver {
    /// The active player presses *Hand over*.
    OnRequest,
    /// Also every `minutes` sim minutes (5, 15 or 30).
    Turns { minutes: u8 },
}

/// What happens when something urgent hits the waiting side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Urgent {
    /// Pause and open the handover screen, without saying why.
    Interrupt,
    /// The waiting side's units act on their ROE; requests wait or expire.
    Hold,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HotSeat {
    pub hand_over: HandOver,
    pub urgent: Urgent,
}

impl Default for HotSeat {
    fn default() -> Self {
        Self {
            hand_over: HandOver::OnRequest,
            urgent: Urgent::Interrupt,
        }
    }
}

/// A 4-digit PIN as a salted BLAKE3 hash: a courtesy between players on one
/// device, not security (ADR-044).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinHash {
    pub salt: [u8; 16],
    pub hash: [u8; 32],
}

impl PinHash {
    fn compute(salt: [u8; 16], pin: &str) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(b"WB-PIN");
        h.update(&salt);
        h.update(pin.as_bytes());
        *h.finalize().as_bytes()
    }

    pub fn new(salt: [u8; 16], pin: &str) -> Self {
        Self {
            salt,
            hash: Self::compute(salt, pin),
        }
    }

    pub fn matches(&self, pin: &str) -> bool {
        Self::compute(self.salt, pin) == self.hash
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// The human side the screen is on.
    pub active_side: u8,
    pub hot_seat: HotSeat,
    /// One entry per side; `None`: no PIN.
    pub pins: Vec<Option<PinHash>>,
    /// The player turned the umpire view on (Command both only).
    pub umpire_enabled: bool,
    /// The umpire view was looked at at least once. Never cleared.
    pub umpire_used: bool,
}

/// The session as the client reads it: no hashes, only whether a PIN is set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SessionView {
    pub active_side: u8,
    pub hot_seat: HotSeat,
    pub has_pin: Vec<bool>,
    pub umpire_enabled: bool,
    pub umpire_used: bool,
}

impl Session {
    /// A fresh session for a war: the screen starts on the `player` side if
    /// that is a human side, else the first human side.
    pub fn new(sides: &Sides) -> Self {
        let humans = sides.human_sides();
        let first = sides
            .sides
            .iter()
            .position(|s| s.player)
            .map(|i| i as u8)
            .filter(|s| humans.contains(s))
            .or_else(|| humans.first().copied())
            .unwrap_or(0);
        Self {
            active_side: first,
            hot_seat: HotSeat::default(),
            pins: vec![None; sides.len()],
            umpire_enabled: false,
            umpire_used: false,
        }
    }

    pub fn view(&self) -> SessionView {
        SessionView {
            active_side: self.active_side,
            hot_seat: self.hot_seat,
            has_pin: self.pins.iter().map(Option::is_some).collect(),
            umpire_enabled: self.umpire_enabled,
            umpire_used: self.umpire_used,
        }
    }
}

/// Session operations; they take the war's sides because the rules depend on
/// the mode.
impl Session {
    /// Moves the screen to a human side. Instant in Command both and Watch; a
    /// hot-seat client goes through the handover screen and then calls this.
    pub fn set_active_side(&mut self, sides: &Sides, side: u8) -> Result<(), String> {
        if usize::from(side) >= sides.len() {
            return Err(format!("There is no side {side}"));
        }
        if sides.mode == PlayMode::VsAi && side != self.active_side {
            return Err("Against the AI there is only one side to command".to_owned());
        }
        if sides.is_ai(side) && sides.mode != PlayMode::Watch {
            return Err("That side is controlled by the AI".to_owned());
        }
        self.active_side = side;
        Ok(())
    }

    /// The umpire view may be turned on only in Command both.
    pub fn set_umpire(&mut self, sides: &Sides, on: bool) -> Result<(), String> {
        if on && sides.mode != PlayMode::CommandBoth {
            return Err("The umpire view is available only when you command both sides".to_owned());
        }
        self.umpire_enabled = on;
        Ok(())
    }

    /// Whether the umpire view may be built now.
    pub fn umpire_allowed(&self, sides: &Sides) -> Result<(), String> {
        if sides.mode != PlayMode::CommandBoth {
            return Err("The umpire view is available only when you command both sides".to_owned());
        }
        if !self.umpire_enabled {
            return Err("The umpire view is switched off".to_owned());
        }
        Ok(())
    }

    pub fn set_hot_seat(&mut self, sides: &Sides, hot_seat: HotSeat) -> Result<(), String> {
        if sides.mode != PlayMode::HotSeat {
            return Err("Hot-seat settings apply only in a hot-seat game".to_owned());
        }
        if let HandOver::Turns { minutes } = hot_seat.hand_over
            && ![5, 15, 30].contains(&minutes)
        {
            return Err("A turn lasts 5, 15 or 30 sim minutes".to_owned());
        }
        self.hot_seat = hot_seat;
        Ok(())
    }

    /// Sets (4 digits) or clears (`None`) a side's PIN.
    pub fn set_pin(
        &mut self,
        sides: &Sides,
        side: u8,
        pin: Option<&str>,
        salt: [u8; 16],
    ) -> Result<(), String> {
        if usize::from(side) >= sides.len() || sides.is_ai(side) {
            return Err("Only a human side can have a PIN".to_owned());
        }
        self.pins.resize(sides.len(), None);
        self.pins[usize::from(side)] = match pin {
            None => None,
            Some(p) if p.len() == 4 && p.bytes().all(|b| b.is_ascii_digit()) => {
                Some(PinHash::new(salt, p))
            }
            Some(_) => return Err("A PIN is 4 digits".to_owned()),
        };
        Ok(())
    }

    /// True when the side has no PIN or `pin` is right.
    pub fn check_pin(&self, side: u8, pin: &str) -> bool {
        match self.pins.get(usize::from(side)).copied().flatten() {
            None => true,
            Some(h) => h.matches(pin),
        }
    }
}
