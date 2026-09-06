//! Personalized lossy weapon HUD state. No weapon outcome is ever accepted from this packet.
use super::{RIFLE_CADENCE_TICKS, RIFLE_MAGAZINE, RIFLE_RELOAD_TICKS, RIFLE_RESERVE, RifleState};

pub const WEAPON_STATE_BYTES: usize = 49;
const MAGIC: &[u8; 4] = b"DFWS";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeaponStatePacket {
    pub session_id: u64,
    pub server_tick: u64,
    pub last_command_id: u64,
    pub rifle: RifleState,
}

impl WeaponStatePacket {
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.session_id != 0
            && self.server_tick != 0
            && self.rifle.magazine <= RIFLE_MAGAZINE
            && self.rifle.reserve <= RIFLE_RESERVE
            && self.rifle.next_fire_tick.saturating_sub(self.server_tick) <= RIFLE_CADENCE_TICKS
            && self.rifle.reload_complete_tick.is_none_or(|end| {
                end > self.server_tick && end - self.server_tick <= RIFLE_RELOAD_TICKS
            })
    }

    #[must_use]
    pub fn encode(self) -> Option<Vec<u8>> {
        if !self.is_valid() {
            return None;
        }
        let mut bytes = Vec::with_capacity(WEAPON_STATE_BYTES);
        bytes.extend_from_slice(MAGIC);
        bytes.push(1);
        for value in [self.session_id, self.server_tick, self.last_command_id] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&self.rifle.magazine.to_le_bytes());
        bytes.extend_from_slice(&self.rifle.reserve.to_le_bytes());
        bytes.extend_from_slice(&self.rifle.next_fire_tick.to_le_bytes());
        bytes.extend_from_slice(&self.rifle.reload_complete_tick.unwrap_or(0).to_le_bytes());
        Some(bytes)
    }

    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != WEAPON_STATE_BYTES || &bytes[..4] != MAGIC || bytes[4] != 1 {
            return None;
        }
        let u64_at = |offset| {
            Some(u64::from_le_bytes(
                bytes.get(offset..offset + 8)?.try_into().ok()?,
            ))
        };
        let end = u64_at(41)?;
        let packet = Self {
            session_id: u64_at(5)?,
            server_tick: u64_at(13)?,
            last_command_id: u64_at(21)?,
            rifle: RifleState {
                magazine: u16::from_le_bytes(bytes[29..31].try_into().ok()?),
                reserve: u16::from_le_bytes(bytes[31..33].try_into().ok()?),
                next_fire_tick: u64_at(33)?,
                reload_complete_tick: (end != 0).then_some(end),
            },
        };
        packet.is_valid().then_some(packet)
    }
}

#[must_use]
pub fn is_weapon_state(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

#[derive(Default)]
pub struct WeaponStateInbox {
    latest: Option<WeaponStatePacket>,
}

impl WeaponStateInbox {
    /// # Errors
    /// Rejects malformed/foreign state and command-high-water regression before updating the HUD.
    pub fn receive(&mut self, session_id: u64, bytes: &[u8]) -> Result<bool, &'static str> {
        let packet = WeaponStatePacket::decode(bytes).ok_or("invalid weapon state")?;
        if packet.session_id != session_id {
            return Err("weapon state belongs to another session");
        }
        if let Some(prior) = self.latest {
            if packet.server_tick <= prior.server_tick {
                return Ok(false);
            }
            if packet.last_command_id < prior.last_command_id {
                return Err("weapon acknowledgement regressed");
            }
        }
        self.latest = Some(packet);
        Ok(true)
    }

    #[must_use]
    pub const fn latest(&self) -> Option<WeaponStatePacket> {
        self.latest
    }
}
