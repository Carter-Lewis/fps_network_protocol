use std::sync::atomic::{AtomicU16, AtomicU32};

pub static NEXT_PLAYER_ID: AtomicU16 = AtomicU16::new(1);
pub static WORLD_TICK: AtomicU32 = AtomicU32::new(0);